use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[test]
fn cli_parser_depth_is_bounded() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("nested.ts");
    std::fs::write(
        &source,
        format!(
            "function main(): number {{ return {}1{}; }}",
            "(".repeat(2048),
            ")".repeat(2048)
        ),
    )
    .unwrap();
    for command in ["check", "run"] {
        let output = bounded_cli(command, &source);
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("parser recursion limit exceeded"),
            "{output:?}"
        );
    }
    std::fs::write(&source, "function main(): number { return 42; }").unwrap();
    let output = bounded_cli("run", &source);
    assert!(output.status.success(), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("42"),
        "{output:?}"
    );
}

fn bounded_cli(command: &str, source: &std::path::Path) -> std::process::Output {
    // File-backed output cannot fill a pipe while the parent waits for exit.
    let stdout = tempfile::tempfile().unwrap();
    let stderr = tempfile::tempfile().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_submilli"))
        .arg(command)
        .arg(source)
        .env("SUBMILLI_TELEMETRY", "0")
        .stdout(Stdio::from(stdout.try_clone().unwrap()))
        .stderr(Stdio::from(stderr.try_clone().unwrap()))
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            use std::io::{Read, Seek};
            let mut stdout = stdout;
            let mut stderr = stderr;
            stdout.rewind().unwrap();
            stderr.rewind().unwrap();
            let mut output = std::process::Output {
                status,
                stdout: Vec::new(),
                stderr: Vec::new(),
            };
            stdout.read_to_end(&mut output.stdout).unwrap();
            stderr.read_to_end(&mut output.stderr).unwrap();
            return output;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("CLI child timed out");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}
