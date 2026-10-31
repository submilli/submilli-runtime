//! Runs the first-party packages' policy scripts: each is a `main` program
//! executed under a restricted blueprint against the locally published package.
//!
//! `build test` runs package tests under an allow-all policy, so it cannot show
//! that a `main` caller is constrained. These scripts can, and they need no
//! credentials or network: each blueprint denies the package its secret or its
//! HTTP calls, so an allowed call stops at that gated operation.
//!
//! A check whose fields come from the service's answers cannot be reached that
//! way. `attio_status_rules_hold_against_a_fake_attio` covers one: it runs a
//! copy of `@submilli/attio` against a fake Attio on 127.0.0.1.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Packages that ship policy scripts under `tests/policy/`.
const PACKAGES: &[&str] = &[
    "attio",
    "browserbase-stagehand",
    "firecrawl",
    "github",
    "gmail",
    "google-calendar",
    "google-drive",
    "jina",
    "notion",
    "slack-bot",
    "slack-user",
];

#[test]
fn policy_scripts_pass_under_their_blueprints() {
    let home = tempfile::tempdir().expect("package store");
    put_secret(home.path(), "policy-token", "policy-placeholder");
    let mut failures = Vec::new();
    for package in PACKAGES {
        let published = submilli(
            &[
                "build",
                "publish-local",
                "-p",
                &format!("@submilli/{package}"),
            ],
            home.path(),
        );
        if !published.status.success() {
            failures.push(format!(
                "publishing @submilli/{package} failed:\n{}",
                String::from_utf8_lossy(&published.stderr)
            ));
            continue;
        }
        let scripts = policy_scripts(package);
        if scripts.is_empty() {
            failures.push(format!("@submilli/{package} has no policy script"));
        }
        for script in scripts {
            let blueprint = script.with_extension("yaml");
            let run = submilli(
                &[
                    "run",
                    &script.to_string_lossy(),
                    "--blueprint",
                    &blueprint.to_string_lossy(),
                ],
                home.path(),
            );
            if !run.status.success() {
                failures.push(format!(
                    "{} failed:\n{}{}",
                    script.display(),
                    String::from_utf8_lossy(&run.stdout),
                    String::from_utf8_lossy(&run.stderr)
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

const ATTIO_DEAL: &str = "8a1b2c3d-1111-4222-8333-944455556666";
const ATTIO_KEY: &str = "fake-attio-key";

/// `records.setStatus` is checked with the canonical status title and the
/// record's previous status, which the package reads from Attio first, so an
/// offline script never reaches that check. Here a copy of `@submilli/attio`,
/// identical but for its API base, talks to a fake Attio, and
/// `tests/fake-host/status-rules.ts` runs as `main` under its blueprint: the
/// stage change is allowed and written, a change of another status attribute is
/// denied and not written, a status attribute passed as a plain value is
/// refused wherever it appears, and a company or person created with a status
/// is denied by the create-only rules.
#[test]
fn attio_status_rules_hold_against_a_fake_attio() {
    let fake = FakeAttio::start();
    let home = tempfile::tempdir().expect("package store");
    put_secret(home.path(), "policy-token", ATTIO_KEY);
    let project = tempfile::tempdir().expect("package copy");
    copy_attio_with_base(
        project.path(),
        &format!("http://127.0.0.1:{}/v2", fake.port),
    );
    let published = Command::new(env!("CARGO_BIN_EXE_submilli"))
        .args(["build", "publish-local", "-p", "@submilli/attio"])
        .current_dir(project.path())
        .env("SUBMILLI_HOME", home.path())
        .env("SUBMILLI_TELEMETRY", "0")
        .output()
        .expect("publish the package copy");
    assert!(
        published.status.success(),
        "{}",
        String::from_utf8_lossy(&published.stderr)
    );

    let directory = repository_root().join("packages/attio/tests/fake-host");
    let script = directory.join("status-rules.ts");
    let blueprint = directory.join("status-rules.yaml");
    let run = submilli(
        &[
            "run",
            &script.to_string_lossy(),
            "--blueprint",
            &blueprint.to_string_lossy(),
        ],
        home.path(),
    );
    assert!(
        run.status.success(),
        "{}{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );

    let requests = fake.requests();
    assert!(
        !requests.is_empty(),
        "the package copy reached the fake Attio"
    );
    assert!(
        requests.iter().all(|request| request.authorized),
        "every request carries the key: {requests:#?}"
    );
    let writes: Vec<&FakeRequest> = requests
        .iter()
        .filter(|request| request.method != "GET")
        .collect();
    assert_eq!(
        writes.len(),
        1,
        "only the allowed stage change is written: {writes:#?}"
    );
    assert_eq!(writes[0].method, "PATCH");
    assert_eq!(
        writes[0].path,
        format!("/v2/objects/deals/records/{ATTIO_DEAL}")
    );
    assert_eq!(
        writes[0].body,
        serde_json::json!({ "data": { "values": { "stage": [{ "status": "s-won" }] } } })
    );
}

/// Copies the package's sources into `project` with its API base replaced.
fn copy_attio_with_base(project: &Path, base: &str) {
    const SHIPPED: &str = "const API = \"https://api.attio.com/v2\";";
    let source = repository_root().join("packages/attio");
    let target = project.join("packages/attio");
    for directory in ["src", "docs"] {
        std::fs::create_dir_all(target.join(directory)).expect("package directory");
        for entry in std::fs::read_dir(source.join(directory)).expect("package sources") {
            let path = entry.expect("source entry").path();
            let name = path.file_name().expect("a source file name");
            let mut text = std::fs::read_to_string(&path).expect("package source");
            if name == "lib.ts" {
                assert_eq!(
                    text.matches(SHIPPED).count(),
                    1,
                    "lib.ts declares the API base once"
                );
                text = text.replace(SHIPPED, &format!("const API = \"{base}\";"));
            }
            std::fs::write(target.join(directory).join(name), text).expect("copied source");
        }
    }
    std::fs::write(
        project.join("submilli.toml"),
        "[[package]]\nname = \"@submilli/attio\"\nversion = \"0.1.0\"\n\
         description = \"@submilli/attio against a fake Attio\"\nkeywords = [\"attio\"]\n\
         path = \"packages/attio\"\n",
    )
    .expect("manifest");
}

#[derive(Debug)]
struct FakeRequest {
    method: String,
    path: String,
    authorized: bool,
    body: serde_json::Value,
}

/// A fake Attio API on 127.0.0.1 serving one deal whose stage is Lead. It
/// answers one request per connection and records every request.
struct FakeAttio {
    port: u16,
    requests: Arc<Mutex<Vec<FakeRequest>>>,
}

impl FakeAttio {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind the fake Attio");
        let port = listener.local_addr().expect("fake Attio address").port();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&requests);
        // The thread serves until the test process exits.
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                serve(stream, &recorded);
            }
        });
        FakeAttio { port, requests }
    }

    fn requests(&self) -> Vec<FakeRequest> {
        // A poisoned lock means the serving thread panicked; the test fails either way.
        std::mem::take(&mut *self.requests.lock().expect("fake Attio log"))
    }
}

fn serve(stream: TcpStream, recorded: &Mutex<Vec<FakeRequest>>) {
    // A client that stops sending fails this request instead of stalling the server.
    if stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .is_err()
    {
        return;
    }
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    if reader.read_line(&mut line).is_err() {
        return;
    }
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let target = parts.next().unwrap_or_default().to_string();
    let mut length = 0;
    let mut authorized = false;
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header).is_err() || header.trim().is_empty() {
            break;
        }
        let (name, value) = header.split_once(':').unwrap_or((&header, ""));
        let value = value.trim();
        if name.eq_ignore_ascii_case("content-length") {
            length = value.parse().unwrap_or(0);
        }
        if name.eq_ignore_ascii_case("authorization") {
            authorized = value == format!("Bearer {ATTIO_KEY}");
        }
    }
    let mut body = vec![0; length];
    if reader.read_exact(&mut body).is_err() {
        return;
    }
    let path = target.split('?').next().unwrap_or_default().to_string();
    let (status, answer) = fake_answer(&method, &path);
    recorded.lock().expect("fake Attio log").push(FakeRequest {
        method,
        path,
        authorized,
        body: serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null),
    });
    let text = answer.to_string();
    let mut stream = reader.into_inner();
    let _ = write!(
        stream,
        "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{text}",
        text.len()
    );
}

fn fake_answer(method: &str, path: &str) -> (&'static str, serde_json::Value) {
    use serde_json::json;
    let status = |id: &str, title: &str| json!({ "id": { "status_id": id }, "title": title, "is_archived": false });
    let attribute = |slug: &str, kind: &str| {
        json!({ "id": { "attribute_id": format!("a-{slug}") }, "api_slug": slug, "title": slug, "type": kind,
            "is_multiselect": false, "is_writable": true, "is_archived": false })
    };
    let deal = |stage_id: &str, stage: &str| {
        json!({ "data": { "id": { "record_id": ATTIO_DEAL }, "created_at": "2026-10-26T10:00:00Z",
            "web_url": format!("https://app.attio.com/x/deals/{ATTIO_DEAL}"),
            "values": { "name": [{ "attribute_type": "text", "value": "Acme pilot" }],
                "stage": [{ "attribute_type": "status", "status": { "id": { "status_id": stage_id }, "title": stage } }] } } })
    };
    let record = format!("/v2/objects/deals/records/{ATTIO_DEAL}");
    match (method, path) {
        ("GET", "/v2/objects/deals/attributes/stage/statuses") => (
            "200 OK",
            json!({ "data": [status("s-lead", "Lead"), status("s-won", "Won 🎉"), status("s-lost", "Lost")] }),
        ),
        (
            "GET",
            "/v2/objects/companies/attributes/stage/statuses"
            | "/v2/objects/people/attributes/stage/statuses",
        ) => ("200 OK", json!({ "data": [status("c-lead", "Lead")] })),
        ("GET", "/v2/objects/deals/attributes/priority/statuses") => (
            "200 OK",
            json!({ "data": [status("p-high", "High"), status("p-low", "Low")] }),
        ),
        ("GET", "/v2/objects/deals/attributes") => (
            "200 OK",
            json!({ "data": [attribute("name", "text"), attribute("stage", "status"), attribute("priority", "status")] }),
        ),
        ("GET", path) if path == record => ("200 OK", deal("s-lead", "Lead")),
        ("PATCH", path) if path == record => ("200 OK", deal("s-won", "Won 🎉")),
        _ => (
            "404 Not Found",
            json!({ "status_code": 404, "type": "invalid_request_error", "code": "not_found", "message": "Not found" }),
        ),
    }
}

fn put_secret(home: &Path, name: &str, value: &str) {
    let mut put = Command::new(env!("CARGO_BIN_EXE_submilli"))
        .args(["secret", "put", name])
        .env("SUBMILLI_HOME", home)
        .env("SUBMILLI_TELEMETRY", "0")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("put a secret");
    put.stdin
        .take()
        .expect("secret stdin")
        .write_all(value.as_bytes())
        .expect("write the secret");
    let result = put.wait_with_output().expect("secret put finished");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}

/// Fails on any warning that names `check()`: a repeated read or a misplaced
/// `check()`, and a `check()` call with no `@capability`. These only warn, so a
/// package that regresses still builds.
#[test]
fn first_party_packages_build_without_check_warnings() {
    let home = tempfile::tempdir().expect("package store");
    let checked = submilli(&["build", "check"], home.path());
    let stderr = String::from_utf8_lossy(&checked.stderr);
    assert!(checked.status.success(), "build check failed:\n{stderr}");
    let warnings: Vec<&str> = stderr
        .lines()
        .filter(|line| line.contains("warning") && line.contains("`check()`"))
        .collect();
    assert!(warnings.is_empty(), "{}", warnings.join("\n"));
}

/// The agent reads `docs/readme.md` while writing a program. It can run no
/// CLI command and cannot change its blueprint, so setup belongs in the root
/// `readme.md`, which people read. That includes the names of the secrets the
/// packages read: credentials are supplied internally, so the agent never
/// needs them.
#[test]
fn agent_docs_carry_no_setup() {
    const COMMANDS: &[&str] = &["submilli install", "submilli secret", "submilli blueprint"];
    // Blueprint YAML keys, starting a line after its indentation.
    const YAML_KEYS: &[&str] = &["secrets:", "harness:"];
    let packages = repository_root().join("packages");
    let package_dirs: Vec<PathBuf> = std::fs::read_dir(&packages)
        .expect("packages directory")
        .map(|entry| entry.expect("package entry").path())
        .collect();
    let secrets = secret_names(&package_dirs);
    assert!(
        !secrets.is_empty(),
        "no `secrets.get` calls found under packages/*/src"
    );
    let mut found = Vec::new();
    for package in &package_dirs {
        let readme = package.join("docs/readme.md");
        let Ok(text) = std::fs::read_to_string(&readme) else {
            continue;
        };
        for (number, line) in (1..).zip(text.lines()) {
            let trimmed = line.trim_start();
            if COMMANDS.iter().any(|command| line.contains(command))
                || YAML_KEYS.iter().any(|key| trimmed.starts_with(key))
                || secrets.iter().any(|secret| line.contains(secret.as_str()))
            {
                found.push(format!("{}:{number}: {line}", readme.display()));
            }
        }
    }
    assert!(
        found.is_empty(),
        "setup in agent docs:\n{}",
        found.join("\n")
    );
}

/// The names every package passes to `secrets.get("…")` in its `src/`.
fn secret_names(package_dirs: &[PathBuf]) -> Vec<String> {
    const CALL: &str = "secrets.get(\"";
    let mut names = Vec::new();
    for package in package_dirs {
        let Ok(sources) = std::fs::read_dir(package.join("src")) else {
            continue;
        };
        for source in sources {
            let Ok(text) = std::fs::read_to_string(source.expect("source entry").path()) else {
                continue;
            };
            for call in text.split(CALL).skip(1) {
                if let Some((name, _)) = call.split_once('"') {
                    names.push(name.to_string());
                }
            }
        }
    }
    names.sort();
    names.dedup();
    names
}

/// Every `<name>.ts` with a `<name>.yaml` beside it under the package's `tests/policy/`.
fn policy_scripts(package: &str) -> Vec<PathBuf> {
    let directory = repository_root()
        .join("packages")
        .join(package)
        .join("tests/policy");
    let mut scripts: Vec<PathBuf> = std::fs::read_dir(&directory)
        .unwrap_or_else(|error| panic!("reading {}: {error}", directory.display()))
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "ts"))
        .filter(|path| path.with_extension("yaml").is_file())
        .collect();
    scripts.sort();
    scripts
}

fn submilli(args: &[&str], home: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_submilli"))
        .args(args)
        .current_dir(repository_root())
        .env("SUBMILLI_HOME", home)
        .env("SUBMILLI_TELEMETRY", "0")
        .output()
        .expect("invoke submilli")
}

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}
