//! End-to-end coverage of the `permissions:` policy through `/v1/execute`: a
//! script calls `submilli:security.check`, and the blueprint's rules decide
//! whether it runs to completion or the check traps.

#[path = "common/in_memory_config.rs"]
mod in_memory_config;

use std::path::Path;
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use interpreter::{ModulePath, PackageSourceModule, compile_package};
use serde_json::{Value, json};
use submilli_build::{ArtifactMetadata, write_package_artifact};
use submilli_server::blueprint::InMemoryBlueprintStore;
use submilli_server::{AppState, ServerConfig, app};
use tower::ServiceExt;

const BLUEPRINT_NAME: &str = "policy";

/// A script that asks for `test.com/op` with a sub-500 amount.
const CHECK_SCRIPT: &str = r#"
import { check } from "submilli:security";
function main(): number { check("test.com/op", { amount: 100 }); return 1; }
"#;

fn router(policy_yaml: &str, package_store_root: Option<&Path>) -> Router {
    let blueprint = submilli_blueprint::parse(policy_yaml).expect("valid policy blueprint");
    let blueprints = Arc::new(InMemoryBlueprintStore::seed([blueprint]).expect("seed blueprints"));
    let config = ServerConfig {
        blueprints: Some(blueprints),
        package_store_root: package_store_root.map(Path::to_path_buf),
        ..in_memory_config::config()
    };
    app(futures::executor::block_on(AppState::new(config)).expect("build AppState"))
}

async fn send(router: Router, code: &str) -> (StatusCode, Value) {
    send_with_secrets(router, code, json!({})).await
}

async fn send_with_secrets(router: Router, code: &str, secrets: Value) -> (StatusCode, Value) {
    let body = json!({ "code": code, "blueprint": BLUEPRINT_NAME, "secrets": secrets }).to_string();
    let req = Request::builder()
        .method("POST")
        .uri("/v1/execute")
        .header("content-type", "application/json")
        .body(Body::from(body))
        .unwrap();
    let resp = router.oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let parsed: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, parsed)
}

async fn execute(policy_yaml: &str, code: &str) -> (StatusCode, Value) {
    send(router(policy_yaml, None), code).await
}

async fn execute_with_vars(policy_yaml: &str, code: &str, variables: Value) -> (StatusCode, Value) {
    let body =
        json!({ "code": code, "blueprint": BLUEPRINT_NAME, "variables": variables }).to_string();
    let req = Request::builder()
        .method("POST")
        .uri("/v1/execute")
        .header("content-type", "application/json")
        .body(Body::from(body))
        .unwrap();
    let resp = router(policy_yaml, None).oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let parsed: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, parsed)
}

/// A script that asks for `test.com/op` carrying a `userId` a `${vars.NAME}`
/// filter can scope against.
const VAR_SCRIPT: &str = r#"
import { check } from "submilli:security";
function main(): number { check("test.com/op", { userId: "u_42" }); return 1; }
"#;

const VAR_POLICY: &str = "\
name: policy
default: deny
variables:
  tenant:
    required: true
permissions:
  main:
    - capability: test.com/op
      filter: userId == ${vars.tenant}
      action: allow
";

#[tokio::test]
async fn variable_filter_allows_matching_value() {
    let (status, body) =
        execute_with_vars(VAR_POLICY, VAR_SCRIPT, json!({ "tenant": "u_42" })).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["result"], json!("1"));
    assert_eq!(body["error"], Value::Null);
}

#[tokio::test]
async fn variable_filter_denies_mismatched_value() {
    // The bound tenant differs from the script's userId, so the filter misses and
    // the call falls through to `default: deny` — the check traps.
    let (status, body) =
        execute_with_vars(VAR_POLICY, VAR_SCRIPT, json!({ "tenant": "u_99" })).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["result"], Value::Null);
    assert_eq!(body["error"]["kind"], json!("permission_denied"));
}

#[tokio::test]
async fn missing_required_variable_rejects_request() {
    // No `tenant` supplied: the request is rejected before the program runs.
    let (status, body) = execute_with_vars(VAR_POLICY, VAR_SCRIPT, json!({})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["result"], Value::Null);
    assert_eq!(body["error"]["kind"], json!("invalid_request"));
}

#[tokio::test]
async fn unknown_variable_rejects_request() {
    let (status, body) = execute_with_vars(
        VAR_POLICY,
        VAR_SCRIPT,
        json!({ "tenant": "u_42", "bogus": "x" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["error"]["kind"], json!("invalid_request"));
}

#[tokio::test]
async fn allowed_check_runs_to_completion() {
    let policy = "\
name: policy
default: deny
permissions:
  main:
    - capability: test.com/op
      filter: amount < 500
      action: allow
";
    let (status, body) = execute(policy, CHECK_SCRIPT).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["result"], json!("1"));
    assert_eq!(body["error"], Value::Null);
}

#[tokio::test]
async fn denied_check_traps() {
    // Capability allowed only over 500; our call passes amount=100, so it falls
    // through to `default: deny` and the check traps.
    let policy = "\
name: policy
default: deny
permissions:
  main:
    - capability: test.com/op
      filter: amount >= 500
      action: allow
";
    let (status, body) = execute(policy, CHECK_SCRIPT).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["result"], Value::Null);
    assert_eq!(body["error"]["kind"], json!("permission_denied"));
}

#[tokio::test]
async fn no_policy_denies_by_default() {
    // The server is always deny-by-default: a policy-free blueprint (no
    // `permissions:`, no `default:`) denies every capability, so the check traps.
    let (status, body) = execute("name: policy\n", CHECK_SCRIPT).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["result"], Value::Null);
    assert_eq!(body["error"]["kind"], json!("permission_denied"));
}

#[tokio::test]
async fn caller_distinction_main_allowed() {
    // The capability is authorized for `main` (the user script's caller id),
    // but not for any library package — only `main` runs here, so it is allowed.
    // The package-caller half is covered below.
    let policy = "\
name: policy
default: deny
permissions:
  main:
    - capability: test.com/op
      action: allow
  some.pkg/sdk:
    - capability: other.com/op
      action: allow
";
    let (status, body) = execute(policy, CHECK_SCRIPT).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["result"], json!("1"));
    assert_eq!(body["error"], Value::Null);
}

/// The carve-out surfaces at the HTTP layer the way any runtime trap does: 200
/// with a populated `error`, because the refusal is a guest-catchable throw and
/// not a transport failure.
#[tokio::test]
async fn secrets_get_is_refused_to_main_even_with_an_explicit_allow_rule() {
    let policy = "\
name: policy
default: deny
secrets:
  TOKEN:
    harness: {}
permissions:
  main:
    - capability: secrets.get
      filter: name == \"TOKEN\"
      action: allow
";
    let script = r#"
import { get } from "submilli:secrets";
function main(): string | null { return get("TOKEN"); }
"#;

    let (status, body) =
        send_with_secrets(router(policy, None), script, json!({"TOKEN": "tok-123"})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["result"], Value::Null);
    assert_eq!(body["error"]["kind"], json!("permission_denied"));
    assert!(
        body["error"]["message"]
            .as_str()
            .is_some_and(|m| m.contains("no policy can grant this")),
        "got: {body}"
    );
}

#[tokio::test]
async fn secrets_get_is_refused_to_main_for_an_undeclared_secret_too() {
    let policy = "\
name: policy
default: deny
permissions:
  main:
    - capability: secrets.get
      filter: name == \"MISSING\"
      action: allow
";
    let script = r#"
import { get } from "submilli:secrets";
function main(): string | null { return get("MISSING"); }
"#;

    let (status, body) = execute(policy, script).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["result"], Value::Null);
    assert_eq!(body["error"]["kind"], json!("permission_denied"));
}

/// The package side of the two tests above: a declared secret still resolves,
/// and an undeclared one is still `null`, when the caller is a package.
///
/// The package reports what it saw rather than returning the value. Handing the
/// plaintext back to `main` is the shape the carve-out exists to prevent, and a
/// fixture doing it would read as a template.
const SECRETS_SDK_SOURCE: &str = r#"
import { get } from "submilli:secrets";

/** True when the named secret resolves to exactly `expected`. */
export function secretMatches(name: string, expected: string): boolean {
    const value = get(name);
    return value !== null && value === expected;
}

/** True when the named secret resolves to nothing. */
export function secretIsAbsent(name: string): boolean {
    return get(name) === null;
}
"#;

fn write_secrets_package(store_root: &Path) {
    let package = compile_package(
        "@acme/secrets",
        ModulePath::from("lib"),
        &[PackageSourceModule {
            path: ModulePath::from("lib"),
            source: SECRETS_SDK_SOURCE,
        }],
        &[],
    )
    .expect("compile @acme/secrets");
    write_package_artifact(
        store_root.join("@acme").join("secrets"),
        &package.wasm,
        &package.type_info,
        &submilli_build::derive_capability_schema(
            &package.declaration,
            &[],
            &package.required_capabilities,
        ),
        &package.declaration,
        &ArtifactMetadata::new("@acme/secrets", "0.0.0-test", Vec::new()),
    )
    .expect("write @acme/secrets artifact");
}

#[tokio::test]
async fn a_package_still_resolves_a_declared_secret_and_gets_null_for_an_undeclared_one() {
    let store = tempfile::tempdir().expect("store tempdir");
    write_secrets_package(store.path());
    let policy = "\
name: policy
default: deny
packages:
  - \"@acme/secrets\"
secrets:
  TOKEN:
    harness: {}
permissions:
  \"@acme/secrets\":
    - capability: secrets.get
      action: allow
";
    let script = r#"
import { secretMatches, secretIsAbsent } from "@acme/secrets";
function main(): string {
    const found = secretMatches("TOKEN", "tok-123");
    const absent = secretIsAbsent("MISSING");
    return found.toString() + "/" + absent.toString();
}
"#;

    let (status, body) = send_with_secrets(
        router(policy, Some(store.path())),
        script,
        json!({"TOKEN": "tok-123"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    assert_eq!(body["error"], Value::Null, "body: {body}");
    assert_eq!(body["result"], json!("true/true"));
}

// ---- Package API caller attribution --------------------------------------

/// A curated package whose exported `op()` asks for `test.com/op`: the check
/// must be attributed to `main`, because `submilli:security` is transparent
/// and the package-provided capability belongs to the package caller.
const SDK_SOURCE: &str = r#"
import { check } from "submilli:security";
/** @capability test.com/op { amount: number } */
export function op(): number { check("test.com/op", { amount: 100 }); return 1; }
"#;

const SDK_SCRIPT: &str = r#"
import { op } from "@acme/sdk";
function main(): number { return op(); }
"#;

fn write_sdk_package(store_root: &Path) {
    let package = compile_package(
        "@acme/sdk",
        ModulePath::from("lib"),
        &[PackageSourceModule {
            path: ModulePath::from("lib"),
            source: SDK_SOURCE,
        }],
        &[],
    )
    .expect("compile @acme/sdk");
    write_package_artifact(
        store_root.join("@acme").join("sdk"),
        &package.wasm,
        &package.type_info,
        &submilli_build::derive_capability_schema(
            &package.declaration,
            &[],
            &package.required_capabilities,
        ),
        &package.declaration,
        &ArtifactMetadata::new("@acme/sdk", "0.0.0-test", Vec::new()),
    )
    .expect("write @acme/sdk artifact");
}

async fn execute_with_packages(
    policy_yaml: &str,
    store_root: &Path,
    code: &str,
) -> (StatusCode, Value) {
    send(router(policy_yaml, Some(store_root)), code).await
}

#[tokio::test]
async fn package_api_check_uses_main_permission_block() {
    let store = tempfile::tempdir().expect("tempdir");
    write_sdk_package(store.path());
    let policy = "\
name: policy
default: deny
packages:
  - \"@acme/sdk\"
permissions:
  main:
    - capability: test.com/op
      action: allow
";
    let (status, body) = execute_with_packages(policy, store.path(), SDK_SCRIPT).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["result"], json!("1"));
    assert_eq!(body["error"], Value::Null);
}

#[tokio::test]
async fn package_permission_block_does_not_cover_package_api_checks() {
    // Same capability, but granted to `@acme/sdk` only: the package API check is
    // attributed to `main`, whose absent rule falls through to `default: deny`.
    let store = tempfile::tempdir().expect("tempdir");
    write_sdk_package(store.path());
    let policy = "\
name: policy
default: deny
packages:
  - \"@acme/sdk\"
permissions:
  \"@acme/sdk\":
    - capability: test.com/op
      action: allow
";
    let (status, body) = execute_with_packages(policy, store.path(), SDK_SCRIPT).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["result"], Value::Null);
    assert_eq!(body["error"]["kind"], json!("permission_denied"));
}

// ---- Structured denials (U3) ------------------------------------------------

const DENY_ALL: &str = "name: policy\ndefault: deny\n";

/// A `check` the policy refuses, from `main`.
const DENIED_CHECK: &str = r#"
import { check } from "submilli:security";
function main(): number { check("test.com/op", { amount: 100 }); return 1; }
"#;

fn assert_denied(body: &Value, caller: &str, capability: &str, source: &str) {
    let error = &body["error"];
    assert_eq!(error["kind"], json!("permission_denied"), "got: {body:#}");
    assert_eq!(error["caller"], json!(caller), "got: {body:#}");
    assert_eq!(error["capability"], json!(capability), "got: {body:#}");
    assert_eq!(error["source"], json!(source), "got: {body:#}");
    assert_eq!(body["result"], Value::Null, "got: {body:#}");
}

fn assert_runtime_error(body: &Value) {
    assert_eq!(
        body["error"]["kind"],
        json!("runtime_error"),
        "got: {body:#}"
    );
    assert!(body["error"]["capability"].is_null(), "got: {body:#}");
}

#[tokio::test]
async fn uncaught_policy_denial_is_a_permission_denied_error() {
    let (status, body) = execute(DENY_ALL, DENIED_CHECK).await;
    assert_eq!(status, StatusCode::OK);
    assert_denied(&body, "main", "test.com/op", "policy");
    let message = body["error"]["message"].as_str().unwrap_or_default();
    assert!(
        message.contains(
            "PermissionDeniedError: permission denied: caller=main capability=test.com/op"
        ),
        "the message keeps today's text: {body:#}"
    );
}

#[tokio::test]
async fn uncaught_invariant_denial_reports_the_invariant_source() {
    let policy = "\
name: policy
default: deny
permissions:
  main:
    - capability: secrets.get
      action: allow
";
    let script = r#"
import { get } from "submilli:secrets";
function main(): string | null { return get("TOKEN"); }
"#;
    let (status, body) = execute(policy, script).await;
    assert_eq!(status, StatusCode::OK);
    assert_denied(&body, "main", "secrets.get", "invariant");
}

#[tokio::test]
async fn a_caught_denial_returns_a_normal_result() {
    let script = r#"
import { check } from "submilli:security";
function main(): string {
    try { check("test.com/op", { amount: 100 }); return "allowed"; }
    catch (e: PermissionDeniedError) { return "caught " + e.capability; }
}
"#;
    let (status, body) = execute(DENY_ALL, script).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["error"], Value::Null, "got: {body:#}");
    assert_eq!(body["result"], json!("caught test.com/op"));
}

#[tokio::test]
async fn a_denial_rethrown_as_the_same_object_still_classifies() {
    let script = r#"
import { check } from "submilli:security";
function main(): number {
    try { check("test.com/op", { amount: 100 }); }
    catch (e: Error) { console.log("rethrowing"); throw e; }
    return 1;
}
"#;
    let (_, body) = execute(DENY_ALL, script).await;
    assert_denied(&body, "main", "test.com/op", "policy");
}

#[tokio::test]
async fn a_denial_through_a_non_matching_typed_catch_still_classifies() {
    let script = r#"
import { check } from "submilli:security";
function main(): number {
    try { check("test.com/op", { amount: 100 }); }
    catch (e: TypeError) { return 0; }
    return 1;
}
"#;
    let (_, body) = execute(DENY_ALL, script).await;
    assert_denied(&body, "main", "test.com/op", "policy");
}

/// The language has no `async`/`await`, so the nearest propagation path the
/// plan's rejected-promise scenario stands for is a denial crossing a host
/// function that re-enters guest code: the throw unwinds through the array
/// callback's host frame before it escapes.
#[tokio::test]
async fn a_denial_through_a_host_callback_frame_still_classifies() {
    let script = r#"
import { check } from "submilli:security";
function main(): number {
    const mapped = [1, 2, 3].map((n: number): number => {
        check("test.com/op", { amount: n });
        return n;
    });
    return mapped.length;
}
"#;
    let (_, body) = execute(DENY_ALL, script).await;
    assert_denied(&body, "main", "test.com/op", "policy");
}

#[tokio::test]
async fn a_program_built_permission_denied_error_is_a_runtime_error() {
    let script = r#"
function main(): number {
    throw new PermissionDeniedError("permission denied: forged", "main", "fs.read", "forged");
}
"#;
    let (_, body) = execute(DENY_ALL, script).await;
    assert_runtime_error(&body);
}

/// Catches a denial in a frame that then returns, so nothing holds it, and
/// allocates enough strings to push the engine into collecting before it builds
/// an error of its own. Run with the lookup instrumented, this reaches the
/// stale-entry path: the collected denial's entry fails its generation check.
const CATCH_DROP_COLLECT_FORGE: &str = r#"
import { check } from "submilli:security";
function churn(rounds: number): number {
    let total = 0;
    for (let i = 0; i < rounds; i++) {
        const chunk: string = "x".repeat(4000000 + i);
        total += chunk.length;
    }
    return total;
}
function attempt(): void {
    try { check("test.com/op", { amount: 100 }); }
    catch (e: PermissionDeniedError) { console.log("caught " + e.capability); }
}
function main(): number {
    attempt();
    churn(30);
    throw new PermissionDeniedError("permission denied: forged", "main", "test.com/op", "forged");
}
"#;

#[tokio::test]
async fn a_forged_denial_after_a_collection_is_a_runtime_error() {
    let (_, body) = execute(DENY_ALL, CATCH_DROP_COLLECT_FORGE).await;
    assert_runtime_error(&body);
}

#[tokio::test]
async fn an_unrelated_runtime_error_stays_a_runtime_error() {
    let script = "function main(): number { throw new Error(\"boom\"); }";
    let (_, body) = execute(DENY_ALL, script).await;
    assert_runtime_error(&body);
}

#[tokio::test]
async fn a_denial_from_a_top_level_statement_is_a_permission_denied_error() {
    let script = r#"
import { check } from "submilli:security";
check("test.com/op", { amount: 100 });
function main(): number { return 1; }
"#;
    let (_, body) = execute(DENY_ALL, script).await;
    assert_denied(&body, "main", "test.com/op", "policy");
}

fn write_denied_on_install_package(store_root: &Path) {
    let source = r#"
import { readText } from "submilli:fs";
export const first: string | null = readText("/never.txt");
"#;
    let package = compile_package(
        "@acme/eager",
        ModulePath::from("lib"),
        &[PackageSourceModule {
            path: ModulePath::from("lib"),
            source,
        }],
        &[],
    )
    .expect("compile @acme/eager");
    write_package_artifact(
        store_root.join("@acme").join("eager"),
        &package.wasm,
        &package.type_info,
        &submilli_build::derive_capability_schema(
            &package.declaration,
            &[],
            &package.required_capabilities,
        ),
        &package.declaration,
        &ArtifactMetadata::new("@acme/eager", "0.0.0-test", Vec::new()),
    )
    .expect("write @acme/eager artifact");
}

#[tokio::test]
async fn a_denial_from_a_package_top_level_statement_names_the_package() {
    let store = tempfile::tempdir().expect("tempdir");
    write_denied_on_install_package(store.path());
    let policy = "\
name: policy
default: deny
packages:
  - \"@acme/eager\"
";
    let script = r#"
import { first } from "@acme/eager";
function main(): string | null { return first; }
"#;
    let (status, body) = execute_with_packages(policy, store.path(), script).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["error"]["kind"],
        json!("permission_denied"),
        "got: {body:#}"
    );
    assert_eq!(
        body["error"]["caller"],
        json!("@acme/eager"),
        "got: {body:#}"
    );
    assert!(
        body["error"]["message"]
            .as_str()
            .is_some_and(|m| m.contains("package `@acme/eager` failed to initialize")),
        "got: {body:#}"
    );
}

/// The MCP execute tool shapes the same error as REST.
#[tokio::test]
async fn an_uncaught_policy_denial_over_mcp_is_a_permission_denied_error() {
    let blueprint = submilli_blueprint::parse(DENY_ALL).expect("valid policy blueprint");
    let blueprints = Arc::new(InMemoryBlueprintStore::seed([blueprint]).expect("seed blueprints"));
    let state = AppState::new(ServerConfig {
        blueprints: Some(blueprints),
        ..ServerConfig::default()
    })
    .await
    .expect("build AppState");

    let post = |body: Value, session: Option<String>| {
        let state = state.clone();
        async move {
            let mut builder = Request::builder()
                .method("POST")
                .uri(format!("/mcp/{BLUEPRINT_NAME}"))
                .header("host", "localhost")
                .header("content-type", "application/json")
                .header("accept", "application/json, text/event-stream");
            if let Some(session) = session {
                builder = builder.header("mcp-session-id", session);
            }
            let resp = app(state)
                .oneshot(builder.body(Body::from(body.to_string())).unwrap())
                .await
                .unwrap();
            let status = resp.status();
            let session = resp
                .headers()
                .get("mcp-session-id")
                .and_then(|v| v.to_str().ok())
                .map(str::to_string);
            let bytes = resp.into_body().collect().await.unwrap().to_bytes();
            let text = String::from_utf8_lossy(&bytes).into_owned();
            let data = text
                .lines()
                .filter_map(|line| line.strip_prefix("data:"))
                .next_back()
                .unwrap_or(&text)
                .trim()
                .to_string();
            (
                status,
                session,
                serde_json::from_str::<Value>(&data).unwrap_or(Value::Null),
            )
        }
    };

    let (status, session, _) = post(
        json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": { "name": "test", "version": "0" }
            }
        }),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let session = session.expect("initialize returns a session id");
    let (status, _, _) = post(
        json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
        Some(session.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);

    let (status, _, rpc) = post(
        json!({
            "jsonrpc": "2.0", "id": 2, "method": "tools/call",
            "params": {
                "name": "submilli__typescript__execute",
                "arguments": { "code": DENIED_CHECK }
            }
        }),
        Some(session),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let out = &rpc["result"]["structuredContent"];
    assert_eq!(
        out["error"]["kind"],
        json!("permission_denied"),
        "got: {rpc}"
    );
    assert_eq!(out["error"]["caller"], json!("main"), "got: {rpc}");
    assert_eq!(
        out["error"]["capability"],
        json!("test.com/op"),
        "got: {rpc}"
    );
    assert_eq!(out["error"]["source"], json!("policy"), "got: {rpc}");
}
