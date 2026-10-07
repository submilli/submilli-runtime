//! `submilli playground` start, attach, status, open, and stop, driven through the
//! real binary: the detached child, the lock and its nonce, both loopback listeners,
//! and the credentials that must never appear in output, links, or files.

use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_submilli");

#[cfg(windows)]
#[test]
fn playground_is_not_supported_on_windows() {
    let dir = tempfile::tempdir().expect("tempdir");
    let output = Command::new(BIN)
        .arg("playground")
        .current_dir(dir.path())
        .output()
        .expect("run submilli");
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("not supported"), "stderr: {stderr}");
}

#[test]
fn playground_outside_a_project_names_init() {
    let dir = tempfile::tempdir().expect("tempdir");
    let output = Command::new(BIN)
        .args(["playground", "start", "--json"])
        .current_dir(dir.path())
        .env("SUBMILLI_HOME", dir.path().join("home"))
        .output()
        .expect("run submilli");
    if cfg!(windows) {
        assert_eq!(output.status.code(), Some(1));
        return;
    }
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("playground init"), "stderr: {stderr}");
}

#[cfg(unix)]
mod unix {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Output, Stdio};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use serde_json::{Value, json};

    use super::BIN;

    const BLUEPRINT: &str = "name: demo\nallow_insecure_http: true\npermissions:\n  main:\n    - capability: http.get\n      action: allow\n";

    const PROVIDER_KEYS: [&str; 5] = [
        "ANTHROPIC_API_KEY",
        "OPENAI_API_KEY",
        "GEMINI_API_KEY",
        "GOOGLE_API_KEY",
        "SUBMILLI_PLAYGROUND_MODEL",
    ];

    /// A project in a temporary directory, with a home of its own so nothing reads
    /// or writes the developer's real stores. Dropping it stops any playground it
    /// started, so a failed test leaves no process behind.
    struct Project {
        dir: tempfile::TempDir,
        root: PathBuf,
    }

    impl Project {
        /// `submilli/submilli.toml` with the one blueprint under `submilli/blueprints/`.
        fn new() -> Self {
            Self::with_blueprint(BLUEPRINT)
        }

        fn with_blueprint(yaml: &str) -> Self {
            let dir = tempfile::tempdir().expect("tempdir");
            let root = dir.path().join("project");
            std::fs::create_dir_all(root.join("submilli/blueprints")).unwrap();
            std::fs::write(
                root.join("submilli/submilli.toml"),
                "[package]\nname = \"@demo/app\"\n",
            )
            .unwrap();
            std::fs::write(root.join("submilli/blueprints/demo.yaml"), yaml).unwrap();
            Self { dir, root }
        }

        fn state(&self) -> PathBuf {
            self.root.join(".submilli/playground")
        }

        fn token(&self, name: &str) -> String {
            std::fs::read_to_string(self.state().join("tokens").join(name))
                .unwrap()
                .trim()
                .to_owned()
        }

        fn command(&self, args: &[&str]) -> Command {
            let mut command = Command::new(BIN);
            command
                .arg("playground")
                .args(args)
                .current_dir(&self.root)
                .env("SUBMILLI_HOME", self.dir.path().join("home"))
                .env("HOME", self.dir.path().join("home"))
                .env_remove("SUBMILLI_TELEMETRY")
                .env_remove("SUBMILLI_ALLOW_LOCALHOST")
                .env_remove("SUBMILLI_ALLOW_PRIVATE")
                .env_remove("SUBMILLI_ALLOW_IP");
            for key in PROVIDER_KEYS {
                command.env_remove(key);
            }
            command
        }

        fn run(&self, args: &[&str]) -> Output {
            self.command(args).output().expect("run submilli")
        }

        /// `start --json` that must succeed; returns its ready record.
        fn start(&self, extra: &[&str]) -> Value {
            let mut args = vec!["start", "--json"];
            args.extend_from_slice(extra);
            let output = self.run(&args);
            assert!(
                output.status.success(),
                "start failed: {}\nstdout: {}",
                stderr(&output),
                stdout(&output)
            );
            parse(&output)
        }

        fn stop(&self) -> Output {
            self.run(&["stop", "--json"])
        }

        fn lock(&self) -> PathBuf {
            self.state().join("lock")
        }

        fn ready_file(&self) -> PathBuf {
            self.state().join("ready")
        }
    }

    impl Drop for Project {
        fn drop(&mut self) {
            if self.lock().exists() {
                let _ = self.stop();
            }
            // Whatever still holds the instance lock (a child that never got ready,
            // a helper holder) is ended too.
            if let Some(pid) = instance_holder(self) {
                terminate(pid);
                let deadline = Instant::now() + Duration::from_secs(10);
                while instance_holder(self).is_some() && Instant::now() < deadline {
                    std::thread::sleep(Duration::from_millis(50));
                }
            }
        }
    }

    fn terminate(pid: u32) {
        let _ = Command::new("kill")
            .args(["-TERM", &pid.to_string()])
            .stderr(Stdio::null())
            .status();
    }

    fn stdout(output: &Output) -> String {
        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    fn stderr(output: &Output) -> String {
        String::from_utf8_lossy(&output.stderr).into_owned()
    }

    fn parse(output: &Output) -> Value {
        serde_json::from_slice(&output.stdout)
            .unwrap_or_else(|error| panic!("not JSON ({error}): {}", stdout(output)))
    }

    fn agent() -> ureq::Agent {
        ureq::Agent::config_builder()
            .http_status_as_error(false)
            .max_redirects(0)
            .timeout_global(Some(Duration::from_secs(30)))
            .build()
            .into()
    }

    /// Status code, headers, and body of a request with optional bearer token and
    /// extra headers.
    fn request(
        method: &str,
        url: &str,
        token: Option<&str>,
        headers: &[(&str, &str)],
        body: Option<Value>,
    ) -> (u16, ureq::http::HeaderMap, String) {
        let mut builder = ureq::http::Request::builder().method(method).uri(url);
        if let Some(token) = token {
            builder = builder.header("authorization", format!("Bearer {token}"));
        }
        for (name, value) in headers {
            builder = builder.header(*name, *value);
        }
        let agent = agent();
        let mut response = match body {
            Some(body) => agent
                .run(
                    builder
                        .header("content-type", "application/json")
                        .body(body.to_string())
                        .unwrap(),
                )
                .unwrap(),
            None => agent.run(builder.body(()).unwrap()).unwrap(),
        };
        let status = response.status().as_u16();
        let headers = response.headers().clone();
        let text = response.body_mut().read_to_string().unwrap_or_default();
        (status, headers, text)
    }

    fn login_code(record: &Value) -> String {
        let link = record["login_url"].as_str().expect("login_url");
        link.split_once("#login=").map_or_else(
            || panic!("no login code in {link}"),
            |(_, code)| code.to_owned(),
        )
    }

    fn control(record: &Value, path: &str) -> String {
        format!(
            "{}/{}",
            record["url"].as_str().unwrap().trim_end_matches('/'),
            path.trim_start_matches('/')
        )
    }

    fn server(record: &Value, path: &str) -> String {
        format!("{}{path}", record["server_url"].as_str().unwrap())
    }

    fn exchange(record: &Value, code: &str) -> (u16, ureq::http::HeaderMap, String) {
        request(
            "POST",
            &control(record, "/api/login"),
            None,
            &[],
            Some(json!({ "code": code })),
        )
    }

    fn execute(record: &Value, token: &str, code: &str) -> Value {
        let (status, _, body) = request(
            "POST",
            &server(record, "/v1/execute"),
            Some(token),
            &[],
            Some(json!({ "code": code, "blueprint": "demo" })),
        );
        assert_eq!(status, 200, "execute: {body}");
        serde_json::from_str(&body).unwrap()
    }

    fn pid_alive(pid: u32) -> bool {
        Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }

    fn wait_until(what: &str, timeout: Duration, mut done: impl FnMut() -> bool) {
        let deadline = Instant::now() + timeout;
        while !done() {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn mode(path: &Path) -> u32 {
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    fn assert_private(path: &Path) {
        for entry in std::fs::read_dir(path).unwrap() {
            let path = entry.unwrap().path();
            let meta = std::fs::symlink_metadata(&path).unwrap();
            if meta.is_dir() {
                assert_eq!(mode(&path), 0o700, "{}", path.display());
                assert_private(&path);
            } else {
                assert_eq!(mode(&path), 0o600, "{}", path.display());
            }
        }
    }

    /// A listener that answers every connection with `reply` and keeps what each
    /// request sent, so a test can prove no credential reached it.
    fn impostor(reply: &'static str) -> (u16, Arc<Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let record = Arc::clone(&seen);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
                let mut buffer = vec![0_u8; 8192];
                let read = stream.read(&mut buffer).unwrap_or(0);
                record
                    .lock()
                    .unwrap()
                    .push(String::from_utf8_lossy(&buffer[..read]).into_owned());
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",
                    reply.len()
                );
            }
        });
        (port, seen)
    }

    fn write_lock(project: &Project, pid: u32, port: u16) {
        std::fs::create_dir_all(project.state()).unwrap();
        std::fs::write(
            project.lock(),
            json!({
                "pid": pid,
                "control_port": port,
                "server_port": port,
                "nonce": "00".repeat(32),
            })
            .to_string(),
        )
        .unwrap();
    }

    fn dead_pid() -> u32 {
        let mut child = Command::new("true").spawn().unwrap();
        let pid = child.id();
        child.wait().unwrap();
        pid
    }

    /// A whole-file `fcntl` write lock request, as the playground makes.
    fn whole_file_write_lock() -> libc::flock {
        // SAFETY: all-zero bytes are a valid `flock`; start 0, length 0 is the file.
        let mut lock: libc::flock = unsafe { std::mem::zeroed() };
        lock.l_type = libc::F_WRLCK as libc::c_short;
        lock.l_whence = libc::SEEK_SET as libc::c_short;
        lock
    }

    fn open_instance_lock(path: &Path) -> std::fs::File {
        use std::os::unix::fs::OpenOptionsExt;
        std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .mode(0o600)
            .open(path)
            .unwrap()
    }

    /// The instance lock, held by this test process as a serving playground holds
    /// it, until dropped. This process must not open the file again meanwhile: a
    /// record lock goes when any of the process's descriptors of the file closes.
    fn hold_instance_lock(project: &Project) -> std::fs::File {
        use std::os::fd::AsRawFd;
        std::fs::create_dir_all(project.state()).unwrap();
        let file = open_instance_lock(&project.state().join("instance.lock"));
        let mut lock = whole_file_write_lock();
        // SAFETY: a live descriptor and a live `flock`.
        let taken = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETLK, &raw mut lock) };
        assert_eq!(taken, 0, "{}", std::io::Error::last_os_error());
        file
    }

    /// The pid of another process holding the project's instance lock, asked of the
    /// kernel without taking the lock.
    fn instance_holder(project: &Project) -> Option<u32> {
        use std::os::fd::AsRawFd;
        let file = std::fs::File::open(project.state().join("instance.lock")).ok()?;
        let mut lock = whole_file_write_lock();
        // SAFETY: a live descriptor and a live `flock`.
        let asked = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETLK, &raw mut lock) };
        assert_eq!(asked, 0, "{}", std::io::Error::last_os_error());
        (lock.l_type != libc::F_UNLCK as libc::c_short).then(|| u32::try_from(lock.l_pid).unwrap())
    }

    fn instance_held(project: &Project) -> bool {
        instance_holder(project).is_some()
    }

    const HOLDER_ENV: &str = "SUBMILLI_TEST_INSTANCE_LOCK_HOLDER";
    const HOLDER_IGNORES_TERM_ENV: &str = "SUBMILLI_TEST_INSTANCE_LOCK_HOLDER_IGNORES_TERM";

    /// Run as its own process by [`spawn_holder`]: take the instance lock at
    /// `$SUBMILLI_TEST_INSTANCE_LOCK_HOLDER` as a starting playground does, then wait
    /// to be killed. Does nothing when run any other way.
    #[test]
    #[ignore = "a helper process the lifecycle tests start"]
    fn instance_lock_holder_process() {
        use std::os::fd::AsRawFd;
        let Some(path) = std::env::var_os(HOLDER_ENV).map(PathBuf::from) else {
            return;
        };
        let file = open_instance_lock(&path);
        let mut lock = whole_file_write_lock();
        // SAFETY: a live descriptor and a live `flock`.
        let taken = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETLKW, &raw mut lock) };
        assert_eq!(taken, 0, "{}", std::io::Error::last_os_error());
        let record = json!({ "pid": std::process::id(), "stopping": false }).to_string();
        std::os::unix::fs::FileExt::write_all_at(&file, record.as_bytes(), 0).unwrap();
        if std::env::var_os(HOLDER_IGNORES_TERM_ENV).is_some() {
            // SAFETY: setting a disposition touches no memory of this process.
            unsafe {
                libc::signal(libc::SIGTERM, libc::SIG_IGN);
            }
        }
        std::fs::write(path.with_extension("held"), "").unwrap();
        std::thread::sleep(Duration::from_secs(120));
        drop(file);
    }

    /// A separate process holding the project's instance lock, killed on drop.
    struct Holder(std::process::Child);

    impl Holder {
        fn pid(&self) -> u32 {
            self.0.id()
        }
    }

    impl Drop for Holder {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    fn spawn_holder(project: &Project, ignore_term: bool) -> Holder {
        std::fs::create_dir_all(project.state()).unwrap();
        let path = project.state().join("instance.lock");
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "unix::instance_lock_holder_process",
                "--ignored",
                "--test-threads=1",
            ])
            .env(HOLDER_ENV, &path)
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if ignore_term {
            command.env(HOLDER_IGNORES_TERM_ENV, "1");
        }
        let holder = Holder(command.spawn().unwrap());
        wait_until(
            "the helper to hold the lock",
            Duration::from_secs(30),
            || path.with_extension("held").exists(),
        );
        assert_eq!(instance_holder(project), Some(holder.pid()));
        holder
    }

    /// `output()`, failing the test rather than hanging when the command does not
    /// end within `timeout`.
    fn run_within(project: &Project, args: &[&str], timeout: Duration) -> Output {
        let mut child = project
            .command(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + timeout;
        while child.try_wait().unwrap().is_none() {
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!("{args:?} did not end within {timeout:?}");
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        child.wait_with_output().unwrap()
    }

    /// Every path under `dir`, relative to it.
    fn tree(dir: &Path) -> Vec<PathBuf> {
        let mut paths = Vec::new();
        let mut stack = vec![dir.to_path_buf()];
        while let Some(next) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&next) else {
                continue;
            };
            for entry in entries {
                let path = entry.unwrap().path();
                paths.push(path.strip_prefix(dir).unwrap().to_path_buf());
                stack.push(path);
            }
        }
        paths.sort();
        paths
    }

    /// A listener that accepts every connection and never answers.
    fn silent_listener() -> u16 {
        silent_listener_reporting().0
    }

    /// [`silent_listener`], with a message for each connection it accepts.
    fn silent_listener_reporting() -> (u16, std::sync::mpsc::Receiver<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let (accepted, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut open = Vec::new();
            for stream in listener.incoming() {
                let Ok(stream) = stream else { return };
                open.push(stream);
                let _ = accepted.send(());
            }
        });
        (port, receiver)
    }

    /// The answer the real control listener gives a challenge under `nonce`.
    fn challenge_answer(nonce: &str, challenge: &str) -> String {
        use hmac::{Hmac, Mac};
        let mut mac = Hmac::<sha2::Sha256>::new_from_slice(nonce.as_bytes()).unwrap();
        mac.update(b"submilli-playground-challenge/1\0");
        mac.update(challenge.to_ascii_lowercase().as_bytes());
        mac.finalize()
            .into_bytes()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    /// A control listener that answers the challenge under the test lock's nonce,
    /// then answers each other route from `routes` (path to body).
    fn convincing_impostor(routes: &'static [(&'static str, &'static str)]) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                let mut request = Vec::new();
                let mut buffer = [0_u8; 4096];
                let body = loop {
                    let Ok(read) = stream.read(&mut buffer) else {
                        break None;
                    };
                    if read == 0 {
                        break None;
                    }
                    request.extend_from_slice(&buffer[..read]);
                    let text = String::from_utf8_lossy(&request).into_owned();
                    let Some((head, body)) = text.split_once("\r\n\r\n") else {
                        continue;
                    };
                    let length = head
                        .lines()
                        .find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            name.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().ok())?
                        })
                        .unwrap_or(0);
                    if body.len() >= length {
                        break Some((head.to_owned(), body.to_owned()));
                    }
                };
                let Some((head, body)) = body else { continue };
                let path = head
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or_default()
                    .to_owned();
                let reply = if path == "/api/challenge" {
                    let challenge: Value = serde_json::from_str(&body).unwrap_or_default();
                    let challenge = challenge["challenge"].as_str().unwrap_or_default();
                    json!({ "response": challenge_answer(&"00".repeat(32), challenge) }).to_string()
                } else {
                    routes
                        .iter()
                        .find(|(route, _)| *route == path)
                        .map_or("{}", |(_, body)| body)
                        .to_owned()
                };
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",
                    reply.len()
                );
            }
        });
        port
    }

    fn read_lock_file(project: &Project) -> String {
        std::fs::read_to_string(project.lock()).unwrap()
    }

    #[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
    #[test]
    fn fresh_start_creates_private_state_and_prints_a_ready_record() {
        let project = Project::new();
        let started = Instant::now();
        let record = project.start(&[]);
        assert!(started.elapsed() < Duration::from_secs(30));

        let state = project.state();
        assert_eq!(mode(&state), 0o700);
        assert_eq!(
            std::fs::read_to_string(state.join(".gitignore")).unwrap(),
            "*\n"
        );
        assert_private(&state);

        assert_eq!(record["blueprint"]["name"], "demo");
        assert_eq!(
            Path::new(record["project"].as_str().unwrap()),
            project.root.canonicalize().unwrap()
        );
        assert!(
            record["url"]
                .as_str()
                .unwrap()
                .starts_with("http://127.0.0.1:")
        );
        assert!(
            record["server_url"]
                .as_str()
                .unwrap()
                .starts_with("http://127.0.0.1:")
        );
        let app_token_file = PathBuf::from(record["app_token_file"].as_str().unwrap());
        assert_eq!(
            app_token_file,
            state.canonicalize().unwrap().join("tokens/app")
        );
        assert!(login_code(&record).len() >= 32);

        // The playground outlives the command that started it.
        let (status, _, _) = request("GET", &server(&record, "/healthz"), None, &[], None);
        assert_eq!(status, 200);
        let log = PathBuf::from(record["log_file"].as_str().unwrap());
        assert_eq!(mode(&log), 0o600);

        // The text form carries the same essentials.
        let text = project.run(&["start"]);
        assert!(text.status.success(), "{}", stderr(&text));
        let text = stdout(&text);
        assert!(text.contains("demo"), "{text}");
        assert!(text.contains("#login="), "{text}");
        assert!(text.contains("tokens/app"), "{text}");
    }

    #[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
    #[test]
    fn a_second_start_attaches_with_a_fresh_working_login_link() {
        let project = Project::new();
        let first = project.start(&[]);
        let second = project.start(&[]);
        assert_eq!(first["pid"], second["pid"]);
        assert_eq!(first["url"], second["url"]);
        assert_eq!(first["server_url"], second["server_url"]);
        assert_ne!(first["login_url"], second["login_url"]);
        assert_eq!(second["attached"], true);
        let (status, _, body) = exchange(&second, &login_code(&second));
        assert_eq!(status, 200, "{body}");
        // A start is not a restart: the first link still works too.
        let (status, _, body) = exchange(&first, &login_code(&first));
        assert_eq!(status, 200, "{body}");
    }

    #[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
    #[test]
    fn a_stale_lock_with_a_dead_pid_is_replaced() {
        let project = Project::new();
        let (port, _) = impostor("{}");
        write_lock(&project, dead_pid(), port);
        let record = project.start(&[]);
        assert_ne!(record["attached"], true);
        let lock: Value =
            serde_json::from_str(&std::fs::read_to_string(project.lock()).unwrap()).unwrap();
        assert_eq!(lock["pid"], record["pid"]);
    }

    #[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
    #[test]
    fn a_stale_lock_whose_live_pid_fails_the_nonce_is_replaced() {
        let project = Project::new();
        let (port, seen) = impostor(r#"{"response":"00"}"#);
        write_lock(&project, std::process::id(), port);
        let record = project.start(&[]);
        assert_ne!(record["pid"], json!(std::process::id()));
        for request in seen.lock().unwrap().iter() {
            assert!(
                !request.to_ascii_lowercase().contains("authorization"),
                "a credential reached the impostor: {request}"
            );
        }
    }

    #[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
    #[test]
    fn requests_need_a_token_and_admin_routes_need_the_admin_token() {
        let project = Project::new();
        let record = project.start(&[]);
        let stand_in = project.token("stand-in");

        for (method, path) in [("GET", "/v1/status"), ("POST", "/v1/execute")] {
            let (status, _, _) = request(method, &server(&record, path), None, &[], None);
            assert_eq!(status, 401, "{method} {path}");
        }
        let (status, _, _) = request(
            "GET",
            &server(&record, "/v1/status"),
            Some(&stand_in),
            &[],
            None,
        );
        assert_eq!(status, 403);

        for (method, path) in [
            ("GET", "/api/status"),
            ("POST", "/api/login-codes"),
            ("POST", "/api/stop"),
            ("GET", "/api/anything-else"),
        ] {
            let (status, headers, _) = request(method, &control(&record, path), None, &[], None);
            assert_eq!(status, 401, "{method} {path}");
            assert_eq!(headers["x-content-type-options"], "nosniff");
            assert!(
                headers["content-type"]
                    .to_str()
                    .unwrap()
                    .starts_with("application/json")
            );
        }
        for (method, path) in [("POST", "/api/login-codes"), ("POST", "/api/stop")] {
            let (status, _, _) =
                request(method, &control(&record, path), Some(&stand_in), &[], None);
            assert_eq!(status, 403, "{method} {path}");
        }

        // The page, the login-code exchange, and the nonce challenge need none.
        let (status, _, _) = request("GET", &control(&record, "/"), None, &[], None);
        assert_eq!(status, 200);
        let (status, _, body) = exchange(&record, "not-a-code");
        assert_ne!(status, 401, "{body}");
        let (status, _, body) = request(
            "POST",
            &control(&record, "/api/challenge"),
            None,
            &[],
            Some(json!({ "challenge": "ab".repeat(32) })),
        );
        assert_eq!(status, 200, "{body}");
    }

    #[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
    #[test]
    fn a_deeply_recursive_program_ends_with_a_resource_error_and_the_playground_stays_up() {
        let project = Project::new();
        let record = project.start(&[]);
        let app = project.token("app");
        let body = execute(
            &record,
            &app,
            "function f(n: number): number { return f(n + 1) + 1; }\nfunction main(): number { return f(0); }",
        );
        assert_eq!(body["error"]["kind"], "stack_exhausted", "{body}");
        let body = execute(&record, &app, "function main(): number { return 2; }");
        assert_eq!(body["result"], "2", "{body}");
        let status = project.run(&["status", "--json"]);
        assert!(status.status.success(), "{}", stderr(&status));
    }

    #[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
    #[test]
    fn a_login_code_works_once_and_its_session_is_not_a_server_credential() {
        let project = Project::new();
        let record = project.start(&[]);
        let code = login_code(&record);

        let (status, headers, body) = exchange(&record, &code);
        assert_eq!(status, 200, "{body}");
        assert!(headers.get("set-cookie").is_none());
        let session: Value = serde_json::from_str(&body).unwrap();
        let session = session["session_token"].as_str().unwrap().to_owned();
        for name in ["admin", "stand-in", "app", "chat", "browser-chat"] {
            assert_ne!(session, project.token(name));
            assert!(!session.contains(&project.token(name)));
        }

        let (status, _, _) = exchange(&record, &code);
        assert_ne!(status, 200, "a login code works only once");

        let (status, _, body) = request(
            "GET",
            &control(&record, "/api/status"),
            Some(&session),
            &[],
            None,
        );
        assert_eq!(status, 200, "{body}");
        let (status, _, _) = request(
            "GET",
            &server(&record, "/v1/status"),
            Some(&session),
            &[],
            None,
        );
        assert_eq!(status, 401);
        let (status, _, _) = request(
            "POST",
            &server(&record, "/v1/execute"),
            Some(&session),
            &[],
            Some(json!({ "code": "function main(): number { return 1; }", "blueprint": "demo" })),
        );
        assert_eq!(status, 401);
    }

    #[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
    #[test]
    fn a_command_whose_listener_fails_the_nonce_sends_no_credential_and_exits_6() {
        let project = Project::new();
        // Tokens exist, so the command has a credential it could send.
        project.start(&[]);
        assert!(project.stop().status.success());

        let (port, seen) = impostor(r#"{"response":"00"}"#);
        write_lock(&project, std::process::id(), port);
        // A process holds the instance lock, so the lock is not simply stale and the
        // challenge is what decides.
        let _held = hold_instance_lock(&project);
        for command in ["status", "open", "stop"] {
            let output = project.run(&[command, "--json"]);
            assert_eq!(
                output.status.code(),
                Some(6),
                "{command}: {}",
                stderr(&output)
            );
        }
        let requests = seen.lock().unwrap();
        assert!(!requests.is_empty(), "the challenge was never sent");
        for request in requests.iter() {
            assert!(
                !request.to_ascii_lowercase().contains("authorization"),
                "a credential reached the impostor: {request}"
            );
        }
    }

    /// One `200 ok` response per connection, on loopback.
    fn ok_fixture() -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                let mut buffer = [0_u8; 4096];
                let _ = stream.read(&mut buffer);
                let _ = stream.write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok",
                );
            }
        });
        port
    }

    #[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
    #[test]
    fn loopback_egress_is_blocked_by_default_and_allowed_with_the_flag() {
        let port = ok_fixture();
        let program = format!(
            "import {{ get, Response }} from \"submilli:http\";\nfunction main(): string {{ const r: Response = get(\"http://127.0.0.1:{port}/\"); return r.body; }}"
        );
        let project = Project::new();

        let record = project.start(&[]);
        assert_eq!(record["egress_grants"], json!([]));
        let body = execute(&record, &project.token("app"), &program);
        let message = body["error"]["message"].as_str().unwrap_or_default();
        assert!(message.contains("--allow-localhost"), "{body}");
        assert!(project.stop().status.success());

        let record = project.start(&["--allow-localhost", "--allow-ip", "10.1.0.0/16"]);
        let grants = record["egress_grants"].as_array().unwrap();
        assert!(grants.contains(&json!("allow-localhost")), "{record}");
        assert!(grants.contains(&json!("allow-ip 10.1.0.0/16")), "{record}");
        let body = execute(&record, &project.token("app"), &program);
        assert_eq!(body["result"], "ok", "{body}");
        let status = parse(&project.run(&["status", "--json"]));
        assert_eq!(status["egress_grants"], record["egress_grants"]);
    }

    #[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
    #[test]
    fn no_output_or_state_file_shows_the_admin_token() {
        let project = Project::new();
        let mut texts = Vec::new();
        for args in [
            &["start", "--json"][..],
            &["start"],
            &["status"],
            &["status", "--json"],
            &["open"],
            &["open", "--json"],
        ] {
            let output = project.run(args);
            assert!(output.status.success(), "{args:?}: {}", stderr(&output));
            texts.push(stdout(&output));
            texts.push(stderr(&output));
        }
        let ready = std::fs::read_to_string(project.ready_file()).unwrap();
        texts.push(std::fs::read_to_string(project.lock()).unwrap());
        texts.push(ready.clone());
        let stop = project.stop();
        texts.push(stdout(&stop));
        texts.push(stderr(&stop));

        let tokens: Vec<String> = ["admin", "stand-in", "app", "chat", "browser-chat"]
            .iter()
            .map(|name| project.token(name))
            .collect();
        for text in &texts {
            assert!(!text.contains(&tokens[0]), "the admin token leaked: {text}");
        }
        // The ready file carries neither a login code nor any token.
        let ready: Value = serde_json::from_str(&ready).unwrap();
        let keys: Vec<&String> = ready.as_object().unwrap().keys().collect();
        assert!(
            keys.iter()
                .all(|key| !key.contains("login") && !key.contains("token"))
        );
        let ready = ready.to_string();
        for token in &tokens {
            assert!(!ready.contains(token.as_str()));
        }
    }

    #[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
    #[test]
    fn control_requests_from_a_foreign_origin_or_host_are_refused() {
        let project = Project::new();
        let record = project.start(&[]);
        let admin = project.token("admin");
        let url = control(&record, "/api/status");
        let port = url.split(':').nth(2).unwrap().split('/').next().unwrap();

        let (status, _, _) = request("GET", &url, Some(&admin), &[], None);
        assert_eq!(status, 200);
        let own_origin = format!("http://127.0.0.1:{port}");
        let (status, _, _) = request("GET", &url, Some(&admin), &[("origin", &own_origin)], None);
        assert_eq!(status, 200);
        let (status, _, _) = request(
            "GET",
            &url,
            Some(&admin),
            &[("origin", "http://evil.example")],
            None,
        );
        assert_eq!(status, 403);
        let other_port = format!("http://127.0.0.1:{}", port.parse::<u16>().unwrap() ^ 1);
        let (status, _, _) = request("GET", &url, Some(&admin), &[("origin", &other_port)], None);
        assert_eq!(status, 403);
        let host = format!("evil.example:{port}");
        let (status, _, _) = request("GET", &url, Some(&admin), &[("host", &host)], None);
        assert_eq!(status, 403);
        let (status, _, _) = request(
            "POST",
            &control(&record, "/api/login"),
            None,
            &[("origin", "http://evil.example")],
            Some(json!({ "code": login_code(&record) })),
        );
        assert_eq!(status, 403);
        // A sandboxed frame or a file page sends `Origin: null`.
        let (status, _, _) = request("GET", &url, Some(&admin), &[("origin", "null")], None);
        assert_eq!(status, 403);
        let (status, _, _) = request(
            "POST",
            &control(&record, "/api/login"),
            None,
            &[("origin", "null")],
            Some(json!({ "code": login_code(&record) })),
        );
        assert_eq!(status, 403);
        // The other loopback spellings of this listener pass.
        for host in [format!("localhost:{port}"), format!("[::1]:{port}")] {
            let (status, _, _) = request("GET", &url, Some(&admin), &[("host", &host)], None);
            assert_eq!(status, 200, "{host}");
        }
        let (status, _, _) = request("GET", &url, Some(&admin), &[("host", "127.0.0.1")], None);
        assert_eq!(status, 403);

        // A garbage or huge credential is refused, and the listener stays up.
        let huge = format!("Bearer {}", "a".repeat(32 * 1024));
        for value in ["garbage", "Bearer", "Basic YWRtaW46YWRtaW4=", huge.as_str()] {
            let (status, _, _) = request("GET", &url, None, &[("authorization", value)], None);
            assert!(
                status == 401 || status == 431,
                "{}: {status}",
                &value[..value.len().min(30)]
            );
        }
        let (status, _, _) = request("GET", &url, Some(&admin), &[], None);
        assert_eq!(status, 200);
    }

    #[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
    #[test]
    fn status_reports_the_running_playground_and_stop_ends_it() {
        let project = Project::new();
        let record = project.start(&[]);
        let tokens_before = project.token("admin");
        let (_, _, body) = exchange(&record, &login_code(&record));
        let session: Value = serde_json::from_str(&body).unwrap();
        let session = session["session_token"].as_str().unwrap().to_owned();

        let output = project.run(&["status", "--json"]);
        assert!(output.status.success(), "{}", stderr(&output));
        let status = parse(&output);
        assert_eq!(status["running"], true);
        assert_eq!(status["url"], record["url"]);
        assert_eq!(status["pid"], record["pid"]);
        assert_eq!(status["blueprint"]["name"], "demo");
        assert_eq!(status["egress_grants"], json!([]));
        assert_eq!(status["health"], "ok");
        assert_eq!(status["provider"], Value::Null);

        let text = project.run(&["status"]);
        assert!(stdout(&text).contains("running"), "{}", stdout(&text));

        let output = project.stop();
        assert!(output.status.success(), "{}", stderr(&output));
        assert!(!project.lock().exists());
        assert!(!project.ready_file().exists());
        let pid = u32::try_from(record["pid"].as_u64().unwrap()).unwrap();
        wait_until("the playground to exit", Duration::from_secs(10), || {
            !pid_alive(pid)
        });

        let output = project.run(&["status", "--json"]);
        assert_eq!(output.status.code(), Some(6));
        assert_eq!(parse(&output)["running"], false);
        // Stopping again is not an error.
        assert!(project.stop().status.success());

        // Tokens survive a restart; browser sessions do not.
        let record = project.start(&[]);
        assert_eq!(project.token("admin"), tokens_before);
        let (status, _, _) = request(
            "GET",
            &control(&record, "/api/status"),
            Some(&session),
            &[],
            None,
        );
        assert_eq!(status, 401);
    }

    #[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
    #[test]
    fn status_reports_the_detected_provider() {
        let project = Project::new();
        let output = project
            .command(&["start", "--json"])
            .env("ANTHROPIC_API_KEY", "sk-test-not-a-key")
            .output()
            .unwrap();
        assert!(output.status.success(), "{}", stderr(&output));
        let record = parse(&output);
        assert_eq!(record["provider"], "anthropic");
        let status = parse(&project.run(&["status", "--json"]));
        assert_eq!(status["provider"], "anthropic");
        assert!(
            !stdout(&project.run(&["status"])).contains("sk-test-not-a-key"),
            "a provider key was printed"
        );
    }

    #[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
    #[test]
    fn a_child_that_cannot_load_its_blueprint_fails_start_and_leaves_no_lock() {
        let project = Project::with_blueprint("name: [unclosed\n");
        let output = project.run(&["start", "--json"]);
        assert_eq!(output.status.code(), Some(1), "{}", stdout(&output));
        assert!(stderr(&output).contains("demo.yaml"), "{}", stderr(&output));
        assert!(stdout(&output).trim().is_empty(), "{}", stdout(&output));
        assert!(!project.lock().exists());
    }

    #[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
    #[test]
    fn after_stop_a_start_that_never_gets_ready_exits_1_and_kills_its_child() {
        let project = Project::new();
        let previous = project.start(&[]);
        assert!(project.stop().status.success());

        let output = project.run(&["start", "--json", "--ready-timeout-ms", "1"]);
        assert_eq!(output.status.code(), Some(1));
        let out = stdout(&output);
        assert!(!out.contains(previous["url"].as_str().unwrap()), "{out}");
        assert!(stderr(&output).contains("not ready"), "{}", stderr(&output));
        assert!(!project.lock().exists());
        assert!(!project.ready_file().exists());

        // A broken blueprint fails the same way.
        std::fs::write(
            project.root.join("submilli/blueprints/demo.yaml"),
            "name: [unclosed\n",
        )
        .unwrap();
        let output = project.run(&["start", "--json"]);
        assert_eq!(output.status.code(), Some(1));
        assert!(!stdout(&output).contains(previous["url"].as_str().unwrap()));
        assert!(!project.lock().exists());
    }

    #[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
    #[test]
    fn foreground_serves_until_stop_or_a_signal() {
        let project = Project::new();
        for ending in ["stop", "signal"] {
            let mut child = project
                .command(&["start", "--foreground", "--json"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap();
            wait_until("the foreground playground", Duration::from_secs(30), || {
                project.ready_file().exists()
            });
            let status = project.run(&["status", "--json"]);
            assert!(status.status.success(), "{}", stderr(&status));
            assert_eq!(parse(&status)["pid"], json!(child.id()));
            assert!(child.try_wait().unwrap().is_none());

            if ending == "stop" {
                assert!(project.stop().status.success());
            } else {
                Command::new("kill")
                    .args(["-TERM", &child.id().to_string()])
                    .status()
                    .unwrap();
            }
            let deadline = Instant::now() + Duration::from_secs(15);
            let exit = loop {
                if let Some(exit) = child.try_wait().unwrap() {
                    break exit;
                }
                assert!(
                    Instant::now() < deadline,
                    "foreground did not end on {ending}"
                );
                std::thread::sleep(Duration::from_millis(50));
            };
            assert!(exit.success(), "{ending}: {exit:?}");
            assert!(!project.lock().exists(), "{ending}");
            assert!(!project.ready_file().exists(), "{ending}");
        }
    }

    #[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
    #[test]
    fn start_returns_to_a_caller_that_waits_for_end_of_output() {
        let project = Project::new();
        let (sender, receiver) = std::sync::mpsc::channel();
        let mut command = project.command(&["start", "--json"]);
        std::thread::spawn(move || {
            // `output` reads both pipes to their end, as a shell tool does.
            let _ = sender.send(command.output());
        });
        let output = receiver
            .recv_timeout(Duration::from_secs(60))
            .expect("start held its output pipes open")
            .unwrap();
        assert!(output.status.success(), "{}", stderr(&output));
    }

    #[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
    #[test]
    fn two_starts_at_once_leave_one_playground() {
        let project = Arc::new(Project::new());
        let handles: Vec<_> = (0..2)
            .map(|_| {
                let project = Arc::clone(&project);
                std::thread::spawn(move || project.start(&[]))
            })
            .collect();
        let records: Vec<Value> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        assert_eq!(records[0]["pid"], records[1]["pid"]);
        assert_eq!(records[0]["url"], records[1]["url"]);
        assert_ne!(records[0]["login_url"], records[1]["login_url"]);
        // Both starts agreed on the tokens: every one on disk is the one the server
        // accepts.
        let record = &records[0];
        let (status, _, body) = request(
            "GET",
            &control(record, "/api/status"),
            Some(&project.token("admin")),
            &[],
            None,
        );
        assert_eq!(status, 200, "admin: {body}");
        for name in ["stand-in", "app", "chat", "browser-chat"] {
            let body = execute(
                record,
                &project.token(name),
                "function main(): number { return 1; }",
            );
            assert_eq!(body["result"], "1", "{name}: {body}");
        }
    }

    #[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
    #[test]
    fn root_layout_with_a_root_blueprint_is_found_from_a_nested_directory() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("app");
        std::fs::create_dir_all(root.join("src/deep")).unwrap();
        std::fs::write(
            root.join("submilli.toml"),
            "[package]\nname = \"@demo/app\"\n",
        )
        .unwrap();
        std::fs::write(root.join("blueprint.yaml"), BLUEPRINT).unwrap();
        let project = Project {
            root: root.clone(),
            dir,
        };
        let output = project
            .command(&["start", "--json"])
            .current_dir(root.join("src/deep"))
            .output()
            .unwrap();
        assert!(output.status.success(), "{}", stderr(&output));
        let record = parse(&output);
        assert_eq!(record["blueprint"]["name"], "demo");
        assert!(root.join(".submilli/playground/lock").exists());
    }

    #[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
    #[test]
    fn a_live_instance_that_does_not_answer_is_busy_and_never_replaced() {
        let project = Project::new();
        project.start(&[]);
        assert!(project.stop().status.success());

        let port = silent_listener();
        write_lock(&project, std::process::id(), port);
        let held = hold_instance_lock(&project);
        let before = read_lock_file(&project);

        let output = project.run(&["stop", "--json"]);
        assert_eq!(output.status.code(), Some(6), "{}", stderr(&output));
        assert!(
            stderr(&output).contains("did not answer"),
            "{}",
            stderr(&output)
        );
        assert_eq!(parse(&output)["busy"], true);
        assert_eq!(read_lock_file(&project), before, "stop removed a live lock");

        let output = project.run(&["start", "--json"]);
        assert_eq!(output.status.code(), Some(1), "{}", stderr(&output));
        assert!(
            stderr(&output).contains("did not answer"),
            "{}",
            stderr(&output)
        );
        assert!(stdout(&output).trim().is_empty(), "{}", stdout(&output));
        assert_eq!(
            read_lock_file(&project),
            before,
            "start replaced a live lock"
        );
        assert!(
            !project.ready_file().exists(),
            "start launched a second instance"
        );

        // Once nothing holds the instance lock, the same lock is stale.
        drop(held);
        let record = project.start(&[]);
        assert_ne!(record["attached"], true);
    }

    #[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
    #[test]
    fn a_start_killed_while_its_child_starts_leaves_one_playground() {
        let project = Project::new();
        let mut first = project
            .command(&["start", "--json"])
            .env("SUBMILLI_PLAYGROUND_TEST_START_DELAY_MS", "15000")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        wait_until(
            "the child to take the instance lock",
            Duration::from_secs(30),
            || instance_held(&project),
        );
        first.kill().unwrap();
        first.wait().unwrap();

        // The orphaned child is still starting: a start leaves it alone, and names it.
        let child = instance_holder(&project).expect("the orphaned child");
        let output = project.run(&["start", "--json"]);
        assert_eq!(output.status.code(), Some(1), "{}", stderr(&output));
        let err = stderr(&output);
        assert!(err.contains("starting"), "{err}");
        assert!(err.contains(&format!("pid {child}")), "{err}");

        // Once it is ready, a start attaches to it rather than starting another.
        wait_until(
            "the orphaned child to be ready",
            Duration::from_secs(60),
            || project.lock().exists() && project.run(&["status", "--json"]).status.success(),
        );
        let record = project.start(&[]);
        assert_eq!(record["attached"], true, "{record}");
        let lock: Value = serde_json::from_str(&read_lock_file(&project)).unwrap();
        assert_eq!(lock["pid"], record["pid"]);
    }

    /// A `start` whose child is still starting (it waits 30s after taking the
    /// instance lock), with stderr piped, once the child holds the lock.
    fn start_waiting(project: &Project, ignore_hup: bool) -> std::process::Child {
        use std::os::unix::process::CommandExt;
        let mut command = project.command(&["start", "--json"]);
        command
            .env("SUBMILLI_PLAYGROUND_TEST_START_DELAY_MS", "30000")
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        if ignore_hup {
            // SAFETY: only an async-signal-safe call between fork and exec.
            unsafe {
                command.pre_exec(|| {
                    libc::signal(libc::SIGHUP, libc::SIG_IGN);
                    Ok(())
                });
            }
        }
        let start = command.spawn().unwrap();
        wait_until(
            "the child to take the instance lock",
            Duration::from_secs(30),
            || instance_held(project),
        );
        start
    }

    fn wait_exit(
        child: &mut std::process::Child,
        what: &str,
    ) -> (std::process::ExitStatus, String) {
        let deadline = Instant::now() + Duration::from_secs(15);
        let exit = loop {
            if let Some(exit) = child.try_wait().unwrap() {
                break exit;
            }
            assert!(Instant::now() < deadline, "{what}");
            std::thread::sleep(Duration::from_millis(50));
        };
        let mut err = String::new();
        if let Some(mut pipe) = child.stderr.take() {
            pipe.read_to_string(&mut err).unwrap();
        }
        (exit, err)
    }

    #[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
    #[test]
    fn interrupting_a_start_stops_the_child_it_launched() {
        let project = Project::new();
        for (signal, code) in [("INT", 130), ("TERM", 143), ("HUP", 129)] {
            let mut start = start_waiting(&project, false);
            Command::new("kill")
                .args([&format!("-{signal}"), &start.id().to_string()])
                .status()
                .unwrap();
            let (exit, err) = wait_exit(&mut start, &format!("start did not end on SIG{signal}"));
            assert_eq!(exit.code(), Some(code), "SIG{signal}: {err}");
            assert!(err.contains("interrupted"), "{err}");
            wait_until("the child to end", Duration::from_secs(10), || {
                !instance_held(&project)
            });
            assert!(!project.lock().exists());
            assert!(!project.ready_file().exists());
        }
    }

    #[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
    #[test]
    fn a_start_under_nohup_ignores_sighup() {
        let project = Project::new();
        let mut start = start_waiting(&project, true);
        Command::new("kill")
            .args(["-HUP", &start.id().to_string()])
            .status()
            .unwrap();
        std::thread::sleep(Duration::from_millis(500));
        assert!(
            start.try_wait().unwrap().is_none(),
            "SIGHUP ended a nohup start"
        );
        assert!(instance_held(&project));
        Command::new("kill")
            .args(["-INT", &start.id().to_string()])
            .status()
            .unwrap();
        let (exit, err) = wait_exit(&mut start, "start did not end on SIGINT");
        assert_eq!(exit.code(), Some(130), "{err}");
    }

    #[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
    #[test]
    fn stop_ends_a_playground_that_is_still_starting() {
        let project = Project::new();
        let mut start = start_waiting(&project, false);
        let child = instance_holder(&project).expect("the starting child");
        let output = project.run(&["status", "--json"]);
        assert_eq!(output.status.code(), Some(6), "{}", stderr(&output));
        assert!(
            stderr(&output).contains(&format!("pid {child}")),
            "{}",
            stderr(&output)
        );

        let output = project.stop();
        assert!(output.status.success(), "{}", stderr(&output));
        let stopped = parse(&output);
        assert_eq!(stopped["stopped"], true, "{stopped}");
        assert_eq!(stopped["pid"], json!(child));
        assert!(!instance_held(&project));
        assert!(!pid_alive(child) || instance_holder(&project) != Some(child));
        // The start that launched it reports that it was stopped while starting.
        let (exit, err) = wait_exit(&mut start, "start did not end once its child stopped");
        assert_eq!(exit.code(), Some(1), "{err}");
        assert!(err.contains("stopped while it was starting"), "{err}");
        assert!(!project.lock().exists());
    }

    #[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
    #[test]
    fn a_stop_once_signals_are_handled_but_before_ready_never_announces() {
        let project = Project::new();
        let mut start = project
            .command(&["start", "--json"])
            .env("SUBMILLI_PLAYGROUND_TEST_ANNOUNCE_DELAY_MS", "30000")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let log = project.state().join("playground.log");
        wait_until(
            "the child to wait with its signal handlers in",
            Duration::from_secs(60),
            || {
                std::fs::read_to_string(&log)
                    .is_ok_and(|log| log.contains("waiting before announcing"))
            },
        );
        let child = instance_holder(&project).expect("the starting child");

        let output = project.stop();
        assert!(output.status.success(), "{}", stderr(&output));
        assert_eq!(parse(&output)["pid"], json!(child));
        let (exit, err) = wait_exit(&mut start, "start did not end once its child stopped");
        assert_eq!(exit.code(), Some(1), "{err}");
        assert!(err.contains("stopped while it was starting"), "{err}");
        assert!(!err.contains("does not answer"), "{err}");
        let mut out = String::new();
        start
            .stdout
            .take()
            .unwrap()
            .read_to_string(&mut out)
            .unwrap();
        assert!(!out.contains("#login="), "{out}");
        assert!(!instance_held(&project));
        assert!(!project.lock().exists());
        assert!(!project.ready_file().exists());
        let log = std::fs::read_to_string(&log).unwrap();
        assert!(log.contains("stopped while it was starting"), "{log}");
    }

    #[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
    #[test]
    fn a_foreground_playground_started_with_sigint_ignored_keeps_ignoring_it() {
        use std::os::unix::process::CommandExt;
        let project = Project::new();
        let mut command = project.command(&["start", "--foreground", "--json"]);
        command.stdout(Stdio::null()).stderr(Stdio::null());
        // SAFETY: only an async-signal-safe call between fork and exec.
        unsafe {
            command.pre_exec(|| {
                libc::signal(libc::SIGINT, libc::SIG_IGN);
                Ok(())
            });
        }
        let mut serving = command.spawn().unwrap();
        wait_until("the playground to serve", Duration::from_secs(60), || {
            project.lock().exists() && project.run(&["status", "--json"]).status.success()
        });
        Command::new("kill")
            .args(["-INT", &serving.id().to_string()])
            .status()
            .unwrap();
        // Nothing to wait on for a signal that must do nothing: a while is enough.
        std::thread::sleep(Duration::from_millis(750));
        assert!(serving.try_wait().unwrap().is_none(), "SIGINT ended it");
        let output = project.run(&["status", "--json"]);
        assert!(output.status.success(), "{}", stderr(&output));
        assert_eq!(parse(&output)["stopping"], false);
        assert!(project.stop().status.success());
        let (exit, _) = wait_exit(&mut serving, "the foreground playground did not stop");
        assert!(exit.success());
    }

    #[test]
    fn a_start_whose_wait_fails_ends_the_child_it_launched() {
        let project = Project::new();
        std::fs::create_dir_all(project.state()).unwrap();
        // A ready file that is not a file fails the wait on its first read.
        std::fs::create_dir(project.ready_file()).unwrap();
        let output = project
            .command(&["start", "--json"])
            .env("SUBMILLI_PLAYGROUND_TEST_START_DELAY_MS", "30000")
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1), "{}", stdout(&output));
        assert!(
            stderr(&output).contains("not a regular file"),
            "{}",
            stderr(&output)
        );
        // The child would hold the instance lock for 30s; it is gone well before.
        std::thread::sleep(Duration::from_secs(1));
        assert!(!instance_held(&project), "the child was left behind");
    }

    #[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
    #[test]
    fn the_serving_process_keeps_its_instance_lock_through_status_runs_and_saves() {
        let project = Project::new();
        let record = project.start(&[]);
        let pid = u32::try_from(record["pid"].as_u64().unwrap()).unwrap();
        assert_eq!(instance_holder(&project), Some(pid));

        let output = project.run(&["status", "--json"]);
        assert!(output.status.success(), "{}", stderr(&output));
        assert_eq!(instance_holder(&project), Some(pid));

        execute(
            &record,
            &project.token("app"),
            "function main(): number { return 1; }",
        );
        let run = project.state().join("store/runs/1.json");
        wait_until("the run to be stored", Duration::from_secs(20), || {
            run.exists()
        });
        assert_eq!(instance_holder(&project), Some(pid));

        let blueprint = project.root.join("submilli/blueprints/demo.yaml");
        std::fs::write(
            &blueprint,
            format!("{BLUEPRINT}    - capability: http.post\n      action: allow\n"),
        )
        .unwrap();
        wait_until("the save to apply", Duration::from_secs(30), || {
            let output = project.run(&["status", "--json"]);
            output.status.success() && parse(&output)["blueprint_status"]["version"] == 2
        });
        assert_eq!(instance_holder(&project), Some(pid));
        assert!(project.stop().status.success());
    }

    #[test]
    fn status_open_and_stop_in_a_project_that_never_started_create_nothing() {
        let project = Project::new();
        let before = tree(&project.root);
        for (command, code) in [("status", 6), ("open", 6), ("stop", 0)] {
            let output = project.run(&[command, "--json"]);
            assert_eq!(
                output.status.code(),
                Some(code),
                "{command}: {}",
                stderr(&output)
            );
            assert_eq!(parse(&output)["running"], false, "{command}");
            let text = project.run(&[command]);
            assert_eq!(
                text.status.code(),
                Some(code),
                "{command}: {}",
                stderr(&text)
            );
            assert!(
                stdout(&text).contains("not running"),
                "{command}: {}",
                stdout(&text)
            );
        }
        assert_eq!(tree(&project.root), before);

        // A state directory without an instance lock is no different.
        std::fs::create_dir_all(project.state()).unwrap();
        let before = tree(&project.root);
        for (command, code) in [("status", 6), ("open", 6), ("stop", 0)] {
            let output = project.run(&[command, "--json"]);
            assert_eq!(
                output.status.code(),
                Some(code),
                "{command}: {}",
                stderr(&output)
            );
        }
        assert_eq!(tree(&project.root), before);
    }

    #[test]
    fn a_corrupt_lock_with_a_held_instance_lock_is_starting_until_stopped() {
        let project = Project::new();
        let holder = spawn_holder(&project, false);
        std::fs::write(project.lock(), "{ not json").unwrap();
        let pid = format!("pid {}", holder.pid());

        let output = project.run(&["status", "--json"]);
        assert_eq!(output.status.code(), Some(6), "{}", stderr(&output));
        assert!(stderr(&output).contains("starting"), "{}", stderr(&output));
        assert!(stderr(&output).contains(&pid), "{}", stderr(&output));
        let output = project.run(&["start", "--json"]);
        assert_eq!(output.status.code(), Some(1), "{}", stderr(&output));
        assert!(stderr(&output).contains(&pid), "{}", stderr(&output));
        assert_eq!(read_lock_file(&project), "{ not json");

        let output = project.stop();
        assert!(output.status.success(), "{}", stderr(&output));
        assert_eq!(parse(&output)["pid"], json!(holder.pid()));
        assert!(!instance_held(&project));
    }

    #[test]
    fn a_stop_that_times_out_exits_1_naming_the_pid() {
        let project = Project::new();
        let holder = spawn_holder(&project, true);
        let output = run_within(&project, &["stop", "--json"], Duration::from_secs(60));
        assert_eq!(output.status.code(), Some(1), "{}", stdout(&output));
        let err = stderr(&output);
        assert!(err.contains(&format!("pid {}", holder.pid())), "{err}");
        assert!(err.contains("did not stop"), "{err}");
        assert!(instance_held(&project));
    }

    #[test]
    fn a_non_regular_instance_lock_is_a_clear_error() {
        let project = Project::new();
        std::fs::create_dir_all(project.state()).unwrap();
        let path = project.state().join("instance.lock");
        let fifo = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()).unwrap();
        // SAFETY: a valid C string.
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
        for args in [
            &["status", "--json"][..],
            &["stop", "--json"],
            &["start", "--json"],
        ] {
            let output = run_within(&project, args, Duration::from_secs(30));
            assert_eq!(
                output.status.code(),
                Some(1),
                "{args:?}: {}",
                stdout(&output)
            );
            let err = stderr(&output);
            assert!(
                err.contains("instance.lock is not a regular file"),
                "{args:?}: {err}"
            );
        }
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        let output = run_within(&project, &["status"], Duration::from_secs(30));
        assert_eq!(output.status.code(), Some(1));
        assert!(
            stderr(&output).contains("instance.lock is not a regular file"),
            "{}",
            stderr(&output)
        );
    }

    #[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
    #[test]
    fn status_and_start_say_stopping_while_the_playground_drains() {
        let (port, accepted) = silent_listener_reporting();
        let project = Project::new();
        let record = project.start(&["--allow-localhost"]);
        let pid = u32::try_from(record["pid"].as_u64().unwrap()).unwrap();
        // A run that never ends holds the drain open for its grace period.
        let program = format!(
            "import {{ get, Response }} from \"submilli:http\";\nfunction main(): string {{ const r: Response = get(\"http://127.0.0.1:{port}/\"); return r.body; }}"
        );
        let url = server(&record, "/v1/execute");
        let app = project.token("app");
        std::thread::spawn(move || {
            let _ = request(
                "POST",
                &url,
                Some(&app),
                &[],
                Some(json!({ "code": program, "blueprint": "demo" })),
            );
        });
        // The run is in flight once it has reached the listener.
        accepted
            .recv_timeout(Duration::from_secs(30))
            .expect("the run to reach the silent listener");
        let (status, _, body) = request(
            "POST",
            &control(&record, "/api/stop"),
            Some(&project.token("admin")),
            &[],
            None,
        );
        assert_eq!(status, 202, "{body}");

        let output = project.run(&["status", "--json"]);
        assert_eq!(output.status.code(), Some(6), "{}", stderr(&output));
        let status = parse(&output);
        assert_eq!(status["stopping"], true, "{status}");
        assert_eq!(status["running"], false, "{status}");
        let output = project.run(&["start", "--json"]);
        assert_eq!(output.status.code(), Some(1), "{}", stderr(&output));
        let err = stderr(&output);
        assert!(err.contains("stopping"), "{err}");
        assert!(err.contains(&format!("pid {pid}")), "{err}");
        // `stop` waits for the drain to end.
        let output = project.stop();
        assert!(output.status.success(), "{}", stderr(&output));
        assert!(!instance_held(&project));
    }

    #[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
    #[test]
    fn open_refuses_a_success_whose_body_is_not_a_status() {
        let project = Project::new();
        project.start(&[]);
        assert!(project.stop().status.success());

        let port = convincing_impostor(&[
            ("/api/login-codes", r#"{"code":"abc"}"#),
            ("/api/status", "this is not JSON"),
        ]);
        write_lock(&project, std::process::id(), port);
        let _held = hold_instance_lock(&project);
        for command in ["open", "status"] {
            let output = project.run(&[command, "--json"]);
            assert!(!output.status.success(), "{command}: {}", stdout(&output));
            assert!(!stdout(&output).contains("#login="), "{}", stdout(&output));
            assert!(stderr(&output).contains("not JSON"), "{}", stderr(&output));
        }

        // JSON that is no status (every field defaults) is refused too.
        let port = convincing_impostor(&[
            ("/api/login-codes", r#"{"code":"abc"}"#),
            ("/api/status", "{}"),
        ]);
        write_lock(&project, std::process::id(), port);
        for command in ["open", "status"] {
            let output = project.run(&[command, "--json"]);
            assert!(!output.status.success(), "{command}: {}", stdout(&output));
            assert!(!stdout(&output).contains("#login="), "{}", stdout(&output));
            assert!(
                stderr(&output).contains("not a playground status"),
                "{command}: {}",
                stderr(&output)
            );
        }
        std::fs::remove_file(project.lock()).unwrap();
    }

    #[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
    #[test]
    fn attaching_with_other_settings_says_the_running_playground_keeps_its_own() {
        let project = Project::new();
        project.start(&[]);
        let output = project.run(&["start", "--json"]);
        assert!(output.status.success(), "{}", stderr(&output));
        assert!(
            !stderr(&output).contains("keeps its own"),
            "{}",
            stderr(&output)
        );

        let output = project.run(&["start", "--json", "--allow-localhost"]);
        assert!(output.status.success(), "{}", stderr(&output));
        let err = stderr(&output);
        assert!(err.contains("keeps its own settings"), "{err}");
        assert!(err.contains("submilli playground stop"), "{err}");
        let record = parse(&output);
        assert_eq!(record["attached"], true);
        assert_eq!(record["egress_grants"], json!([]));
    }
}
