//! Process-boundary tests: no provider credentials or model calls required.
#![cfg(unix)]

use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::Path;
use std::process::{Command, Output};

use serde_json::{Value, json};

struct Project {
    temp: tempfile::TempDir,
}

impl Project {
    fn new() -> Self {
        let project = Self {
            temp: tempfile::tempdir().unwrap(),
        };
        project.write(
            "src/lib.ts",
            "export function answer(): number { return 42; }\n",
        );
        project.write(
            "submilli.toml",
            "[[package]]\nname = \"@acme/test\"\nversion = \"0.1.0\"\ndescription = \"Example.\"\n",
        );
        project
    }

    fn write(&self, name: &str, text: &str) {
        let path = self.temp.path().join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn mock(&self, agent: &str, response: &Value, behavior: &str) {
        let response = if agent == "claude" {
            json!({"is_error":false,"subtype":"success","structured_output":response})
        } else {
            response.clone()
        };
        let output = if agent == "copilot" {
            copilot_events(&response)
        } else {
            serde_json::to_string(&response).unwrap()
        };
        self.write("response.txt", &output);
        let capture_profile = if agent == "copilot" {
            "cp \"$COPILOT_HOME/agents/submilli-security-review.agent.md\" \"$REVIEW_PROFILE\" || exit 92"
        } else {
            ""
        };
        self.write(
            &format!("bin/{agent}"),
            &format!(
                r#"#!/bin/sh
if [ -n "$NODE_OPTIONS$BASH_ENV$ENV" ]; then exit 91; fi
if [ "$1" = --version ]; then echo "test-agent 1.0.0"; exit 0; fi
{capture_profile}
printf '%s\n' "$@" > "$REVIEW_ARGS"
cat > "$REVIEW_INPUT"
{behavior}
while [ "$#" -gt 0 ]; do
  if [ "$1" = --output-last-message ]; then
    shift
    cp "$REVIEW_RESPONSE" "$1"
    exit 0
  fi
  shift
done
cat "$REVIEW_RESPONSE"
"#
            ),
        );
        fs::set_permissions(
            self.temp.path().join(format!("bin/{agent}")),
            fs::Permissions::from_mode(0o755),
        )
        .unwrap();
    }

    fn run(&self, agent: &str, extra: &[&str]) -> Output {
        let path = std::env::join_paths(
            std::iter::once(self.temp.path().join("bin"))
                .chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
        )
        .unwrap();
        Command::new(env!("CARGO_BIN_EXE_submilli"))
            .args([
                "build",
                "security-review",
                "-a",
                agent,
                "-m",
                "test-model",
                "--output",
                "report.json",
            ])
            .args(extra)
            .current_dir(self.temp.path())
            .env("PATH", path)
            .env("SUBMILLI_HOME", self.temp.path().join("home"))
            .env("REVIEW_ARGS", self.temp.path().join("args.txt"))
            .env("REVIEW_INPUT", self.temp.path().join("input.txt"))
            .env("REVIEW_RESPONSE", self.temp.path().join("response.txt"))
            .env("REVIEW_PROFILE", self.temp.path().join("profile.md"))
            .env("NODE_OPTIONS", "must-be-removed")
            .env("BASH_ENV", "/nonexistent/security-review-test")
            .env("ENV", "/nonexistent/security-review-test")
            .output()
            .unwrap()
    }

    fn report(&self) -> Value {
        serde_json::from_slice(&fs::read(self.temp.path().join("report.json")).unwrap()).unwrap()
    }
}

fn clean() -> Value {
    json!({"complete":true,"reviewed_files":["src/lib.ts","submilli.toml"],"coverage_gaps":[],"findings":[]})
}

fn copilot_events(response: &Value) -> String {
    [
        json!({"type":"assistant.turn_start","data":{"turnId":"0"}}),
        json!({"type":"assistant.message","data":{"phase":"commentary","content":"Reviewing snapshot."}}),
        json!({"type":"assistant.message","data":{"phase":"final_answer","content":serde_json::to_string_pretty(response).unwrap()}}),
        json!({"type":"assistant.turn_end","data":{"turnId":"0"}}),
        json!({"type":"result","exitCode":0}),
    ]
    .iter()
    .map(Value::to_string)
    .collect::<Vec<_>>()
    .join("\n")
}

#[test]
fn all_adapters_review_the_same_source_and_preserve_model_arguments() {
    for agent in ["codex", "claude", "copilot"] {
        let project = Project::new();
        project.mock(agent, &clean(), "");
        let output = project.run(agent, &["-e", "high"]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let report = project.report();
        assert_eq!(report["status"], "complete");
        assert_eq!(report["model"], "test-model");
        assert_eq!(report["files"].as_object().unwrap().len(), 2);
        assert_eq!(report["files"]["src/lib.ts"].as_str().unwrap().len(), 64);
        let prompt = fs::read_to_string(project.temp.path().join("input.txt")).unwrap();
        assert!(prompt.contains("export function answer()"));
        let args = fs::read_to_string(project.temp.path().join("args.txt")).unwrap();
        assert!(args.contains("--model\ntest-model\n"));
        match agent {
            "codex" => {
                assert!(args.contains("--ignore-user-config") && args.contains("shell_tool"));
            }
            "claude" => assert!(args.contains("--tools\n\n") && args.contains("disableAllHooks")),
            _ => {
                assert!(args.contains("--agent\nsubmilli-security-review\n"));
                assert!(args.contains("--stream\noff\n") && args.contains("--deny-tool=shell"));
                assert!(args.contains("--output-format\njson\n"));
                assert!(args.contains("--excluded-tools\nskill\nsql\n"));
                let profile = fs::read_to_string(project.temp.path().join("profile.md")).unwrap();
                assert!(profile.contains("\ntools: []\n"));
                assert!(profile.contains("exactly one JSON object"));
            }
        }
    }
}

#[test]
fn copilot_requires_an_unambiguous_successful_tool_free_result() {
    let valid = copilot_events(&clean());
    let final_answer = json!({"type":"assistant.message","data":{"phase":"final_answer","content":clean().to_string()}}).to_string();
    for invalid in [
        valid.replace("\"exitCode\":0", "\"exitCode\":1"),
        valid
            .lines()
            .filter(|line| !line.contains("\"type\":\"result\""))
            .collect::<Vec<_>>()
            .join("\n"),
        valid
            .lines()
            .filter(|line| !line.contains("final_answer"))
            .collect::<Vec<_>>()
            .join("\n"),
        format!("{final_answer}\n{valid}"),
        format!("{valid}\n{{\"type\":\"result\",\"exitCode\":0}}"),
        format!("{{\"type\":\"tool.execution_start\"}}\n{valid}"),
        format!("{{\"type\":\"session.error\"}}\n{valid}"),
        format!("not-json\n{valid}"),
        copilot_events(&json!("not a report")),
    ] {
        let project = Project::new();
        project.mock("copilot", &clean(), "");
        project.write("response.txt", &invalid);
        assert_eq!(project.run("copilot", &[]).status.code(), Some(2));
        assert_eq!(project.report()["status"], "incomplete");
    }
}

#[test]
fn snapshot_includes_hidden_and_artifact_named_compiler_sources() {
    let project = Project::new();
    let paths = [
        "src/.hidden.ts",
        "src/.private/hidden.subm",
        "src/dist/effect.ts",
        "src/target/effect.ts",
        "src/node_modules/effect.ts",
        "src/graphify-out/effect.ts",
        "src/alias\\file.ts",
        "src/alias/file.ts",
    ];
    for path in paths {
        project.write(path, "const value = 1;\n");
    }
    let mut response = clean();
    response["reviewed_files"] = json!(
        ["src/lib.ts", "submilli.toml"]
            .into_iter()
            .chain(paths)
            .collect::<Vec<_>>()
    );
    project.mock("codex", &response, "");
    assert!(project.run("codex", &[]).status.success());
    let report = project.report();
    for path in paths {
        assert!(report["files"][path].is_string(), "{path}");
    }
}

#[test]
fn findings_fail_only_at_the_selected_severity() {
    for (threshold, code) in [("high", 0), ("medium", 1)] {
        let project = Project::new();
        let mut response = clean();
        response["findings"] = json!([{"severity":"medium","title":"Missing scope","path":"src/lib.ts","line":1,"evidence":"An exported call discloses another tenant's result.","recommendation":"Check the owner before returning."}]);
        project.mock("codex", &response, "");
        assert_eq!(
            project
                .run("codex", &["--fail-on", threshold])
                .status
                .code(),
            Some(code)
        );
        assert_eq!(project.report()["findings"].as_array().unwrap().len(), 1);
    }
}

#[test]
fn incomplete_malformed_and_failed_reviews_never_pass() {
    for variant in [
        "incomplete",
        "missing-file",
        "unknown-file",
        "bad-line",
        "invalid",
        "exit",
        "claude-error",
    ] {
        let project = Project::new();
        let mut response = clean();
        let mut behavior = "";
        match variant {
            "incomplete" => response["complete"] = json!(false),
            "missing-file" => response["reviewed_files"] = json!(["src/lib.ts"]),
            "unknown-file" => response["reviewed_files"] = json!(["/etc/passwd"]),
            "bad-line" => {
                response["findings"] = json!([{"severity":"high","title":"Bug","path":"src/lib.ts","line":999,"evidence":"evidence","recommendation":"fix"}]);
            }
            "invalid" => response = json!({}),
            "exit" => behavior = "echo secret-token >&2; exit 7",
            _ => behavior = "printf '%s' '{\"is_error\":true,\"subtype\":\"error\"}'; exit 0",
        }
        let agent = if variant == "claude-error" {
            "claude"
        } else {
            "codex"
        };
        project.mock(agent, &response, behavior);
        let output = project.run(agent, &[]);
        assert_eq!(output.status.code(), Some(2), "{variant}");
        assert_eq!(project.report()["status"], "incomplete");
        assert!(!String::from_utf8_lossy(&output.stderr).contains("secret-token"));
    }
}

#[test]
fn timeout_terminates_the_agent_and_reports_failure() {
    let project = Project::new();
    project.mock("codex", &clean(), "sleep 20");
    let start = std::time::Instant::now();
    let output = project.run("codex", &["--timeout", "1"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(start.elapsed().as_secs() < 10);
    assert!(
        project.report()["error"]
            .as_str()
            .unwrap()
            .contains("timed out")
    );
}

#[test]
fn source_symlinks_size_limits_and_existing_reports_are_rejected() {
    for variant in ["symlink", "size", "existing"] {
        let project = Project::new();
        project.mock("codex", &clean(), "");
        match variant {
            "symlink" => symlink(
                Path::new("/etc/passwd"),
                project.temp.path().join("src/secret.ts"),
            )
            .unwrap(),
            "size" => project.write("src/large.ts", &"x".repeat(512 * 1024)),
            _ => project.write("report.json", "existing report"),
        }
        assert_eq!(project.run("codex", &[]).status.code(), Some(2));
        assert!(!project.temp.path().join("args.txt").exists());
        if variant == "existing" {
            assert_eq!(
                fs::read_to_string(project.temp.path().join("report.json")).unwrap(),
                "existing report"
            );
        }
    }
}

#[test]
fn local_dependency_closure_is_reviewed_but_missing_external_source_is_incomplete() {
    let project = Project::new();
    project.write("submilli.toml", "[dependencies]\n\"@remote/http\" = \"1.0.0\"\n[[package]]\nname = \"@acme/test\"\nversion = \"0.1.0\"\ndescription = \"Test.\"\npath = \"app\"\ndependencies = [\"@acme/helper\", \"@remote/http\"]\n[[package]]\nname = \"@acme/helper\"\nversion = \"0.1.0\"\ndescription = \"Helper.\"\npath = \"helper\"\n[[package]]\nname = \"@acme/unrelated\"\nversion = \"0.1.0\"\ndescription = \"Unrelated.\"\npath = \"unrelated\"\n");
    for package in ["app", "helper", "unrelated"] {
        project.write(
            &format!("{package}/src/lib.ts"),
            "export function f(): number { return 1; }\n",
        );
    }
    let mut response = clean();
    response["reviewed_files"] = json!(["submilli.toml", "app/src/lib.ts", "helper/src/lib.ts"]);
    project.mock("codex", &response, "");
    assert_eq!(
        project.run("codex", &["-p", "@acme/test"]).status.code(),
        Some(2)
    );
    let report = project.report();
    assert_eq!(report["packages"], json!(["@acme/helper", "@acme/test"]));
    assert_eq!(report["files"].as_object().unwrap().len(), 3);
    assert!(
        report["coverage_gaps"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v.as_str().unwrap().contains("@remote/http"))
    );
}
