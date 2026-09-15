//! End-to-end coverage of the `permissions:` policy through `/v1/execute`: a
//! script calls `submilli:security.check`, and the blueprint's rules decide
//! whether it runs to completion or the check traps.

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
    let blueprints = Arc::new(InMemoryBlueprintStore::seed([blueprint]));
    let config = ServerConfig {
        blueprints: Some(blueprints),
        package_store_root: package_store_root.map(Path::to_path_buf),
        ..ServerConfig::default()
    };
    app(AppState::new(config).expect("build AppState"))
}

async fn send(router: Router, code: &str) -> (StatusCode, Value) {
    let body = json!({ "code": code, "blueprint": BLUEPRINT_NAME }).to_string();
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
    assert_eq!(body["error"]["kind"], json!("runtime_error"));
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
    assert_eq!(body["error"]["kind"], json!("runtime_error"));
}

#[tokio::test]
async fn no_policy_denies_by_default() {
    // The server is always deny-by-default: a policy-free blueprint (no
    // `permissions:`, no `default:`) denies every capability, so the check traps.
    let (status, body) = execute("name: policy\n", CHECK_SCRIPT).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["result"], Value::Null);
    assert_eq!(body["error"]["kind"], json!("runtime_error"));
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
    let tmp = tempfile::tempdir().expect("tempdir");
    let secret_path = tmp.path().join("token.txt");
    std::fs::write(&secret_path, "tok-file-123\n").expect("write secret");
    let policy = format!(
        "\
name: policy
default: deny
secrets:
  TOKEN:
    file: {}
permissions:
  main:
    - capability: secrets.get
      filter: name == \"TOKEN\"
      action: allow
",
        secret_path.display()
    );
    let script = r#"
import { get } from "submilli:secrets";
function main(): string | null { return get("TOKEN"); }
"#;

    let (status, body) = execute(&policy, script).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["result"], Value::Null);
    assert_eq!(body["error"]["kind"], json!("runtime_error"));
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
    assert_eq!(body["error"]["kind"], json!("runtime_error"));
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
            &package.required_capabilities,
        ),
        &package.declaration,
        &ArtifactMetadata::new("@acme/secrets", "0.0.0-test", Vec::new()),
    )
    .expect("write @acme/secrets artifact");
}

#[tokio::test]
async fn a_package_still_resolves_a_declared_secret_and_gets_null_for_an_undeclared_one() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let secret_path = tmp.path().join("token.txt");
    std::fs::write(&secret_path, "tok-file-123\n").expect("write secret");
    let store = tempfile::tempdir().expect("store tempdir");
    write_secrets_package(store.path());
    let policy = format!(
        "\
name: policy
default: deny
packages:
  - \"@acme/secrets\"
secrets:
  TOKEN:
    file: {}
permissions:
  \"@acme/secrets\":
    - capability: secrets.get
      action: allow
",
        secret_path.display()
    );
    let script = r#"
import { secretMatches, secretIsAbsent } from "@acme/secrets";
function main(): string {
    const found = secretMatches("TOKEN", "tok-file-123");
    const absent = secretIsAbsent("MISSING");
    return found.toString() + "/" + absent.toString();
}
"#;

    let (status, body) = execute_with_packages(&policy, store.path(), script).await;
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
    assert_eq!(body["error"]["kind"], json!("runtime_error"));
}
