use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use interpreter::{ModulePath, PackageSourceModule, compile_package};
use serde_json::{Value, json};
use submilli_blueprint::{Blueprint, McpAuth, McpServer};
use submilli_build::{
    ArtifactDependency, ArtifactMetadata, ArtifactSource, write_package_artifact,
    write_package_artifact_with_docs_and_sources,
};
use submilli_server::blueprint::InMemoryBlueprintStore;
use submilli_server::{AppState, ServerConfig, app};
use tower::ServiceExt;
use uuid::Uuid;

const BLUEPRINT_NAME: &str = "test";

fn router() -> Router {
    let blueprints = Arc::new(InMemoryBlueprintStore::seed([Blueprint {
        name: BLUEPRINT_NAME.into(),
        ..Default::default()
    }]));
    let config = ServerConfig {
        blueprints: Some(blueprints),
        ..ServerConfig::default()
    };
    app(AppState::new(config).expect("build AppState"))
}

fn router_with_package_store(package_store_root: PathBuf) -> Router {
    router_with_packages(package_store_root, &["@acme/util"])
}

fn router_with_packages(package_store_root: PathBuf, packages: &[&str]) -> Router {
    let blueprints = Arc::new(InMemoryBlueprintStore::seed([Blueprint {
        name: BLUEPRINT_NAME.into(),
        packages: packages.iter().map(ToString::to_string).collect(),
        ..Default::default()
    }]));
    let config = ServerConfig {
        blueprints: Some(blueprints),
        package_store_root: Some(package_store_root),
        ..ServerConfig::default()
    };
    app(AppState::new(config).expect("build AppState"))
}

fn router_with_oauth_mcp() -> Router {
    let blueprints = Arc::new(InMemoryBlueprintStore::seed([Blueprint {
        name: BLUEPRINT_NAME.into(),
        mcp: BTreeMap::from([(
            "github".to_string(),
            McpServer {
                transport: "streamable_http".into(),
                url: "https://api.githubcopilot.com/mcp/".into(),
                headers: BTreeMap::new(),
                auth: Some(McpAuth::Oauth2 {
                    client_id: None,
                    authorization_endpoint: None,
                    token_endpoint: None,
                    scopes: Vec::new(),
                }),
            },
        )]),
        ..Default::default()
    }]));
    let config = ServerConfig {
        blueprints: Some(blueprints),
        ..ServerConfig::default()
    };
    app(AppState::new(config).expect("build AppState"))
}

async fn execute(code: &str) -> (StatusCode, Value) {
    post_body(json!({ "code": code, "blueprint": BLUEPRINT_NAME }).to_string()).await
}

async fn post_body(body: String) -> (StatusCode, Value) {
    let req = Request::builder()
        .method("POST")
        .uri("/v1/execute")
        .header("content-type", "application/json")
        .body(Body::from(body))
        .unwrap();
    let resp = router().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let parsed: Value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, parsed)
}

async fn execute_on(router: &Router, code: &str) -> (StatusCode, Value) {
    let req = Request::builder()
        .method("POST")
        .uri("/v1/execute")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({ "code": code, "blueprint": BLUEPRINT_NAME }).to_string(),
        ))
        .unwrap();
    let resp = router.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let parsed = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, parsed)
}

async fn apply_blueprint_on(router: &Router, yaml: &str) -> (StatusCode, Value) {
    let req = Request::builder()
        .method("PUT")
        .uri(format!("/v1/blueprints/{BLUEPRINT_NAME}"))
        .header("content-type", "application/json")
        .body(Body::from(json!({ "yaml": yaml }).to_string()))
        .unwrap();
    let resp = router.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let parsed = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, parsed)
}

fn write_acme_util_package(store_root: &Path) {
    const SOURCE: &str = "export function answer(): number { return 41; }\nexport function plusOne(n: number): number { return n + 1; }\nexport function explode(): void { assert(false, \"pkg boom\"); }";
    let package = compile_package(
        "@acme/util",
        ModulePath::from("lib"),
        &[PackageSourceModule {
            path: ModulePath::from("lib"),
            source: SOURCE,
        }],
        &[],
    )
    .expect("compile synthetic package");
    let dir = store_root.join("@acme").join("util");
    write_package_artifact_with_docs_and_sources(
        &dir,
        &package.wasm,
        &package.type_info,
        &submilli_build::derive_capability_schema(
            &package.declaration,
            &package.required_capabilities,
        ),
        &package.declaration,
        &ArtifactMetadata::new("@acme/util", "0.0.0-test", Vec::new()),
        "",
        &[ArtifactSource {
            path: ModulePath::from("lib"),
            text: SOURCE.to_string(),
        }],
    )
    .expect("write synthetic package artifact");
}

// `@a/app` depends on `@z/util` — names chosen so alphabetical order is wrong
// and only metadata-driven topological linking instantiates util first.
fn write_dependent_packages(store_root: &Path) {
    let util = compile_package(
        "@z/util",
        ModulePath::from("lib"),
        &[PackageSourceModule {
            path: ModulePath::from("lib"),
            source: "export function forty(): number { return 40; }",
        }],
        &[],
    )
    .expect("compile util package");
    write_package_artifact(
        store_root.join("@z").join("util"),
        &util.wasm,
        &util.type_info,
        &submilli_build::derive_capability_schema(&util.declaration, &util.required_capabilities),
        &util.declaration,
        &ArtifactMetadata::new("@z/util", "1.0.0", Vec::new()),
    )
    .expect("write util artifact");

    let app = compile_package(
        "@a/app",
        ModulePath::from("lib"),
        &[PackageSourceModule {
            path: ModulePath::from("lib"),
            source: "import { forty } from \"@z/util\";\nexport function fortyTwo(): number { return forty() + 2; }",
        }],
        &[&util.declaration],
    )
    .expect("compile app package");
    write_package_artifact(
        store_root.join("@a").join("app"),
        &app.wasm,
        &app.type_info,
        &submilli_build::derive_capability_schema(&app.declaration, &app.required_capabilities),
        &app.declaration,
        &ArtifactMetadata::new(
            "@a/app",
            "1.0.0",
            vec![ArtifactDependency::new("@z/util", "1.0.0")],
        ),
    )
    .expect("write app artifact");
}

#[tokio::test]
async fn string_return_success() {
    let (status, body) = execute(r#"function main(): string { return "hello"; }"#).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["result"], json!("hello"));
    assert_eq!(body["console"], json!([]));
    assert_eq!(body["error"], Value::Null);
}

#[tokio::test]
async fn execute_without_import_skips_package_preparation() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let store_root = tmp.path().join("packages");
    let router = router_with_packages(store_root, &["@missing/pkg"]);

    let (status, body) = execute_on(
        &router,
        r#"function main(): string { return Temporal.Now.instant().toString(); }"#,
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert!(body["result"].as_str().is_some(), "got: {body:#}");
    assert_eq!(body["error"], Value::Null, "got: {body:#}");
}

#[tokio::test]
async fn execute_without_mcp_import_skips_mcp_discovery() {
    let router = router_with_oauth_mcp();

    let (status, body) = execute_on(&router, r#"function main(): string { return "ok"; }"#).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["result"], json!("ok"), "got: {body:#}");
    assert_eq!(body["error"], Value::Null, "got: {body:#}");
    assert!(
        body.get("discovery_warnings").is_none(),
        "MCP discovery should not run for code without @mcp imports: {body:#}"
    );
}

#[tokio::test]
async fn blueprint_package_dependency_runs() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let store_root = tmp.path().join("packages");
    write_acme_util_package(&store_root);
    let router = router_with_package_store(store_root);
    let code = r#"
        import { answer, plusOne } from "@acme/util";
        function main(): number { return plusOne(answer()); }
    "#;

    let (status, body) = execute_on(&router, code).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["result"], json!("42"), "got: {body:#}");
    assert_eq!(body["error"], Value::Null);
}

#[tokio::test]
async fn blueprint_package_resolves_from_the_fallback_store() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let owned = tmp.path().join("server-packages");
    let fallback = tmp.path().join("cli-packages");
    write_acme_util_package(&fallback);
    let blueprints = Arc::new(InMemoryBlueprintStore::seed([Blueprint {
        name: BLUEPRINT_NAME.into(),
        packages: ["@acme/util".to_string()].into_iter().collect(),
        ..Default::default()
    }]));
    let router = app(AppState::new(ServerConfig {
        blueprints: Some(blueprints),
        package_store_root: Some(owned.clone()),
        package_fallback_root: Some(fallback),
        ..ServerConfig::default()
    })
    .expect("build AppState"));
    let code = r#"
        import { answer, plusOne } from "@acme/util";
        function main(): number { return plusOne(answer()); }
    "#;

    let (status, body) = execute_on(&router, code).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["result"], json!("42"), "got: {body:#}");
    assert_eq!(body["error"], Value::Null);
    assert!(
        !owned.exists(),
        "reading through the fallback writes nothing"
    );
}

#[tokio::test]
async fn package_error_renders_package_source_context() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let store_root = tmp.path().join("packages");
    write_acme_util_package(&store_root);
    let router = router_with_package_store(store_root);
    let code = r#"
        import { explode } from "@acme/util";
        function main(): void { explode(); }
    "#;

    let (status, body) = execute_on(&router, code).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["result"], Value::Null);
    let message = body["error"]["message"].as_str().unwrap_or_default();
    assert!(message.contains("error: Error: pkg boom"), "got: {body:#}");
    assert!(message.contains("lib:"), "missing package frame: {body:#}");
    assert!(
        message.contains("export function explode(): void"),
        "missing package source: {body:#}",
    );
    assert!(message.contains("^"), "missing caret: {body:#}");
}

#[tokio::test]
async fn package_dependency_closure_links_in_topological_order() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let store_root = tmp.path().join("packages");
    write_dependent_packages(&store_root);
    // The blueprint lists only the root; `@z/util` loads transitively from
    // artifact metadata and must be instantiated before `@a/app`.
    let router = router_with_packages(store_root, &["@a/app"]);
    let code = r#"
        import { fortyTwo } from "@a/app";
        function main(): number { return fortyTwo(); }
    "#;

    let (status, body) = execute_on(&router, code).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["result"], json!("42"), "got: {body:#}");
    assert_eq!(body["error"], Value::Null);
}

#[tokio::test]
async fn closure_only_dependency_is_not_importable() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let store_root = tmp.path().join("packages");
    write_dependent_packages(&store_root);
    let router = router_with_packages(store_root, &["@a/app"]);
    let code = r#"
        import { forty } from "@z/util";
        function main(): number { return forty(); }
    "#;

    let (_, body) = execute_on(&router, code).await;

    assert_ne!(body["error"], Value::Null, "got: {body:#}");
    let message = body["error"]["message"].as_str().unwrap_or_default();
    assert!(message.contains("@z/util"), "got: {body:#}");
}

#[tokio::test]
async fn prepared_package_cache_survives_store_removal() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let store_root = tmp.path().join("packages");
    write_acme_util_package(&store_root);
    let artifact_wasm = store_root.join("@acme").join("util").join("pkg.wasm");
    let router = router_with_package_store(store_root);
    let code = r#"
        import { answer, plusOne } from "@acme/util";
        function main(): number { return plusOne(answer()); }
    "#;

    let (_, first) = execute_on(&router, code).await;
    std::fs::remove_file(artifact_wasm).expect("remove package wasm after prepare");
    let (_, second) = execute_on(&router, code).await;

    assert_eq!(first["result"], json!("42"), "got: {first:#}");
    assert_eq!(second["result"], json!("42"), "got: {second:#}");
}

#[tokio::test]
async fn prepared_package_cache_only_evicts_when_blueprint_packages_change() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let store_root = tmp.path().join("packages");
    write_acme_util_package(&store_root);
    let artifact_wasm = store_root.join("@acme").join("util").join("pkg.wasm");
    let router = router_with_package_store(store_root);
    let code = r#"
        import { answer, plusOne } from "@acme/util";
        function main(): number { return plusOne(answer()); }
    "#;

    let (_, first) = execute_on(&router, code).await;
    std::fs::remove_file(artifact_wasm).expect("remove package wasm after prepare");

    let (status, _) = apply_blueprint_on(
        &router,
        r#"
name: test
packages:
  - "@acme/util"
vfs: none
"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, after_non_package_update) = execute_on(&router, code).await;

    let (status, _) = apply_blueprint_on(
        &router,
        r#"
name: test
packages:
  - "@acme/util"
  - "@acme/other"
vfs: none
"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, after_package_update) = execute_on(&router, code).await;

    assert_eq!(first["result"], json!("42"), "got: {first:#}");
    assert_eq!(
        after_non_package_update["result"],
        json!("42"),
        "got: {after_non_package_update:#}"
    );
    assert_eq!(
        after_package_update["error"]["kind"],
        json!("package_resolution"),
        "got: {after_package_update:#}"
    );
}

#[tokio::test]
async fn uninstalling_a_package_takes_effect_on_the_next_execute() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let store_root = tmp.path().join("packages");
    write_acme_util_package(&store_root);
    let router = router_with_package_store(store_root);
    let code = r#"
        import { answer } from "@acme/util";
        function main(): number { return answer(); }
    "#;
    let (_, before) = execute_on(&router, code).await;
    assert_eq!(before["result"], json!("41"), "got: {before:#}");

    let req = Request::builder()
        .method("DELETE")
        .uri("/v1/packages/@acme/util")
        .body(Body::empty())
        .unwrap();
    let resp = router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let (status, after) = execute_on(&router, code).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        after["error"]["kind"],
        json!("package_resolution"),
        "the cached module set must not outlive the package: {after:#}"
    );
}

#[tokio::test]
async fn missing_blueprint_package_returns_package_resolution_error() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let router = router_with_package_store(tmp.path().join("packages"));
    let code = r#"
        import { answer } from "@acme/util";
        function main(): number { return answer(); }
    "#;

    let (status, body) = execute_on(&router, code).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["error"]["kind"], json!("package_resolution"));
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("@acme/util"),
        "got: {body:#}"
    );
}

#[tokio::test]
async fn number_return_success() {
    let (status, body) = execute(r#"function main(): number { return 42; }"#).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["result"], json!("42"));
    assert_eq!(body["error"], Value::Null);
}

#[tokio::test]
async fn boolean_return_success() {
    let (status, body) = execute(r#"function main(): boolean { return true; }"#).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["result"], json!("true"));
    assert_eq!(body["error"], Value::Null);
}

#[tokio::test]
async fn void_main_returns_null_result() {
    let (status, body) =
        execute(r#"function main(): void { console.log("hello"); console.log("world"); }"#).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["result"], Value::Null);
    assert_eq!(body["console"], json!([]));
    assert_eq!(body["error"], Value::Null);
}

#[tokio::test]
async fn void_main_empty_stdio() {
    let (status, body) = execute(r#"function main(): void { }"#).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["result"], Value::Null);
    assert_eq!(body["console"], json!([]));
    assert_eq!(body["error"], Value::Null);
}

#[tokio::test]
async fn console_suppressed_on_success_nonvoid() {
    let (status, body) =
        execute(r#"function main(): number { console.log("debug"); return 7; }"#).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["result"], json!("7"));
    assert_eq!(body["console"], json!([]));
    assert_eq!(body["error"], Value::Null);
}

#[tokio::test]
async fn compile_error_returns_diagnostics() {
    let (status, body) = execute(r#"function main(): string { return 42; }"#).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["result"], Value::Null);
    assert_eq!(body["console"], json!([]));
    assert_eq!(body["error"]["kind"], json!("compile_error"));
    let diags = body["error"]["diagnostics"].as_array().unwrap();
    assert!(!diags.is_empty(), "expected at least one diagnostic");
    assert_eq!(diags[0]["line"], json!(1));
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("error:")
    );
}

#[tokio::test]
async fn unresolved_import_does_not_claim_a_complete_catalog() {
    let (_, body) = execute(r#"import fs from "node:fs"; function main(): void {}"#).await;
    let message = body["error"]["message"].as_str().expect("compile error");
    assert!(message.contains("not the full catalog"), "{message}");
    assert!(message.contains("package-discovery tool"), "{message}");
    assert!(!message.contains("help: available packages:"), "{message}");
}

#[tokio::test]
async fn compile_error_syntax() {
    let (status, body) = execute("let x = ;").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["error"]["kind"], json!("compile_error"));
}

#[tokio::test]
async fn runtime_trap_includes_console() {
    let (status, body) =
        execute(r#"function main(): void { console.log("before"); assert(false, "boom"); }"#).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["result"], Value::Null);
    assert_eq!(body["error"]["kind"], json!("runtime_error"));
    assert_eq!(body["console"], json!(["before"]));
}

#[tokio::test]
async fn runtime_trap_no_console() {
    let (status, body) = execute(r#"function main(): void { assert(false, "boom"); }"#).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["error"]["kind"], json!("runtime_error"));
    assert_eq!(body["console"], json!([]));
}

#[tokio::test]
async fn uncaught_throw_returns_runtime_error() {
    let (status, body) =
        execute(r#"function main(): void { throw new Error("boom from throw"); }"#).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["error"]["kind"],
        json!("runtime_error"),
        "got: {body:#}"
    );
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("boom from throw"),
        "got: {body:#}"
    );
}

#[tokio::test]
async fn uncaught_throw_in_nonvoid_main_returns_runtime_error() {
    let (status, body) =
        execute(r#"function main(): number { throw new Error("boom nonvoid"); }"#).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["result"], Value::Null);
    assert_eq!(
        body["error"]["kind"],
        json!("runtime_error"),
        "got: {body:#}"
    );
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("boom nonvoid"),
        "got: {body:#}"
    );
}

#[tokio::test]
async fn multiple_console_lines_on_failure() {
    let (status, body) = execute(
        r#"function main(): void { console.log("a"); console.log("b"); assert(false, "x"); }"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["console"], json!(["a", "b"]));
}

#[tokio::test]
async fn extra_field_ignored() {
    let body = json!({
        "code": "function main(): number { return 1; }",
        "blueprint": BLUEPRINT_NAME,
        "unknown": 5,
    });
    let (status, response) = post_body(body.to_string()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(response["result"], json!("1"));
}

#[tokio::test]
async fn missing_code_field() {
    let (status, _) = post_body("{}".to_string()).await;
    assert_ne!(status, StatusCode::OK);
}

#[tokio::test]
async fn missing_blueprint_field() {
    let body = json!({ "code": "function main(): number { return 1; }" });
    let (status, _) = post_body(body.to_string()).await;
    assert_ne!(status, StatusCode::OK);
}

#[tokio::test]
async fn unknown_blueprint_rejected() {
    let body = json!({
        "code": "function main(): number { return 1; }",
        "blueprint": "does-not-exist",
    });
    let (status, response) = post_body(body.to_string()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(response["error"]["kind"], json!("blueprint_not_found"));
    assert!(
        response["error"]["message"]
            .as_str()
            .unwrap()
            .contains("does-not-exist")
    );
    assert_eq!(response["result"], Value::Null);
}

#[tokio::test]
async fn malformed_json() {
    let (status, _) = post_body("{".to_string()).await;
    assert_ne!(status, StatusCode::OK);
}

#[tokio::test]
async fn session_id_generated_when_omitted() {
    let (status, body) = execute(r#"function main(): number { return 1; }"#).await;
    assert_eq!(status, StatusCode::OK);
    let id = body["session_id"].as_str().expect("session_id present");
    Uuid::parse_str(id).expect("session_id is a valid UUID");
}

#[tokio::test]
async fn git_configuration_controls_imports_and_resolves_identity_variables() {
    let router = router();
    let code = r#"
        import { Repository } from "submilli:git";
        import * as fs from "submilli:fs";
        function main(): string {
            const repo = Repository.init("/repo");
            fs.writeText("/repo/note", "note");
            repo.add(["note"]);
            repo.commit("note");
            return repo.log().commits[0].authorName;
        }
    "#;
    let (_, disabled) = execute_on(&router, code).await;
    assert!(!disabled["error"].is_null(), "{disabled}");
    let yaml = "name: test\ndefault: allow\nvariables:\n  author: {default: Support}\ngit:\n  identity:\n    name: '${vars.author}'\n    email: agent@example.com\n";
    let (status, applied) = apply_blueprint_on(&router, yaml).await;
    assert_eq!(status, StatusCode::OK, "{applied}");
    let (_, enabled) = execute_on(&router, code).await;
    assert!(enabled["error"].is_null(), "{enabled}");
    assert_eq!(enabled["result"], "Support");
    apply_blueprint_on(&router, "name: test\ndefault: allow\n").await;
    let (_, disabled_again) = execute_on(&router, code).await;
    assert!(!disabled_again["error"].is_null(), "{disabled_again}");
}

#[tokio::test]
async fn git_transitive_imports_require_configuration_and_keep_package_attribution() {
    let directory = tempfile::tempdir().unwrap();
    let package = compile_package(
        "@acme/util",
        ModulePath::from("lib"),
        &[PackageSourceModule {
            path: ModulePath::from("lib"),
            source: "import { Repository } from \"submilli:git\"; export function answer(): string { const repo = Repository.init(\"/repo\"); return repo.commit(\"initial\"); }",
        }],
        &[],
    ).unwrap();
    write_package_artifact(
        directory.path().join("@acme/util"),
        &package.wasm,
        &package.type_info,
        &submilli_build::derive_capability_schema(
            &package.declaration,
            &package.required_capabilities,
        ),
        &package.declaration,
        &ArtifactMetadata::new("@acme/util", "1.0.0", vec![]),
    )
    .unwrap();
    let router = router_with_package_store(directory.path().to_owned());
    let code =
        "import { answer } from \"@acme/util\"; function main(): string { return answer(); }";
    let (_, disabled) = execute_on(&router, code).await;
    assert!(
        disabled["error"]["message"]
            .as_str()
            .unwrap()
            .contains("blueprint git block"),
        "{disabled}"
    );
    let yaml = "name: test\npackages: ['@acme/util']\ngit:\n  identity: {name: Agent, email: agent@example.com}\npermissions:\n  main:\n    - {capability: git.init, action: allow}\n";
    let (status, applied) = apply_blueprint_on(&router, yaml).await;
    assert_eq!(status, StatusCode::OK, "{applied}");
    let (_, denied) = execute_on(&router, code).await;
    let message = denied["error"]["message"].as_str().unwrap();
    assert!(
        message.contains("caller=@acme/util") && message.contains("git.init"),
        "{denied}"
    );
    let yaml = "name: test\npackages: ['@acme/util']\ngit:\n  identity: {name: Agent, email: agent@example.com}\npermissions:\n  main:\n    - {capability: git.commit, action: allow}\n  '@acme/util':\n    - {capability: git.init, action: allow}\n";
    apply_blueprint_on(&router, yaml).await;
    let (_, denied) = execute_on(&router, code).await;
    let message = denied["error"]["message"].as_str().unwrap();
    assert!(
        message.contains("caller=@acme/util") && message.contains("git.commit"),
        "{denied}"
    );
}

#[tokio::test]
async fn configured_execution_timeout_interrupts_loop_without_expiring_early() {
    use std::time::{Duration, Instant};
    let blueprints = Arc::new(InMemoryBlueprintStore::seed([Blueprint {
        name: BLUEPRINT_NAME.into(),
        ..Default::default()
    }]));
    let router = app(AppState::new(ServerConfig {
        blueprints: Some(blueprints),
        runtime: submilli_server::RuntimeConfig {
            timeout: Some(Duration::from_secs(1)),
            ..Default::default()
        },
        ..Default::default()
    })
    .unwrap());
    let started = Instant::now();
    let (_, body) = tokio::time::timeout(
        Duration::from_secs(15),
        execute_on(&router, "function main(): void { while (true) {} }"),
    )
    .await
    .expect("loop must be interrupted");
    assert!(started.elapsed() >= Duration::from_secs(1));
    assert_eq!(body["error"]["kind"], "timeout", "{body}");
    let (_, body) = execute_on(&router, "function main(): number { return 42; }").await;
    assert_eq!(body["result"], "42", "{body}");
}

#[path = "common/parser_depth.rs"]
mod parser_depth;

#[test]
fn http_parser_depth_is_bounded() {
    parser_depth::isolated_worker("http_parser_depth_is_bounded", async {
        let app = router();
        for (excessive, code) in [
            (true, parser_depth::nested_source()),
            (false, "function main(): number { return 42; }".into()),
        ] {
            let (status, body) = execute_on(&app, &code).await;
            assert_eq!(status, StatusCode::OK);
            if excessive {
                assert_eq!(body["error"]["kind"], "compile_error", "{body}");
                assert!(
                    body.to_string().contains("parser recursion limit exceeded"),
                    "{body}"
                );
            } else {
                assert!(body["error"].is_null(), "{body}");
                assert_eq!(body["result"], "42", "{body}");
            }
        }
    });
}

#[test]
fn http_closure_arity_returns_diagnostics() {
    parser_depth::isolated_worker("http_closure_arity_returns_diagnostics", async {
        let app = router();
        let (status, body) = execute_on(&app, &parser_depth::oversized_closure_source()).await;
        assert_eq!(status, StatusCode::OK);
        parser_depth::assert_closure_diagnostic(&body);
        let (status, body) = execute_on(&app, "function main(): number { return 42; }").await;
        assert_eq!(status, StatusCode::OK);
        assert!(body["error"].is_null(), "{body}");
        assert_eq!(body["result"], "42", "{body}");
    });
}
