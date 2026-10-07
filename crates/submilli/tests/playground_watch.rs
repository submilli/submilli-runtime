//! The blueprint watcher through the real binary: saving the blueprint, however the
//! editor saves it, applies it without a restart as a classified version, and every
//! run records the version in force; a save that does not parse is reported against
//! its line while runs continue under the last good version.

#[cfg(unix)]
mod unix {
    use std::path::PathBuf;
    use std::process::{Command, Output};
    use std::time::{Duration, Instant};

    use serde_json::{Value, json};

    const BIN: &str = env!("CARGO_BIN_EXE_submilli");

    const PINNED: &str = "\
name: demo
variables:
  customerId:
    required: true
permissions:
  main:
    - name: charges-for-signed-in-customer
      capability: test.com/charges
      filter: customerId == ${vars.customerId}
      action: allow
";

    const CHECK: &str = r#"import { check } from "submilli:security";
function main(): string {
  try { check("test.com/charges", { customerId: "cus_initech" }); return "allowed"; }
  catch (e: PermissionDeniedError) { return "denied"; }
}"#;

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

        fn blueprint(&self) -> PathBuf {
            self.root.join("submilli/blueprints/demo.yaml")
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
            let output = self.run(&["start", "--json"]);
            assert!(
                output.status.success(),
                "start failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            serde_json::from_slice(&output.stdout).unwrap()
        }

        fn status(&self) -> Value {
            let output = self.run(&["status", "--json"]);
            assert!(
                output.status.success(),
                "status failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            serde_json::from_slice(&output.stdout).unwrap()
        }

        /// Every line of the change log.
        fn changes(&self) -> Vec<Value> {
            std::fs::read_to_string(self.state().join("store/changes.jsonl"))
                .unwrap_or_default()
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect()
        }

        fn versions(&self) -> Vec<Value> {
            self.changes()
                .into_iter()
                .filter(|line| line["kind"] == "version")
                .collect()
        }

        /// Polls `status` until `done` holds for its `blueprint_status`.
        fn wait_for(&self, what: &str, done: impl Fn(&Value) -> bool) -> Value {
            let deadline = Instant::now() + Duration::from_secs(30);
            loop {
                let status = self.status()["blueprint_status"].clone();
                if done(&status) {
                    return status;
                }
                assert!(
                    Instant::now() < deadline,
                    "timed out waiting for {what}: {status}"
                );
                std::thread::sleep(Duration::from_millis(100));
            }
        }

        fn wait_for_version(&self, version: u64) {
            self.wait_for(&format!("version {version}"), |status| {
                status["version"] == version
            });
        }

        /// Saves as editors that write a new file and rename it over the old one do.
        fn save_by_rename(&self, yaml: &str) {
            let temp = self.root.join("submilli/blueprints/.demo.yaml.swp");
            std::fs::write(&temp, yaml).unwrap();
            std::fs::rename(&temp, self.blueprint()).unwrap();
        }

        fn run_file(&self, id: u64) -> Value {
            let path = self.state().join("store/runs").join(format!("{id}.json"));
            let bytes =
                std::fs::read(&path).unwrap_or_else(|error| panic!("run {id} is stored: {error}"));
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

    fn execute(record: &Value, token: &str, customer: &str) -> Value {
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
            .send(
                json!({
                    "code": CHECK,
                    "blueprint": "demo",
                    "variables": { "customerId": customer },
                })
                .to_string(),
            )
            .unwrap();
        let text = response.body_mut().read_to_string().unwrap();
        assert_eq!(response.status().as_u16(), 200, "execute: {text}");
        serde_json::from_str(&text).unwrap()
    }

    /// Waits for run `id` to be stored: the recorder writes it as the run ends.
    fn stored_run(project: &Project, id: u64) -> Value {
        let path = project
            .state()
            .join("store/runs")
            .join(format!("{id}.json"));
        let deadline = Instant::now() + Duration::from_secs(20);
        while !path.exists() {
            assert!(Instant::now() < deadline, "run {id} was not stored");
            std::thread::sleep(Duration::from_millis(50));
        }
        project.run_file(id)
    }

    #[test]
    fn saved_edits_apply_as_classified_versions_that_runs_record() {
        let project = Project::new(PINNED);
        let record = project.start();
        project.wait_for_version(1);
        let app = project.token("app");

        // Under version 1 the pin holds: another customer's charges are denied.
        let denied = execute(&record, &app, "cus_northwind");
        assert_eq!(denied["result"], "denied", "{denied}");
        assert_eq!(
            stored_run(&project, 1)["recording"]["blueprint_version"],
            "1"
        );

        // Covers AE5 and F4: the pin goes, by a rename-save, with no restart and no
        // approval; the version is a widening flagged as a pin removal.
        project.save_by_rename(
            &PINNED.replace("      filter: customerId == ${vars.customerId}\n", ""),
        );
        project.wait_for_version(2);
        let versions = project.versions();
        let version = versions.last().unwrap();
        assert_eq!(version["version"], 2);
        assert_eq!(
            version["classification"]["classification"], "widening",
            "{version}"
        );
        assert_eq!(
            version["classification"]["changes"][0]["pin"]["kind"], "removed",
            "{version}"
        );
        let allowed = execute(&record, &app, "cus_northwind");
        assert_eq!(allowed["result"], "allowed", "{allowed}");
        assert_eq!(
            stored_run(&project, 2)["recording"]["blueprint_version"],
            "2"
        );

        // A comment-only save is no new version.
        let v2 = std::fs::read_to_string(project.blueprint()).unwrap();
        std::fs::write(project.blueprint(), format!("# charges\n{v2}")).unwrap();
        let deadline = Instant::now() + Duration::from_secs(30);
        while !project
            .changes()
            .iter()
            .any(|line| line["kind"] == "bytes-updated" && line["version"] == 2)
        {
            assert!(
                Instant::now() < deadline,
                "the comment-only save was not logged"
            );
            std::thread::sleep(Duration::from_millis(100));
        }
        assert_eq!(project.versions().len(), 2);

        // Covers AE8: a save that does not parse is reported against its line, and runs
        // continue under the last good version.
        std::fs::write(
            project.blueprint(),
            format!("{PINNED}    - capability: [unclosed\n"),
        )
        .unwrap();
        let status = project.wait_for("the refusal", |status| status["refused"].is_object());
        assert_eq!(status["refused"]["code"], "parse_error", "{status}");
        assert!(status["refused"]["line"].as_u64().is_some(), "{status}");
        assert_eq!(status["version"], 2);
        let still = execute(&record, &app, "cus_northwind");
        assert_eq!(still["result"], "allowed", "{still}");
        assert_eq!(
            stored_run(&project, 3)["recording"]["blueprint_version"],
            "2"
        );
        let text = project.run(&["status"]);
        let text = String::from_utf8_lossy(&text.stdout);
        assert!(text.contains("refused:    line "), "{text}");

        // Changing `name:` is refused too.
        std::fs::write(
            project.blueprint(),
            PINNED.replace("name: demo", "name: renamed"),
        )
        .unwrap();
        let status = project.wait_for("the name refusal", |status| {
            status["refused"]["code"] == "name_changed"
        });
        assert_eq!(status["version"], 2);

        // Returning to the first text is a new version, not version 1 again.
        std::fs::write(project.blueprint(), PINNED).unwrap();
        project.wait_for_version(3);
        let status = project.status()["blueprint_status"].clone();
        assert!(status["refused"].is_null(), "{status}");
        let versions = project.versions();
        assert_eq!(versions.len(), 3);
        assert_eq!(versions[0]["hash"], versions[2]["hash"]);
        let denied = execute(&record, &app, "cus_northwind");
        assert_eq!(denied["result"], "denied", "{denied}");
        assert_eq!(
            stored_run(&project, 4)["recording"]["blueprint_version"],
            "3"
        );

        let stop = project.run(&["stop", "--json"]);
        assert!(stop.status.success());

        // A restart with the file unchanged keeps version 3 in force.
        let record = project.start();
        project.wait_for_version(3);
        assert_eq!(project.versions().len(), 3);
        execute(&record, &app, "cus_northwind");
        assert_eq!(
            stored_run(&project, 5)["recording"]["blueprint_version"],
            "3"
        );
    }
}
