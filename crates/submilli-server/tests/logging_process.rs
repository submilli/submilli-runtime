#![cfg(unix)]

mod common;

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use common::{BIN, free_port, signal, wait_for_exit};

struct Server(Child);

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn command(home: &Path, port: u16) -> Command {
    let mut command = Command::new(BIN);
    command
        .env_clear()
        .env("SUBMILLI_HOME", home)
        .env("SUBMILLI_TELEMETRY", "0")
        .args(["--allow-unauthenticated", "--bind", "127.0.0.1", "--port"])
        .arg(port.to_string());
    command
}

fn start(home: &Path, port: u16, extra: impl FnOnce(&mut Command)) -> Server {
    let mut command = command(home, port);
    command.stdout(Stdio::null()).stderr(Stdio::null());
    extra(&mut command);
    Server(command.spawn().unwrap())
}

fn wait_for(path: &Path, text: &str, server: &mut Server) -> String {
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        let content = std::fs::read_to_string(path).unwrap_or_default();
        if content.contains(text) {
            return content;
        }
        assert!(
            server.0.try_wait().unwrap().is_none(),
            "server exited while waiting for {text}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("no {text} in {}", path.display());
}

fn stop(server: &mut Server) {
    signal(&server.0, libc::SIGTERM);
    assert!(wait_for_exit(&mut server.0, Duration::from_secs(10)).success());
}

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[test]
fn config_env_and_flag_select_output_and_append_across_restarts() {
    let directory = tempfile::tempdir().unwrap();
    let config_log = directory.path().join("config.log");
    let env_log = directory.path().join("env.log");
    let flag_log = directory.path().join("flag.log");
    let config = directory.path().join("server.yaml");
    std::fs::write(
        &config,
        format!("logging:\n  file: {}\n", config_log.display()),
    )
    .unwrap();
    for selected in [&config_log, &env_log, &flag_log, &flag_log] {
        let port = free_port();
        let mut server = start(directory.path(), port, |command| {
            command.arg("--config").arg(&config);
            if selected != &config_log {
                command.env("SUBMILLI_LOG_FILE", &env_log);
            }
            if selected == &flag_log {
                command.arg("--log-file").arg(&flag_log);
            }
        });
        wait_for(selected, &format!("addr=127.0.0.1:{port}"), &mut server);
        stop(&mut server);
    }
    assert_eq!(
        std::fs::read_to_string(flag_log)
            .unwrap()
            .matches("submilli-server listening")
            .count(),
        2
    );
}

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[test]
fn sighup_reopens_a_rotated_file_and_sigterm_still_exits() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("server.log");
    let rotated = directory.path().join("rotated.log");
    let mut server = start(directory.path(), free_port(), |command| {
        command.arg("--log-file").arg(&path);
    });
    wait_for(&path, "submilli-server listening", &mut server);
    std::fs::rename(&path, &rotated).unwrap();
    signal(&server.0, libc::SIGHUP);
    let deadline = Instant::now() + Duration::from_secs(10);
    while !path.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(path.exists(), "SIGHUP did not create a fresh log");
    stop(&mut server);
    assert!(
        std::fs::read_to_string(path)
            .unwrap()
            .contains("SIGTERM received")
    );
    assert!(
        !std::fs::read_to_string(rotated)
            .unwrap()
            .contains("SIGTERM received")
    );
}

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[test]
fn failed_reopening_reports_to_stderr_and_keeps_logging_to_the_old_file() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("server.log");
    let rotated = directory.path().join("rotated.log");
    let errors = directory.path().join("stderr.log");
    let mut server = start(directory.path(), free_port(), |command| {
        command
            .arg("--log-file")
            .arg(&path)
            .stderr(std::fs::File::create(&errors).unwrap());
    });
    wait_for(&path, "submilli-server listening", &mut server);
    std::fs::rename(&path, &rotated).unwrap();
    std::fs::create_dir(&path).unwrap();
    signal(&server.0, libc::SIGHUP);
    let error = wait_for(&errors, "cannot reopen server log", &mut server);
    assert!(error.contains(&path.display().to_string()));
    stop(&mut server);
    assert!(
        std::fs::read_to_string(rotated)
            .unwrap()
            .contains("SIGTERM received")
    );
}

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[test]
fn stdout_is_logfmt_and_sighup_is_a_noop() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("stdout.log");
    let mut server = start(directory.path(), free_port(), |command| {
        command.stdout(std::fs::File::create(&path).unwrap());
    });
    let content = wait_for(&path, "submilli-server listening", &mut server);
    for line in content.lines() {
        assert!(line.starts_with("ts="));
        assert!(line.contains(" level="));
        assert!(line.contains(" stream=log target="));
        assert!(!line.contains('\u{1b}'));
    }
    signal(&server.0, libc::SIGHUP);
    stop(&mut server);
    assert!(
        std::fs::read_to_string(path)
            .unwrap()
            .contains("SIGTERM received")
    );
}

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[test]
fn fifo_replacement_does_not_block_reopening_or_shutdown() {
    use std::os::unix::ffi::OsStrExt;
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("server.log");
    let rotated = directory.path().join("rotated.log");
    let errors = directory.path().join("stderr.log");
    let mut server = start(directory.path(), free_port(), |command| {
        command
            .arg("--log-file")
            .arg(&path)
            .stderr(std::fs::File::create(&errors).unwrap());
    });
    wait_for(&path, "submilli-server listening", &mut server);
    std::fs::rename(&path, &rotated).unwrap();
    let name = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
    // SAFETY: name is a live NUL-terminated path and 0600 is a valid mode.
    assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
    signal(&server.0, libc::SIGHUP);
    wait_for(&errors, "cannot reopen server log", &mut server);
    let start = Instant::now();
    stop(&mut server);
    assert!(start.elapsed() < Duration::from_secs(5));
    assert!(
        std::fs::read_to_string(rotated)
            .unwrap()
            .contains("SIGTERM received")
    );
}

#[test]
fn an_unopenable_log_file_fails_startup() {
    let directory = tempfile::tempdir().unwrap();
    let path: PathBuf = directory.path().join("absent").join("server.log");
    let output = command(directory.path(), 0)
        .arg("--log-file")
        .arg(&path)
        .output()
        .unwrap();
    assert!(!output.status.success());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains("cannot open server log"));
    assert!(error.contains(&path.display().to_string()));
}

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[test]
fn rust_log_filters_process_output() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("server.log");
    let port = free_port();
    let mut server = start(directory.path(), port, |command| {
        command
            .arg("--log-file")
            .arg(&path)
            .env("RUST_LOG", "error");
    });
    common::wait_ready(port);
    stop(&mut server);
    assert!(std::fs::read_to_string(path).unwrap().is_empty());
}

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[test]
fn terminal_stdout_has_no_ansi_sequences() {
    use std::io::Read;
    use std::os::fd::{AsRawFd, FromRawFd};
    let directory = tempfile::tempdir().unwrap();
    let mut master = -1;
    let mut slave = -1;
    // SAFETY: openpty writes two valid descriptors to live integers. Null
    // optional arguments request default terminal settings and window size.
    assert_eq!(
        unsafe {
            libc::openpty(
                &mut master,
                &mut slave,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        },
        0
    );
    // SAFETY: successful openpty transfers these distinct owned descriptors.
    let mut master = unsafe { std::fs::File::from_raw_fd(master) };
    // SAFETY: fcntl operates on the live master descriptor and sets a valid flag.
    assert_eq!(
        unsafe { libc::fcntl(master.as_raw_fd(), libc::F_SETFL, libc::O_NONBLOCK) },
        0
    );
    // SAFETY: the slave descriptor is valid and has no other Rust owner.
    let slave = unsafe { std::fs::File::from_raw_fd(slave) };
    let port = free_port();
    let mut server = start(directory.path(), port, |command| {
        command.stdout(slave);
    });
    common::wait_ready(port);
    let mut bytes = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if let Err(error) = master.read_to_end(&mut bytes) {
            assert_eq!(error.kind(), std::io::ErrorKind::WouldBlock);
        }
        if String::from_utf8_lossy(&bytes).contains("submilli-server listening") {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    // Read while the slave is open: macOS may discard queued bytes on close.
    stop(&mut server);
    let output = String::from_utf8(bytes).unwrap();
    assert!(
        output.contains("submilli-server listening"),
        "PTY output: {output:?}"
    );
    assert!(!output.contains('\u{1b}'));
    assert!(output.lines().all(|line| line.starts_with("ts=")));
}
