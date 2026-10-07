//! The run store through the real binary: a run the developer's app sends to the
//! playground's server is stored in the project, labeled by its token, with its
//! bindings, owner-only, with the secrets it carried cut out, and run ids keep counting
//! across a restart.

#[cfg(unix)]
mod unix {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Output};
    use std::time::Duration;

    use base64::Engine as _;
    use serde_json::{Value, json};

    const BIN: &str = env!("CARGO_BIN_EXE_submilli");

    const SECRET: &str = "sk_live_playground_9Zq1Xw7Vb";

    struct Project {
        dir: tempfile::TempDir,
        root: PathBuf,
    }

    impl Project {
        fn new(yaml: &str) -> Self {
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

        fn store(&self) -> PathBuf {
            self.state().join("store")
        }

        fn token(&self, name: &str) -> String {
            std::fs::read_to_string(self.state().join("tokens").join(name))
                .unwrap()
                .trim()
                .to_owned()
        }

        fn run(&self, args: &[&str]) -> Output {
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
            for key in [
                "ANTHROPIC_API_KEY",
                "OPENAI_API_KEY",
                "GEMINI_API_KEY",
                "GOOGLE_API_KEY",
            ] {
                command.env_remove(key);
            }
            command.output().expect("run submilli")
        }

        fn start(&self) -> Value {
            let output = self.run(&["start", "--json", "--allow-localhost"]);
            assert!(
                output.status.success(),
                "start failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            serde_json::from_slice(&output.stdout).unwrap()
        }

        fn stop(&self) {
            let output = self.run(&["stop", "--json"]);
            assert!(
                output.status.success(),
                "stop failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        fn run_file(&self, id: u64) -> Value {
            let bytes = std::fs::read(self.store().join("runs").join(format!("{id}.json")))
                .unwrap_or_else(|error| panic!("run {id} is stored: {error}"));
            serde_json::from_slice(&bytes).unwrap()
        }
    }

    impl Drop for Project {
        fn drop(&mut self) {
            if self.state().join("lock").exists() {
                let _ = self.run(&["stop", "--json"]);
            }
        }
    }

    /// A loopback origin that echoes the bearer token it was sent, as is, base64, and
    /// URL-encoded.
    fn echo_origin() -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                let mut head = Vec::new();
                let mut byte = [0_u8; 1];
                while !head.ends_with(b"\r\n\r\n") && stream.read(&mut byte).unwrap_or(0) == 1 {
                    head.push(byte[0]);
                }
                let head = String::from_utf8_lossy(&head).into_owned();
                let token = head
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("authorization")
                            .then(|| value.trim().trim_start_matches("Bearer ").to_owned())
                    })
                    .unwrap_or_default();
                let body = format!(
                    "plain={token} b64={} url={}",
                    base64::engine::general_purpose::STANDARD.encode(&token),
                    url::form_urlencoded::byte_serialize(token.as_bytes()).collect::<String>()
                );
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            }
        });
        port
    }

    fn blueprint() -> String {
        "name: demo\n\
         allow_insecure_http: true\n\
         variables:\n  customerId:\n    required: true\n\
         secrets:\n  API_KEY:\n    harness: {}\n\
         auth_proxy:\n  - host: 127.0.0.1\n    allow_insecure_http: true\n    headers:\n      Authorization: \"Bearer ${secrets.API_KEY}\"\n\
         permissions:\n  main:\n    - capability: http.get\n      action: allow\n"
            .to_owned()
    }

    fn execute(record: &Value, token: &str, body: Value) -> Value {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(60)))
            .build()
            .into();
        let url = format!("{}/v1/execute", record["server_url"].as_str().unwrap());
        let mut response = agent
            .post(&url)
            .header("authorization", &format!("Bearer {token}"))
            .header("content-type", "application/json")
            .send(body.to_string())
            .unwrap();
        let text = response.body_mut().read_to_string().unwrap();
        assert_eq!(response.status().as_u16(), 200, "execute: {text}");
        serde_json::from_str(&text).unwrap()
    }

    fn mode(path: &Path) -> u32 {
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    fn walk(dir: &Path, visit: &mut dyn FnMut(&Path)) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                assert_eq!(mode(&path), 0o700, "{}", path.display());
                walk(&path, visit);
            } else {
                visit(&path);
            }
        }
    }

    #[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
    #[test]
    fn an_app_run_is_stored_labeled_with_its_binding_owner_only_and_without_its_secret() {
        let port = echo_origin();
        let project = Project::new(&blueprint());
        let record = project.start();
        let code = format!(
            r#"import {{ get }} from "submilli:http";
function main(): string {{
  const body = get("http://127.0.0.1:{port}/echo").body;
  console.log(body);
  return body;
}}"#
        );
        let response = execute(
            &record,
            &project.token("app"),
            json!({
                "code": code,
                "blueprint": "demo",
                "variables": { "customerId": "cus_northwind" },
                "secrets": { "API_KEY": SECRET },
            }),
        );
        assert!(response["error"].is_null(), "{response}");
        // The app itself got the echo; the store must not keep it.
        assert!(
            response["result"].as_str().unwrap().contains(SECRET),
            "{response}"
        );
        project.stop();

        // Covers AE7: labeled as the developer's own agent, showing the binding.
        let run = project.run_file(1);
        assert_eq!(run["label"], "app");
        assert_eq!(run["entry"], "http");
        assert_eq!(run["recording"]["variables"]["customerId"], "cus_northwind");
        assert_eq!(run["recording"]["execution_id"], response["execution_id"]);
        assert!(
            run["console"]
                .as_str()
                .unwrap()
                .contains("plain=[redacted] b64=[redacted] url=[redacted]"),
            "{}",
            run["console"]
        );

        let base64 = base64::engine::general_purpose::STANDARD.encode(SECRET);
        let url: String = url::form_urlencoded::byte_serialize(SECRET.as_bytes()).collect();
        let mut files = 0;
        assert_eq!(mode(&project.store()), 0o700);
        walk(&project.store(), &mut |path| {
            assert_eq!(mode(path), 0o600, "{}", path.display());
            let text = String::from_utf8_lossy(&std::fs::read(path).unwrap()).into_owned();
            for form in [SECRET, base64.as_str(), url.as_str()] {
                assert!(!text.contains(form), "{} holds {form}", path.display());
            }
            files += 1;
        });
        assert!(files >= 4, "store, sequence, index, run");

        // A restart keeps the runs and continues their ids.
        let record = project.start();
        let response = execute(
            &record,
            &project.token("stand-in"),
            json!({
                "code": "function main(): string { return \"second\"; }",
                "blueprint": "demo",
                "variables": { "customerId": "cus_initech" },
            }),
        );
        assert!(response["error"].is_null(), "{response}");
        project.stop();
        assert_eq!(project.run_file(1)["label"], "app");
        let second = project.run_file(2);
        assert_eq!(second["label"], "stand-in");
        assert_eq!(
            second["recording"]["variables"]["customerId"],
            "cus_initech"
        );
    }
}
