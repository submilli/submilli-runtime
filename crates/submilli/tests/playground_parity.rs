//! The playground keeps its own copy of the server's sessions, MCP endpoint and run
//! path. This test runs the same programs, blueprint, variables and package through
//! a real `submilli playground` and an in-process `submilli-server`, and checks that
//! what an agent sees over MCP is the same on both.
//!
//! When it fails, either port the server change to the playground host
//! (`crates/submilli/src/commands/playground/host/`) or, when the playground is
//! meant to differ for now, add an entry with an issue link to [`KNOWN_DIFFERENCES`].

use serde_json::{Value, json};

/// A difference the playground is known to have, tracked by its own issue.
struct KnownDifference {
    /// The observation it covers, as named in [`observations`].
    observation: &'static str,
    /// The issue that brings the playground in step, or records why it differs.
    issue: &'static str,
}

const KNOWN_DIFFERENCES: &[KnownDifference] = &[];

/// One thing an agent sees, already normalized.
type Observation = (String, Value);

/// The failure message for every observation that differs and is not a known
/// difference, or `None` when the hosts agree.
fn compare(
    playground: &[Observation],
    server: &[Observation],
    known: &[KnownDifference],
) -> Option<String> {
    let mut differences = Vec::new();
    let names: Vec<&String> = server.iter().map(|(name, _)| name).collect();
    assert_eq!(
        playground.iter().map(|(name, _)| name).collect::<Vec<_>>(),
        names,
        "both hosts must make the same observations"
    );
    for ((name, ours), (_, theirs)) in playground.iter().zip(server) {
        if ours == theirs || known.iter().any(|entry| entry.observation == name) {
            continue;
        }
        differences.push(format!(
            "`{name}` differs.\n  server:     {}\n  playground: {}",
            serde_json::to_string_pretty(theirs).unwrap(),
            serde_json::to_string_pretty(ours).unwrap()
        ));
    }
    if differences.is_empty() {
        return None;
    }
    Some(format!(
        "The playground host and the server no longer show an agent the same thing.\n\n{}\n\n\
         Port the server change to the playground host \
         (crates/submilli/src/commands/playground/host/), or, if the playground should \
         differ for now, add a known-differences entry with an issue link in \
         crates/submilli/tests/playground_parity.rs.",
        differences.join("\n\n")
    ))
}

#[test]
fn known_differences_each_carry_an_issue_link() {
    for entry in KNOWN_DIFFERENCES {
        assert!(
            entry.issue.starts_with("https://linear.app/"),
            "known difference `{}` needs an issue link, not {:?}",
            entry.observation,
            entry.issue
        );
    }
}

#[test]
fn a_difference_fails_with_both_ways_out() {
    let server = vec![("execute".to_owned(), json!({ "result": "2" }))];
    let playground = vec![("execute".to_owned(), json!({ "result": "3" }))];
    let message = compare(&playground, &server, &[]).expect("a difference");
    assert!(message.contains("`execute` differs"), "{message}");
    assert!(message.contains("Port the server change"), "{message}");
    assert!(message.contains("known-differences entry"), "{message}");
    let known = [KnownDifference {
        observation: "execute",
        issue: "https://linear.app/example",
    }];
    assert_eq!(compare(&playground, &server, &known), None);
}

#[test]
fn a_failure_both_hosts_share_still_fails() {
    let server = vec![
        ("execute: a result".to_owned(), json!({ "isError": true })),
        ("finalizer ran on close".to_owned(), json!(false)),
    ];
    let missing = missing_facts(&server);
    assert!(
        missing
            .iter()
            .any(|fact| fact.contains("execute: a result")),
        "{missing:?}"
    );
    assert!(
        missing
            .iter()
            .any(|fact| fact.contains("finalizer ran on close")),
        "{missing:?}"
    );
}

/// What the server must show for the comparison to mean anything: without these,
/// a setup problem both hosts share would make every observation equal.
fn missing_facts(server: &[Observation]) -> Vec<String> {
    let find = |name: &str| {
        server
            .iter()
            .find(|(seen, _)| seen == name)
            .map_or(Value::Null, |(_, value)| value.clone())
    };
    let succeeded_with = |name: &str, text: &str| {
        let value = find(name);
        value.get("isError") != Some(&json!(true)) && value.to_string().contains(text)
    };
    let mut missing = Vec::new();
    if !succeeded_with("execute: a result", "2") {
        missing.push("`execute: a result` returns 2".to_owned());
    }
    if !succeeded_with("execute: a later run reads them", "hi v1") {
        missing.push("`execute: a later run reads them` returns `hi v1`".to_owned());
    }
    if !succeeded_with("execute: a package registers a finalizer", "held") {
        missing.push("`execute: a package registers a finalizer` returns `held`".to_owned());
    }
    if find("finalizer ran on close") != json!(true) {
        missing.push("`finalizer ran on close` is true".to_owned());
    }
    missing
}

/// Replace what legitimately varies between two runs: ids, session ids, timings,
/// and the temporary directories each host lives in.
fn normalize(value: &mut Value, dirs: &[String]) {
    match value {
        Value::Object(map) => {
            for (key, field) in map.iter_mut() {
                if is_timing(key) && field.is_number() {
                    *field = json!("<time>");
                } else {
                    normalize(field, dirs);
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(|item| normalize(item, dirs)),
        Value::String(text) => {
            let mut normalized = replace_uuids(text);
            for dir in dirs {
                normalized = normalized.replace(dir.as_str(), "<dir>");
            }
            *text = normalized;
        }
        _ => {}
    }
}

fn is_timing(key: &str) -> bool {
    key.ends_with("_ms")
        || key.ends_with("_at")
        || key.contains("duration")
        || key.contains("elapsed")
}

/// Every `8-4-4-4-12` hex id in `text`, written as `<id>`.
fn replace_uuids(text: &str) -> String {
    const SHAPE: [usize; 5] = [8, 4, 4, 4, 12];
    let bytes = text.as_bytes();
    let matches_at = |start: usize| -> Option<usize> {
        let mut at = start;
        for (index, len) in SHAPE.iter().enumerate() {
            if index > 0 {
                (bytes.get(at) == Some(&b'-')).then_some(())?;
                at += 1;
            }
            let run = bytes.get(at..at + len)?;
            run.iter().all(u8::is_ascii_hexdigit).then_some(())?;
            at += len;
        }
        (!bytes.get(at).is_some_and(u8::is_ascii_hexdigit)).then_some(at)
    };
    let mut out = String::new();
    let mut at = 0;
    while at < bytes.len() {
        let boundary = at == 0 || !bytes[at - 1].is_ascii_hexdigit();
        if let Some(end) = boundary.then(|| matches_at(at)).flatten() {
            out.push_str("<id>");
            at = end;
        } else {
            let ch = text[at..].chars().next().unwrap();
            out.push(ch);
            at += ch.len_utf8();
        }
    }
    out
}

#[test]
fn normalizing_hides_ids_timings_and_directories() {
    let mut value = json!({
        "session": "0191e7a2-3b4c-7d5e-8f90-a1b2c3d4e5f6",
        "duration_ms": 12,
        "message": "wrote /tmp/x/a.txt in run 0191e7a2-3b4c-7d5e-8f90-a1b2c3d4e5f6",
        "count": 3,
    });
    normalize(&mut value, &["/tmp/x".to_owned()]);
    assert_eq!(
        value,
        json!({
            "session": "<id>",
            "duration_ms": "<time>",
            "message": "wrote <dir>/a.txt in run <id>",
            "count": 3,
        })
    );
}

#[cfg(unix)]
mod hosts {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Output};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    use axum::body::Body;
    use axum::http::Request;
    use serde_json::{Value, json};
    use submilli_server::blueprint::InMemoryBlueprintStore;
    use submilli_server::{AppState, NetworkPolicy, ServerConfig, app};
    use tower::ServiceExt;

    use super::{KNOWN_DIFFERENCES, Observation, compare, normalize};

    const BIN: &str = env!("CARGO_BIN_EXE_submilli");
    const BLUEPRINT_NAME: &str = "demo";
    const EXECUTE: &str = "submilli__typescript__execute";
    const LAST_RUN: &str = "submilli__typescript__last_run";

    /// One blueprint both hosts serve: a session filesystem, a fixed variable a
    /// refusal depends on, the test package, and an MCP server at `__MCP__` whose
    /// catalog gives every run discovery warnings.
    const BLUEPRINT: &str = "name: demo
default: allow
allow_insecure_http: true
variables:
  tenant:
    required: true
vfs:
  mode: per_session
packages: ['@parity/res']
permissions:
  main:
    - capability: test.com/op
      filter: userId == ${vars.tenant}
      action: deny
  '@parity/res':
    - capability: http.get
      action: allow
    - capability: session.finalize
      action: allow
mcp:
  up:
    transport: streamable_http
    url: __MCP__
";

    fn blueprint(upstream: &str) -> String {
        BLUEPRINT.replace("__MCP__", upstream)
    }

    /// An MCP server on loopback whose catalog has a tool that cannot be imported
    /// and one with no output schema, each a discovery warning.
    fn upstream() -> String {
        use axum::Json;
        use axum::http::StatusCode;
        use axum::response::{IntoResponse, Response};

        async fn answer(Json(message): Json<Value>) -> Response {
            let Some(id) = message.get("id").cloned() else {
                return StatusCode::ACCEPTED.into_response();
            };
            let result = match message["method"].as_str() {
                Some("initialize") => json!({
                    "protocolVersion": message["params"]["protocolVersion"],
                    "capabilities": { "tools": {} },
                    "serverInfo": { "name": "up", "version": "0" }
                }),
                Some("tools/list") => json!({ "tools": [
                    { "name": "not-an-identifier", "description": "Dropped.",
                      "inputSchema": { "type": "object" } },
                    { "name": "ping", "description": "Untyped.",
                      "inputSchema": { "type": "object" } }
                ] }),
                _ => json!({}),
            };
            Json(json!({ "jsonrpc": "2.0", "id": id, "result": result })).into_response()
        }

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}/mcp", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            runtime.block_on(async move {
                let listener = tokio::net::TcpListener::from_std(listener).unwrap();
                let router = axum::Router::new().route("/mcp", axum::routing::post(answer));
                let _ = axum::serve(listener, router).await;
            });
        });
        url
    }

    /// A package that keeps a remote resource open and releases it by calling
    /// `url` from its finalizer when the session closes.
    const PACKAGE: &str = r#"import * as session from "submilli:session";
import { get } from "submilli:http";
/** Hold `url` open until the session closes. */
export function retain(url: string): void { session.registerFinalizer(url, url); }
/** Release `value`. */
export function finalizer(key: string, value: string): void { get(value); }
"#;

    /// Each program, in order, in one MCP session.
    const PROGRAMS: &[(&str, &str)] = &[
        (
            "execute: a result",
            "function main(): number { return 1 + 1; }",
        ),
        (
            "execute: console on success",
            r#"function main(): string { console.log("hello"); return "done"; }"#,
        ),
        (
            "execute: write a file and a value",
            r#"import { writeText } from "submilli:fs";
import session from "submilli:session";
function main(): void { writeText("/a.txt", "hi"); session.set("k", "v1"); }"#,
        ),
        (
            "execute: a later run reads them",
            r#"import { readText } from "submilli:fs";
import session from "submilli:session";
function main(): string { return `${readText("/a.txt")} ${session.get("k") as string}`; }"#,
        ),
        (
            "execute: a compile error",
            "function main(): number { return \"no\"; }",
        ),
        (
            "execute: a thrown error",
            r#"function main(): number { console.log("before"); throw new Error("boom"); }"#,
        ),
        (
            "execute: console overflow",
            r#"function main(): number { for (let i = 0; i < 200000; i++) { console.log(`line ${i} of a long console`); } throw new Error("after the console"); }"#,
        ),
        (
            "execute: importing a discovered server",
            r#"import up from "@mcp/up"; function main(): string { return "imported"; }"#,
        ),
        (
            "execute: refused by the blueprint",
            r#"import { check } from "submilli:security";
function main(): number { check("test.com/op", { userId: "pied_piper" }); return 1; }"#,
        ),
    ];

    const REGISTER_FROM_PROGRAM: &str = r#"import * as session from "submilli:session";
function main(): string { try { session.registerFinalizer("k", "v"); } catch (error: Error) { return error.message; } return "registered"; }"#;

    /// An MCP endpoint to talk to: the playground over HTTP, or the server in process.
    trait Host {
        /// Status, `mcp-session-id`, and the JSON-RPC message of a request.
        fn send(
            &self,
            method: &str,
            body: Option<Value>,
            session: Option<&str>,
        ) -> (u16, Option<String>, Value);
        /// Directories the host's output may name.
        fn dirs(&self) -> Vec<String>;
    }

    fn message(content_type: &str, text: &str) -> Value {
        if text.is_empty() {
            return Value::Null;
        }
        if content_type.starts_with("text/event-stream") {
            let data = text
                .lines()
                .filter_map(|line| line.strip_prefix("data:"))
                .next_back()
                .unwrap_or("")
                .trim();
            return serde_json::from_str(data).unwrap_or(Value::Null);
        }
        serde_json::from_str(text).unwrap_or(Value::Null)
    }

    struct Server {
        runtime: tokio::runtime::Runtime,
        state: AppState,
        dir: tempfile::TempDir,
    }

    impl Server {
        fn new(package_store: &Path, blueprint: &str) -> Self {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .unwrap();
            let dir = tempfile::tempdir().unwrap();
            std::fs::create_dir(dir.path().join("volumes")).unwrap();
            let blueprint = submilli_blueprint::parse(blueprint).expect("blueprint");
            let config = ServerConfig {
                blueprints: Some(Arc::new(
                    InMemoryBlueprintStore::seed([blueprint]).expect("seed"),
                )),
                volume_root: Some(dir.path().join("volumes")),
                package_store_root: Some(package_store.to_path_buf()),
                network_policy: NetworkPolicy::deny_private().allow_localhost(true),
                ..Default::default()
            };
            // Booting installs the finalizer runner, as a served server does.
            let state = runtime.block_on(async {
                let state = AppState::new(config).await.expect("AppState");
                state.boot().await.expect("boot");
                state
            });
            Self {
                runtime,
                state,
                dir,
            }
        }
    }

    impl Host for Server {
        fn send(
            &self,
            method: &str,
            body: Option<Value>,
            session: Option<&str>,
        ) -> (u16, Option<String>, Value) {
            let mut builder = Request::builder()
                .method(method)
                .uri(format!("/mcp/{BLUEPRINT_NAME}"))
                .header("host", "localhost")
                .header("content-type", "application/json")
                .header("accept", "application/json, text/event-stream");
            if let Some(session) = session {
                builder = builder.header("mcp-session-id", session);
            }
            let body = body.map_or_else(Body::empty, |body| Body::from(body.to_string()));
            let request = builder.body(body).unwrap();
            self.runtime.block_on(async {
                let response = app(self.state.clone()).oneshot(request).await.unwrap();
                let status = response.status().as_u16();
                let header = |name: &str| {
                    response
                        .headers()
                        .get(name)
                        .and_then(|value| value.to_str().ok())
                        .map(str::to_owned)
                };
                let session = header("mcp-session-id");
                let content_type = header("content-type").unwrap_or_default();
                let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
                    .await
                    .unwrap();
                let text = String::from_utf8_lossy(&bytes);
                (status, session, message(&content_type, &text))
            })
        }

        fn dirs(&self) -> Vec<String> {
            vec![self.dir.path().display().to_string()]
        }
    }

    /// A project with the blueprint, and a playground started on it. Dropping it
    /// stops the playground.
    struct Playground {
        dir: tempfile::TempDir,
        root: PathBuf,
        url: String,
        token: String,
    }

    fn command(home: &Path, cwd: &Path, args: &[&str]) -> Command {
        let mut command = Command::new(BIN);
        command
            .args(args)
            .current_dir(cwd)
            .env("SUBMILLI_HOME", home)
            .env("HOME", home)
            .env_remove("SUBMILLI_TELEMETRY")
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

    fn succeed(output: &Output, what: &str) {
        assert!(
            output.status.success(),
            "{what} failed: {}\n{}",
            String::from_utf8_lossy(&output.stderr),
            String::from_utf8_lossy(&output.stdout)
        );
    }

    impl Playground {
        /// Publishes the test package into the home's package store, then starts the
        /// playground on a project that serves the blueprint.
        fn start(blueprint: &str) -> Self {
            let dir = tempfile::tempdir().unwrap();
            let home = dir.path().join("home");
            let root = dir.path().join("project");
            let submilli = root.join("submilli");
            let package = submilli.join("packages/res");
            std::fs::create_dir_all(package.join("src")).unwrap();
            std::fs::create_dir_all(package.join("docs")).unwrap();
            std::fs::create_dir_all(submilli.join("blueprints")).unwrap();
            std::fs::write(
                submilli.join("submilli.toml"),
                "[[package]]\nname = \"@parity/res\"\nversion = \"0.1.0\"\n\
                 description = \"Resources.\"\npath = \"packages/res\"\n",
            )
            .unwrap();
            std::fs::write(package.join("docs/readme.md"), "# @parity/res\n").unwrap();
            std::fs::write(package.join("src/lib.ts"), PACKAGE).unwrap();
            std::fs::write(submilli.join("blueprints/demo.yaml"), blueprint).unwrap();
            // The server reads the same build from the home's package store.
            let output = command(&home, &submilli, &["build", "publish-local"])
                .output()
                .unwrap();
            succeed(&output, "publish-local");
            let output = command(
                &home,
                &root,
                &["playground", "start", "--json", "--allow-localhost"],
            )
            .output()
            .unwrap();
            let mut playground = Self {
                dir,
                root,
                url: String::new(),
                token: String::new(),
            };
            succeed(&output, "playground start");
            let record: Value = serde_json::from_slice(&output.stdout).unwrap();
            playground.url = format!(
                "{}/mcp/{BLUEPRINT_NAME}",
                record["server_url"].as_str().unwrap()
            );
            playground.token = std::fs::read_to_string(record["app_token_file"].as_str().unwrap())
                .unwrap()
                .trim()
                .to_owned();
            playground
        }

        fn home(&self) -> PathBuf {
            self.dir.path().join("home")
        }

        fn package_store(&self) -> PathBuf {
            self.home().join("packages")
        }
    }

    impl Drop for Playground {
        fn drop(&mut self) {
            let _ = command(&self.home(), &self.root, &["playground", "stop", "--json"]).output();
        }
    }

    impl Host for Playground {
        fn send(
            &self,
            method: &str,
            body: Option<Value>,
            session: Option<&str>,
        ) -> (u16, Option<String>, Value) {
            let agent: ureq::Agent = ureq::Agent::config_builder()
                .http_status_as_error(false)
                .timeout_global(Some(Duration::from_secs(60)))
                .build()
                .into();
            let mut builder = ureq::http::Request::builder()
                .method(method)
                .uri(&self.url)
                .header("authorization", format!("Bearer {}", self.token))
                .header("content-type", "application/json")
                .header("accept", "application/json, text/event-stream");
            if let Some(session) = session {
                builder = builder.header("mcp-session-id", session);
            }
            let mut response = match body {
                Some(body) => agent.run(builder.body(body.to_string()).unwrap()),
                None => agent.run(builder.body(()).unwrap()),
            }
            .unwrap();
            let status = response.status().as_u16();
            let header = |name: &str| {
                response
                    .headers()
                    .get(name)
                    .and_then(|value| value.to_str().ok())
                    .map(str::to_owned)
            };
            let session = header("mcp-session-id");
            let content_type = header("content-type").unwrap_or_default();
            let text = response.body_mut().read_to_string().unwrap_or_default();
            (status, session, message(&content_type, &text))
        }

        fn dirs(&self) -> Vec<String> {
            let dir = self.dir.path();
            let mut dirs = vec![dir.display().to_string()];
            if let Ok(real) = dir.canonicalize() {
                dirs.insert(0, real.display().to_string());
            }
            dirs
        }
    }

    /// Counts requests to a loopback address, which the package's finalizer calls.
    struct Releases {
        url: String,
        hits: Arc<AtomicUsize>,
    }

    impl Releases {
        fn start() -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let url = format!("http://{}/release", listener.local_addr().unwrap());
            let hits = Arc::new(AtomicUsize::new(0));
            let counted = Arc::clone(&hits);
            std::thread::spawn(move || {
                for stream in listener.incoming() {
                    let Ok(mut stream) = stream else { continue };
                    let mut buffer = [0_u8; 4096];
                    let _ = stream.read(&mut buffer);
                    counted.fetch_add(1, Ordering::SeqCst);
                    let _ = stream.write_all(
                        b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\nconnection: close\r\n\r\nok",
                    );
                }
            });
            Self { url, hits }
        }

        /// Whether a new release arrives within a few seconds of `before`.
        fn released_after(&self, before: usize) -> bool {
            let deadline = Instant::now() + Duration::from_secs(20);
            while Instant::now() < deadline {
                if self.hits.load(Ordering::SeqCst) > before {
                    return true;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            false
        }
    }

    fn rpc(id: u32, method: &str, params: Value) -> Value {
        json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })
    }

    fn initialize() -> Value {
        rpc(
            1,
            "initialize",
            json!({
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": { "name": "parity", "version": "0" },
                "_meta": { "variables": { "tenant": "pied_piper" } }
            }),
        )
    }

    /// `initialize` and `notifications/initialized`; the session id and the
    /// instructions the host gave.
    fn handshake(host: &dyn Host) -> (String, Value) {
        let (status, session, reply) = host.send("POST", Some(initialize()), None);
        assert_eq!(status, 200, "initialize: {reply}");
        let session = session.expect("a session id");
        let initialized = json!({ "jsonrpc": "2.0", "method": "notifications/initialized" });
        let (status, _, _) = host.send("POST", Some(initialized), Some(&session));
        assert_eq!(status, 202, "initialized");
        (session, reply["result"]["instructions"].clone())
    }

    fn call(host: &dyn Host, session: &str, id: u32, tool: &str, arguments: Value) -> Value {
        let (status, _, reply) = host.send(
            "POST",
            Some(rpc(
                id,
                "tools/call",
                json!({ "name": tool, "arguments": arguments }),
            )),
            Some(session),
        );
        assert_eq!(status, 200, "{tool}: {reply}");
        reply["result"].clone()
    }

    /// Everything an agent sees in one session, then the close, in order.
    fn observations(host: &dyn Host, releases: &Releases) -> Vec<Observation> {
        let mut seen: Vec<Observation> = Vec::new();
        let (session, instructions) = handshake(host);
        seen.push(("initialize instructions".into(), instructions));
        let (_, _, tools) = host.send(
            "POST",
            Some(rpc(2, "tools/list", json!({}))),
            Some(&session),
        );
        seen.push(("tools/list".into(), tools["result"].clone()));

        let mut id = 10;
        for (name, code) in PROGRAMS {
            id += 1;
            seen.push((
                (*name).to_owned(),
                call(host, &session, id, EXECUTE, json!({ "code": code })),
            ));
        }
        // An MCP agent sees discovery's outcome in the server's docs: the tool that
        // was dropped is missing, the untyped one is marked.
        seen.push((
            "packages docs of a discovered server".into(),
            call(
                host,
                &session,
                29,
                "submilli__typescript__packages__docs",
                json!({ "name": "@mcp/up" }),
            ),
        ));
        seen.push((
            "last_run after a refused run".into(),
            call(host, &session, 30, LAST_RUN, json!({})),
        ));
        seen.push((
            "execute: registering a finalizer from program code".into(),
            call(
                host,
                &session,
                31,
                EXECUTE,
                json!({ "code": REGISTER_FROM_PROGRAM }),
            ),
        ));
        let retain = format!(
            "import {{ retain }} from \"@parity/res\";\nfunction main(): string {{ retain({}); return \"held\"; }}",
            serde_json::to_string(&releases.url).unwrap()
        );
        seen.push((
            "execute: a package registers a finalizer".into(),
            call(host, &session, 32, EXECUTE, json!({ "code": retain })),
        ));

        let before = releases.hits.load(Ordering::SeqCst);
        let (status, _, _) = host.send("DELETE", None, Some(&session));
        seen.push(("DELETE status".into(), json!(status)));
        seen.push((
            "finalizer ran on close".into(),
            json!(releases.released_after(before)),
        ));

        let dirs = host.dirs();
        for (_, value) in &mut seen {
            normalize(value, &dirs);
        }
        seen
    }

    #[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
    #[test]
    fn an_agent_sees_the_same_on_the_playground_and_the_server() {
        let releases = Releases::start();
        let blueprint = blueprint(&upstream());
        let playground = Playground::start(&blueprint);
        let server = Server::new(&playground.package_store(), &blueprint);

        let from_server = observations(&server, &releases);
        let missing = super::missing_facts(&from_server);
        assert!(
            missing.is_empty(),
            "the server did not show what the comparison relies on: {missing:?}"
        );
        let from_playground = observations(&playground, &releases);

        if let Some(message) = compare(&from_playground, &from_server, KNOWN_DIFFERENCES) {
            panic!("{message}");
        }
    }
}
