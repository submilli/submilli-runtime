//! The playground's project (U9) through the real binary: finding the project from
//! anywhere in it, `playground init` and its starter, the package closure in the
//! ready record and `status`, and the package check every run gets, whoever sends it:
//! an edited project package is rebuilt before the run, one that no longer builds
//! fails the run as a package-resolution error, and an install from outside the
//! playground is picked up.
//!
//! `exec` (the assistant's own runs) does not exist yet, so runs here arrive through
//! the server: REST `execute` with a token, and the MCP execute tool, which is the
//! bridge's path.

#[cfg(unix)]
mod unix {
    use std::path::{Path, PathBuf};
    use std::process::{Command, Output};
    use std::time::{Duration, Instant};

    use serde_json::{Value, json};

    const BIN: &str = env!("CARGO_BIN_EXE_submilli");

    const EXAMPLE_RESULT: &str = "2 charges, 6150 cents";

    /// A directory holding a project, with a home of its own so nothing reads or
    /// writes the developer's real package store. Dropping it stops any playground it
    /// started.
    struct Workspace {
        _dir: tempfile::TempDir,
        home: PathBuf,
        root: PathBuf,
    }

    impl Workspace {
        /// An existing repository with no Submilli project in it.
        fn repository() -> Self {
            let dir = tempfile::tempdir().expect("tempdir");
            let root = dir.path().join("app").canonicalize_or_create();
            std::fs::create_dir_all(root.join(".git")).unwrap();
            std::fs::create_dir_all(root.join("src/handlers")).unwrap();
            std::fs::write(root.join("src/handlers/agent.py"), "print('agent')\n").unwrap();
            Self {
                home: dir.path().join("home"),
                _dir: dir,
                root,
            }
        }

        /// A repository with the starter scaffolded into it.
        fn starter() -> Self {
            let workspace = Self::repository();
            let output = workspace.playground(&workspace.root, &["init", "--json"]);
            assert!(output.status.success(), "init: {}", stderr(&output));
            workspace
        }

        fn submilli(&self) -> PathBuf {
            self.root.join("submilli")
        }

        fn lib(&self) -> PathBuf {
            self.submilli().join("packages/billing/src/lib.ts")
        }

        fn blueprint(&self) -> PathBuf {
            self.submilli().join("blueprints/billing.yaml")
        }

        fn state(&self) -> PathBuf {
            self.root.join(".submilli/playground")
        }

        fn command(&self, cwd: &Path, args: &[&str]) -> Command {
            let mut command = Command::new(BIN);
            command
                .args(args)
                .current_dir(cwd)
                .env("SUBMILLI_HOME", &self.home)
                .env("HOME", &self.home)
                .env_remove("SUBMILLI_TELEMETRY")
                .env_remove("SUBMILLI_DENY_WARNINGS")
                .env_remove("SUBMILLI_ALLOW_LOCALHOST")
                .env_remove("SUBMILLI_ALLOW_PRIVATE")
                .env_remove("SUBMILLI_ALLOW_IP");
            for key in [
                "ANTHROPIC_API_KEY",
                "OPENAI_API_KEY",
                "GEMINI_API_KEY",
                "GOOGLE_API_KEY",
                "SUBMILLI_PLAYGROUND_MODEL",
            ] {
                command.env_remove(key);
            }
            command
        }

        fn submilli_cli(&self, cwd: &Path, args: &[&str]) -> Output {
            self.command(cwd, args).output().expect("run submilli")
        }

        fn playground(&self, cwd: &Path, args: &[&str]) -> Output {
            let mut all = vec!["playground"];
            all.extend_from_slice(args);
            self.submilli_cli(cwd, &all)
        }

        fn start_from(&self, cwd: &Path) -> Value {
            let output = self.playground(cwd, &["start", "--json"]);
            assert!(
                output.status.success(),
                "start failed: {}\nstdout: {}",
                stderr(&output),
                stdout(&output)
            );
            parse(&output)
        }

        fn start(&self) -> Value {
            self.start_from(&self.root)
        }

        fn status(&self) -> Value {
            let output = self.playground(&self.root, &["status", "--json"]);
            assert!(output.status.success(), "status: {}", stderr(&output));
            parse(&output)
        }

        fn token(&self, name: &str) -> String {
            std::fs::read_to_string(self.state().join("tokens").join(name))
                .unwrap()
                .trim()
                .to_owned()
        }

        fn log(&self) -> String {
            std::fs::read_to_string(self.state().join("playground.log")).unwrap_or_default()
        }

        fn reinstalls(&self) -> usize {
            self.log()
                .matches("reinstalled @acme/billing: its source changed")
                .count()
        }

        /// Every stored run.
        fn runs(&self) -> Vec<Value> {
            let Ok(entries) = std::fs::read_dir(self.state().join("store/runs")) else {
                return Vec::new();
            };
            let mut runs: Vec<Value> = entries
                .flatten()
                .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "json"))
                .map(|entry| serde_json::from_slice(&std::fs::read(entry.path()).unwrap()).unwrap())
                .collect();
            runs.sort_by_key(|run| run["id"].as_u64());
            runs
        }

        /// Stored runs appear once their owner task finishes; wait for `count`.
        fn wait_for_runs(&self, count: usize) -> Vec<Value> {
            let deadline = Instant::now() + Duration::from_secs(20);
            loop {
                let runs = self.runs();
                if runs.len() >= count {
                    return runs;
                }
                assert!(
                    Instant::now() < deadline,
                    "{count} runs are stored: {runs:?}"
                );
                std::thread::sleep(Duration::from_millis(50));
            }
        }

        /// Polls `status` until `done` holds for its `blueprint_status`.
        fn wait_for_blueprint(&self, what: &str, done: impl Fn(&Value) -> bool) -> Value {
            let deadline = Instant::now() + Duration::from_secs(30);
            loop {
                let status = self.status()["blueprint_status"].clone();
                if done(&status) {
                    return status;
                }
                assert!(
                    Instant::now() < deadline,
                    "timed out waiting for {what}: {status}\n{}",
                    self.log()
                );
                std::thread::sleep(Duration::from_millis(100));
            }
        }

        /// The versions the change log holds.
        fn versions(&self) -> Vec<u64> {
            std::fs::read_to_string(self.state().join("store/changes.jsonl"))
                .unwrap_or_default()
                .lines()
                .map(|line| serde_json::from_str::<Value>(line).unwrap())
                .filter(|line| line["kind"] == "version")
                .filter_map(|line| line["version"].as_u64())
                .collect()
        }

        /// A new project package `@acme/rates` under `packages/rates`, with `lib` as its
        /// source, listed in the project's `submilli.toml`.
        fn add_rates_package(&self, lib: &str) -> PathBuf {
            let rates = self.submilli().join("packages/rates");
            std::fs::create_dir_all(rates.join("src")).unwrap();
            std::fs::create_dir_all(rates.join("docs")).unwrap();
            std::fs::write(rates.join("docs/readme.md"), "# @acme/rates\n").unwrap();
            std::fs::write(rates.join("src/lib.ts"), lib).unwrap();
            let manifest = self.submilli().join("submilli.toml");
            let mut text = std::fs::read_to_string(&manifest).unwrap();
            text.push_str(
                "\n[[package]]\nname = \"@acme/rates\"\nversion = \"0.1.0\"\n\
                 description = \"Rates.\"\npath = \"packages/rates\"\n",
            );
            std::fs::write(&manifest, text).unwrap();
            rates
        }
    }

    impl Drop for Workspace {
        fn drop(&mut self) {
            if self.state().join("lock").exists() {
                let _ = self.playground(&self.root, &["stop", "--json"]);
            }
        }
    }

    trait CanonicalOrCreate {
        fn canonicalize_or_create(self) -> PathBuf;
    }

    impl CanonicalOrCreate for PathBuf {
        fn canonicalize_or_create(self) -> PathBuf {
            std::fs::create_dir_all(&self).unwrap();
            self.canonicalize().unwrap()
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

    fn edit(path: &Path, from: &str, to: &str) {
        let text = std::fs::read_to_string(path).unwrap();
        assert!(text.contains(from), "`{from}` is in {}", path.display());
        std::fs::write(path, text.replace(from, to)).unwrap();
    }

    fn agent() -> ureq::Agent {
        ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(60)))
            .build()
            .into()
    }

    fn post(
        url: &str,
        token: &str,
        headers: &[(&str, &str)],
        body: &Value,
    ) -> (u16, String, String) {
        let mut request = agent()
            .post(url)
            .header("authorization", &format!("Bearer {token}"))
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream");
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        let mut response = request.send(body.to_string()).unwrap();
        let session = response
            .headers()
            .get("mcp-session-id")
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_owned();
        let status = response.status().as_u16();
        (
            status,
            session,
            response.body_mut().read_to_string().unwrap(),
        )
    }

    /// The JSON body, or the last `data:` line of an event stream.
    fn json_body(text: &str) -> Value {
        let data = text
            .lines()
            .filter_map(|line| line.strip_prefix("data:"))
            .next_back()
            .map_or(text, str::trim);
        serde_json::from_str(data).unwrap_or(Value::Null)
    }

    /// The example program, as the starter wrote it.
    fn example(workspace: &Workspace) -> String {
        std::fs::read_to_string(workspace.submilli().join("examples/total.ts")).unwrap()
    }

    /// `POST /v1/execute` with `token`, as an app or the stand-in sends it.
    fn execute(record: &Value, token: &str, code: &str) -> Value {
        let url = format!("{}/v1/execute", record["server_url"].as_str().unwrap());
        let body = json!({
            "code": code,
            "blueprint": "billing",
            "variables": { "customerId": "cus_northwind" },
        });
        let (status, _, text) = post(&url, token, &[], &body);
        assert_eq!(status, 200, "execute: {text}");
        serde_json::from_str(&text).unwrap()
    }

    /// The MCP execute tool on a fresh MCP session: the bridge's path.
    fn mcp_execute(record: &Value, token: &str, code: &str) -> Value {
        let url = format!("{}/mcp/billing", record["server_url"].as_str().unwrap());
        let initialize = json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18", "capabilities": {},
                "clientInfo": { "name": "submilli-bridge", "version": "0" },
                "_meta": { "variables": { "customerId": "cus_northwind" } }
            }
        });
        let (status, session, text) = post(&url, token, &[], &initialize);
        assert_eq!(status, 200, "initialize: {text}");
        let initialized = json!({ "jsonrpc": "2.0", "method": "notifications/initialized" });
        post(&url, token, &[("mcp-session-id", &session)], &initialized);
        let call = json!({
            "jsonrpc": "2.0", "id": 2, "method": "tools/call",
            "params": { "name": "submilli__typescript__execute", "arguments": { "code": code } }
        });
        let (status, _, text) = post(&url, token, &[("mcp-session-id", &session)], &call);
        assert_eq!(status, 200, "tools/call: {text}");
        json_body(&text)
    }

    /// The tool's structured result.
    fn tool_result(response: &Value) -> Value {
        response["result"]["structuredContent"].clone()
    }

    #[test]
    fn init_creates_only_the_submilli_folder_and_the_starter_builds_tests_and_lints() {
        let workspace = Workspace::repository();
        let mut before: Vec<_> = std::fs::read_dir(&workspace.root)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        // From deep inside the repository, the project goes at its root.
        let output =
            workspace.playground(&workspace.root.join("src/handlers"), &["init", "--json"]);
        assert!(output.status.success(), "init: {}", stderr(&output));
        let record = parse(&output);
        assert_eq!(record["project"], json!(workspace.root));
        for file in record["files"].as_array().unwrap() {
            assert!(
                Path::new(file.as_str().unwrap()).starts_with(workspace.submilli()),
                "{file}"
            );
        }
        let mut after: Vec<_> = std::fs::read_dir(&workspace.root)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        before.push("submilli".into());
        before.sort();
        after.sort();
        assert_eq!(after, before, "only `submilli/` was added");
        assert_eq!(
            std::fs::read_to_string(workspace.root.join("src/handlers/agent.py")).unwrap(),
            "print('agent')\n"
        );

        // A second init refuses rather than touching the project.
        let again = workspace.playground(&workspace.root, &["init"]);
        assert_eq!(again.status.code(), Some(1));
        assert!(
            stderr(&again).contains("already holds"),
            "{}",
            stderr(&again)
        );

        let submilli = workspace.submilli();
        for args in [
            &["build", "test", "--deny-warnings"][..],
            &["build", "publish-local", "--deny-warnings"],
            &[
                "blueprint",
                "lint",
                "--deny-warnings",
                "blueprints/billing.yaml",
            ],
        ] {
            let output = workspace.submilli_cli(&submilli, args);
            assert!(
                output.status.success(),
                "{args:?}: {}\n{}",
                stderr(&output),
                stdout(&output)
            );
        }
        let mut after_build: Vec<_> = std::fs::read_dir(&workspace.root)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        after_build.sort();
        assert_eq!(
            after_build, before,
            "building wrote nothing outside `submilli/`"
        );
    }

    #[test]
    fn outside_a_project_without_a_terminal_start_exits_2_naming_init_and_creates_nothing() {
        let workspace = Workspace::repository();
        let output = workspace.playground(&workspace.root.join("src"), &["start", "--json"]);
        assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
        assert!(
            stderr(&output).contains("submilli playground init"),
            "{}",
            stderr(&output)
        );
        assert!(!workspace.submilli().exists());
        assert!(!workspace.root.join(".submilli").exists());
    }

    #[test]
    fn a_root_layout_project_is_found_from_a_nested_directory() {
        let workspace = Workspace::repository();
        std::fs::write(workspace.root.join("submilli.toml"), "").unwrap();
        std::fs::write(
            workspace.root.join("blueprint.yaml"),
            "name: demo\npermissions:\n  main: []\n",
        )
        .unwrap();
        let record = workspace.start_from(&workspace.root.join("src/handlers"));
        assert_eq!(record["project"], json!(workspace.root));
        assert_eq!(record["blueprint"]["name"], "demo");
        assert_eq!(record["packages"], json!([]));
    }

    #[test]
    fn from_init_to_a_successful_example_run_that_records_its_fixture_read() {
        let workspace = Workspace::starter();
        let record = workspace.start_from(&workspace.submilli().join("packages/billing/src"));
        assert_eq!(record["project"], json!(workspace.root));
        assert_eq!(record["blueprint"]["name"], "billing");
        let billing = json!([{
            "name": "@acme/billing", "version": "0.1.0",
            "origin": "blueprint", "importable": true, "project": true,
        }]);
        assert_eq!(record["packages"], billing);
        assert_eq!(workspace.status()["packages"], billing);

        let response = execute(&record, &workspace.token("app"), &example(&workspace));
        assert_eq!(response["result"], EXAMPLE_RESULT, "{response}");

        let runs = workspace.wait_for_runs(1);
        let run = &runs[0];
        assert_eq!(run["label"], "app");
        let calls = run["recording"]["calls"].as_array().unwrap();
        let read = calls
            .iter()
            .find(|call| call["capability"] == "fs.read")
            .unwrap_or_else(|| panic!("the fixture read is recorded: {calls:#?}"));
        assert_eq!(read["caller"], "@acme/billing");
        // What the data summary's "Fetched" counts: the full size of what came back.
        let fetched = read["response"]["bytes"]
            .as_u64()
            .unwrap_or_else(|| panic!("the read's size is recorded: {read:#}"));
        let fixture = std::fs::metadata(workspace.submilli().join("volumes/billing/charges.json"))
            .unwrap()
            .len();
        assert_eq!(fetched, fixture, "{read:#}");
    }

    #[test]
    fn the_closure_marks_a_dependency_only_package_not_importable_in_the_record_and_status() {
        let workspace = Workspace::starter();
        let money = workspace.submilli().join("packages/money");
        std::fs::create_dir_all(money.join("src")).unwrap();
        std::fs::create_dir_all(money.join("docs")).unwrap();
        std::fs::write(
            money.join("src/lib.ts"),
            "/** Cents as dollars. */\nexport function dollars(cents: number): number { return cents / 100; }\n",
        )
        .unwrap();
        std::fs::write(money.join("docs/readme.md"), "# @acme/money\n").unwrap();
        let manifest = workspace.submilli().join("submilli.toml");
        let mut text = std::fs::read_to_string(&manifest).unwrap();
        text.push_str(
            "dependencies = [\"@acme/money\"]\n\n[[package]]\nname = \"@acme/money\"\n\
             version = \"0.1.0\"\ndescription = \"Money helpers.\"\npath = \"packages/money\"\n",
        );
        std::fs::write(&manifest, text).unwrap();

        let record = workspace.start();
        let expected = json!([
            { "name": "@acme/billing", "version": "0.1.0", "origin": "blueprint",
              "importable": true, "project": true },
            { "name": "@acme/money", "version": "0.1.0", "origin": "dependency",
              "importable": false, "project": true },
        ]);
        assert_eq!(record["packages"], expected);
        assert_eq!(workspace.status()["packages"], expected);
        let text = workspace.playground(&workspace.root, &["status"]);
        let text = stdout(&text);
        assert!(
            text.contains("@acme/money 0.1.0 (dependency, not importable, project)"),
            "{text}"
        );
        assert!(
            text.contains("@acme/billing 0.1.0 (blueprint, importable, project)"),
            "{text}"
        );
    }

    #[test]
    fn an_edited_package_is_rebuilt_before_the_next_run_from_rest_and_from_the_bridge() {
        let workspace = Workspace::starter();
        let record = workspace.start();
        let code = example(&workspace);
        assert_eq!(
            execute(&record, &workspace.token("stand-in"), &code)["result"],
            EXAMPLE_RESULT
        );
        assert_eq!(workspace.reinstalls(), 0);

        // A run through REST, as `exec` will send it, sees the edit.
        edit(
            &workspace.lib(),
            "return found;",
            "return found.slice(0, 1);",
        );
        let edited = execute(&record, &workspace.token("stand-in"), &code);
        assert_eq!(edited["result"], "1 charges, 4900 cents", "{edited}");
        assert_eq!(workspace.reinstalls(), 1);

        // A run through the MCP execute tool, with no REST call in between, sees the next.
        edit(
            &workspace.lib(),
            "return found.slice(0, 1);",
            "return found.slice(1);",
        );
        let bridged = mcp_execute(&record, &workspace.token("stand-in"), &code);
        assert_eq!(
            tool_result(&bridged)["result"],
            "1 charges, 1250 cents",
            "{bridged}"
        );
        assert_eq!(workspace.reinstalls(), 2);
    }

    #[test]
    fn a_package_that_no_longer_builds_fails_the_bridge_run_as_a_recorded_resolution_error() {
        let workspace = Workspace::starter();
        let record = workspace.start();
        let code = example(&workspace);
        let first = mcp_execute(&record, &workspace.token("stand-in"), &code);
        assert_eq!(tool_result(&first)["result"], EXAMPLE_RESULT, "{first}");

        edit(&workspace.lib(), "return found;", "return found +;");
        let failed = mcp_execute(&record, &workspace.token("stand-in"), &code);
        let message = failed["error"]["message"]
            .as_str()
            .unwrap_or_else(|| panic!("a JSON-RPC error: {failed}"));
        assert!(message.contains("`@acme/billing`"), "{message}");
        assert!(message.contains("no longer builds"), "{message}");
        assert!(
            message.contains("lib.ts"),
            "the build diagnostic: {message}"
        );
        assert!(!message.contains("denied"), "{message}");
        assert!(
            tool_result(&failed).is_null(),
            "the old copy did not run: {failed}"
        );

        // REST gets the same kind, and nothing ran.
        let rest = execute(&record, &workspace.token("app"), &code);
        assert_eq!(rest["error"]["kind"], "package_resolution", "{rest}");
        assert!(rest["result"].is_null(), "{rest}");

        let runs = workspace.wait_for_runs(3);
        let failed_runs: Vec<_> = runs
            .iter()
            .filter(|run| run["error"]["kind"] == "package_resolution")
            .collect();
        assert_eq!(failed_runs.len(), 2, "{runs:#?}");
        let bridge = failed_runs
            .iter()
            .find(|run| run["label"] == "stand-in")
            .expect("the bridge's failed run is stored");
        assert_eq!(bridge["entry"], "mcp");
        assert_eq!(bridge["dispatched"], false);
        assert!(
            bridge["error"]["message"]
                .as_str()
                .unwrap()
                .contains("`@acme/billing`")
        );
        assert!(
            bridge["recording"]["calls"]
                .as_array()
                .is_none_or(Vec::is_empty),
            "the stale package never ran: {bridge:#}"
        );
    }

    #[test]
    fn two_concurrent_runs_after_one_edit_rebuild_it_once() {
        let workspace = Workspace::starter();
        let record = workspace.start();
        let code = example(&workspace);
        let token = workspace.token("app");
        assert_eq!(execute(&record, &token, &code)["result"], EXAMPLE_RESULT);

        edit(
            &workspace.lib(),
            "return found;",
            "return found.slice(0, 1);",
        );
        let runs: Vec<_> = (0..2)
            .map(|_| {
                let (record, token, code) = (record.clone(), token.clone(), code.clone());
                std::thread::spawn(move || execute(&record, &token, &code))
            })
            .collect();
        for run in runs {
            let response = run.join().unwrap();
            assert_eq!(response["result"], "1 charges, 4900 cents", "{response}");
        }
        assert_eq!(workspace.reinstalls(), 1, "{}", workspace.log());
    }

    #[test]
    fn a_publish_local_outside_the_playground_is_picked_up_by_the_next_run() {
        let workspace = Workspace::starter();
        // A package from another project, installed the usual way.
        let other = workspace.root.parent().unwrap().join("rates");
        std::fs::create_dir_all(other.join("src")).unwrap();
        std::fs::create_dir_all(other.join("docs")).unwrap();
        std::fs::write(
            other.join("submilli.toml"),
            "[[package]]\nname = \"@acme/rates\"\nversion = \"0.1.0\"\ndescription = \"Rates.\"\npath = \".\"\n",
        )
        .unwrap();
        std::fs::write(other.join("docs/readme.md"), "# @acme/rates\n").unwrap();
        let lib = other.join("src/lib.ts");
        std::fs::write(
            &lib,
            "/** The rate. */\nexport function rate(): number { return 1; }\n",
        )
        .unwrap();
        let publish = || {
            let output = workspace.submilli_cli(&other, &["build", "publish-local"]);
            assert!(output.status.success(), "publish: {}", stderr(&output));
        };
        publish();
        edit(
            &workspace.blueprint(),
            "- '@acme/billing'",
            "- '@acme/billing'\n- '@acme/rates'",
        );

        let record = workspace.start();
        let code = "import { rate } from \"@acme/rates\";\nfunction main(): string { return `rate ${rate()}`; }\n";
        let token = workspace.token("app");
        assert_eq!(execute(&record, &token, code)["result"], "rate 1");

        edit(&lib, "return 1;", "return 2;");
        publish();
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            let response = execute(&record, &token, code);
            if response["result"] == "rate 2" {
                break;
            }
            assert_eq!(response["result"], "rate 1", "{response}");
            assert!(
                Instant::now() < deadline,
                "the new install was not picked up"
            );
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    const RATES_PROGRAM: &str = "import { rate } from \"@acme/rates\";\nfunction main(): string { return `rate ${rate()}`; }\n";

    #[test]
    fn a_blueprint_edit_naming_a_new_project_package_builds_it_and_applies() {
        let workspace = Workspace::starter();
        let record = workspace.start();
        workspace.wait_for_blueprint("version 1", |status| status["version"] == 1);

        workspace
            .add_rates_package("/** The rate. */\nexport function rate(): number { return 1; }\n");
        edit(
            &workspace.blueprint(),
            "- '@acme/billing'",
            "- '@acme/billing'\n- '@acme/rates'",
        );
        let status = workspace.wait_for_blueprint("version 2", |status| {
            status["version"] == 2 || status["refused"].is_object()
        });
        assert_eq!(status["version"], 2, "{status}\n{}", workspace.log());
        assert!(status["refused"].is_null(), "{status}");
        assert_eq!(workspace.versions(), [1, 2]);

        let response = execute(&record, &workspace.token("app"), RATES_PROGRAM);
        assert_eq!(response["result"], "rate 1", "{response}");

        // A package that is neither the project's nor installed is still missing.
        edit(
            &workspace.blueprint(),
            "- '@acme/rates'",
            "- '@acme/rates'\n- '@acme/absent'",
        );
        let status =
            workspace.wait_for_blueprint("the refusal", |status| status["refused"].is_object());
        assert_eq!(status["refused"]["code"], "package_missing", "{status}");
        let message = status["refused"]["message"].as_str().unwrap();
        assert!(message.contains("`@acme/absent`"), "{message}");
        assert!(message.contains("install it with"), "{message}");
        assert_eq!(status["version"], 2, "{status}");
        assert_eq!(workspace.versions(), [1, 2]);
    }

    #[test]
    fn a_blueprint_edit_naming_a_new_package_that_does_not_build_is_refused() {
        let workspace = Workspace::starter();
        let record = workspace.start();
        workspace.wait_for_blueprint("version 1", |status| status["version"] == 1);

        workspace.add_rates_package(
            "/** The rate. */\nexport function rate(): number { return 1 +; }\n",
        );
        edit(
            &workspace.blueprint(),
            "- '@acme/billing'",
            "- '@acme/billing'\n- '@acme/rates'",
        );
        let status =
            workspace.wait_for_blueprint("the refusal", |status| status["refused"].is_object());
        let message = status["refused"]["message"].as_str().unwrap();
        assert!(message.contains("`@acme/rates`"), "{message}");
        assert!(
            message.contains("lib.ts"),
            "the build diagnostic: {message}"
        );
        assert_ne!(status["refused"]["code"], "package_missing", "{status}");
        assert_eq!(status["version"], 1, "{status}");
        assert_eq!(workspace.versions(), [1], "no version was logged");
        assert!(
            workspace.log().contains("`@acme/rates`"),
            "{}",
            workspace.log()
        );

        // The last good version is still in force.
        let response = execute(&record, &workspace.token("app"), &example(&workspace));
        assert_eq!(response["result"], EXAMPLE_RESULT, "{response}");
    }

    #[test]
    fn a_blueprint_naming_a_missing_package_exits_5_with_the_install_command() {
        let workspace = Workspace::starter();
        edit(
            &workspace.blueprint(),
            "- '@acme/billing'",
            "- '@acme/billing'\n- '@acme/absent'",
        );
        let output = workspace.playground(&workspace.root, &["start", "--json"]);
        assert_eq!(output.status.code(), Some(5), "{}", stderr(&output));
        let message = stderr(&output);
        assert!(message.contains("`@acme/absent`"), "{message}");
        assert!(message.contains("submilli install"), "{message}");
        assert!(!message.to_lowercase().contains("denied"), "{message}");
        assert!(!workspace.state().join("lock").exists());
    }
}
