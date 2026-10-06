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
        }
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
        for command in ["status", "open"] {
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
}
