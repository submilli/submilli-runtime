//! `GET /v1/capabilities` catalog tests, in-process via `oneshot`.
//!
//! Package groups depend on what's installed in the local store, so assertions
//! stick to the stdlib groups, which come from the interpreter catalog.

#[path = "common/in_memory_config.rs"]
mod in_memory_config;

use std::collections::BTreeMap;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use submilli_build::{
    ArtifactMetadata, CapabilitySchema, PackageStore, ProvidedCapability, ProvidedField,
    RequiredCapability, write_package_artifact_with_docs,
};
use submilli_server::{AppState, ServerConfig, app};
use tower::ServiceExt;

async fn get(uri: &str) -> (StatusCode, Value) {
    let state = AppState::new(in_memory_config::config()).expect("AppState");
    get_with_state(state, uri).await
}

async fn get_with_state(state: AppState, uri: &str) -> (StatusCode, Value) {
    let req = Request::builder()
        .method("GET")
        .uri(uri)
        .body(Body::empty())
        .unwrap();
    let resp = app(state).oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, body)
}

fn stdlib_group<'a>(body: &'a Value, source: &str) -> &'a Value {
    body["groups"]
        .as_array()
        .unwrap()
        .iter()
        .find(|g| g["source"] == source && g["kind"] == "stdlib")
        .unwrap_or_else(|| panic!("no stdlib group '{source}' in: {body}"))
}

#[tokio::test]
async fn serves_the_stdlib_catalog() {
    let (status, body) = get("/v1/capabilities").await;
    assert_eq!(status, StatusCode::OK);

    let fs = stdlib_group(&body, "submilli:fs");
    let read = fs["capabilities"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == "fs.read")
        .expect("fs.read listed");
    assert!(!read["summary"].as_str().unwrap().is_empty());
    assert!(!read["example_filter"].as_str().unwrap().is_empty());
    let fields: Vec<&str> = read["filter_fields"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f.as_str().unwrap())
        .collect();
    assert!(fields.contains(&"path"), "got: {fields:?}");

    let path = &read["fields"]["path"];
    assert_eq!(path["type"], "string", "got: {read}");
    assert!(
        !path["description"].as_str().unwrap().is_empty(),
        "got: {read}"
    );
}

/// The console renders `main_denial` as a note beside the entry, so the entry
/// must still be served — annotate, never filter. Dropping it would lose
/// discovery of the package grant the capability still supports.
#[tokio::test]
async fn serves_the_main_denial_marker_without_dropping_the_entry() {
    let (_, body) = get("/v1/capabilities").await;

    let secrets = stdlib_group(&body, "submilli:secrets");
    let get_secret = secrets["capabilities"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == "secrets.get")
        .expect("secrets.get listed");
    assert!(
        get_secret["main_denial"]
            .as_str()
            .is_some_and(|reason| reason.contains("secret NAME")),
        "got: {get_secret}"
    );

    let fs = stdlib_group(&body, "submilli:fs");
    let read = fs["capabilities"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == "fs.read")
        .expect("fs.read listed");
    assert_eq!(read["main_denial"], Value::Null, "got: {read}");
}

#[tokio::test]
async fn includes_the_mcp_template_group() {
    let (_, body) = get("/v1/capabilities").await;
    let mcp = stdlib_group(&body, "@mcp");
    assert_eq!(mcp["capabilities"][0]["name"], "mcp.<server>");
}

fn installed_package(store_root: &std::path::Path, name: &str, schema: &CapabilitySchema) {
    let store = PackageStore::new(store_root);
    let dir = store.package_dir(name).expect("package dir");
    let metadata =
        ArtifactMetadata::with_description(name, "0.1.0", "test package", Vec::new(), Vec::new());
    write_package_artifact_with_docs(
        dir,
        &[],
        &interpreter::TypeInfoTable {
            package_name: name.to_string(),
            types: Vec::new(),
        },
        schema,
        &interpreter::PackageDeclaration::with_package(name),
        &metadata,
        "# test package\n",
    )
    .expect("write package artifact");
}

fn package_group<'a>(body: &'a Value, source: &str) -> &'a Value {
    body["groups"]
        .as_array()
        .unwrap()
        .iter()
        .find(|g| g["source"] == source && g["kind"] == "package")
        .unwrap_or_else(|| panic!("no package group '{source}' in: {body}"))
}

#[tokio::test]
async fn package_groups_carry_provides_and_requires() {
    let root = tempfile::tempdir().expect("package store root");
    installed_package(
        root.path(),
        "@acme/tool",
        &CapabilitySchema {
            namespace: "tool.acme.dev".into(),
            provides: vec![ProvidedCapability {
                name: "tool.acme.dev/run".into(),
                description: Some("Run a tool".into()),
                fields: BTreeMap::from([(
                    "id".into(),
                    ProvidedField {
                        ty: "string".into(),
                        description: Some("target id".into()),
                    },
                )]),
            }],
            requires: vec![RequiredCapability {
                capability: "http.post".into(),
                filter: Some("url.host == \"api.acme.dev\"".into()),
            }],
        },
    );
    let state = AppState::new(ServerConfig {
        package_store_root: Some(root.path().to_path_buf()),
        ..in_memory_config::config()
    })
    .expect("AppState");

    let (status, body) = get_with_state(state, "/v1/capabilities").await;
    assert_eq!(status, StatusCode::OK);
    let group = package_group(&body, "@acme/tool");
    assert_eq!(group["capabilities"][0]["name"], "tool.acme.dev/run");
    assert_eq!(group["capabilities"][0]["fields"]["id"]["type"], "string");
    assert_eq!(
        group["requires"],
        json!([{ "capability": "http.post", "filter": "url.host == \"api.acme.dev\"" }])
    );
}

#[tokio::test]
async fn requires_only_package_is_listed() {
    let root = tempfile::tempdir().expect("package store root");
    installed_package(
        root.path(),
        "@acme/quiet",
        &CapabilitySchema {
            namespace: "quiet.acme.dev".into(),
            provides: Vec::new(),
            requires: vec![RequiredCapability {
                capability: "http.get".into(),
                filter: None,
            }],
        },
    );
    let state = AppState::new(ServerConfig {
        package_store_root: Some(root.path().to_path_buf()),
        ..in_memory_config::config()
    })
    .expect("AppState");

    let (_, body) = get_with_state(state, "/v1/capabilities").await;
    let group = package_group(&body, "@acme/quiet");
    assert_eq!(group["capabilities"], json!([]));
    assert_eq!(
        group["requires"],
        json!([{ "capability": "http.get", "filter": null }])
    );
}

#[tokio::test]
async fn installed_packages_are_listed() {
    let root = tempfile::tempdir().expect("package store root");
    installed_package(root.path(), "@acme/tool", &CapabilitySchema::default());
    let state = AppState::new(ServerConfig {
        package_store_root: Some(root.path().to_path_buf()),
        ..in_memory_config::config()
    })
    .expect("AppState");

    let (status, body) = get_with_state(state, "/v1/packages").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["roots"], json!([root.path().display().to_string()]));
    assert_eq!(
        body["packages"],
        json!([{
            "name": "@acme/tool",
            "version": "0.1.0",
            "description": "test package",
            "root": root.path().display().to_string(),
            "managed": true,
        }])
    );
}

#[tokio::test]
async fn fallback_packages_are_listed_as_unmanaged() {
    let owned = tempfile::tempdir().expect("owned root");
    let fallback = tempfile::tempdir().expect("fallback root");
    installed_package(fallback.path(), "@acme/tool", &CapabilitySchema::default());
    let state = AppState::new(ServerConfig {
        package_store_root: Some(owned.path().to_path_buf()),
        package_fallback_root: Some(fallback.path().to_path_buf()),
        ..in_memory_config::config()
    })
    .expect("AppState");

    let (status, body) = get_with_state(state, "/v1/packages").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["roots"],
        json!([
            owned.path().display().to_string(),
            fallback.path().display().to_string()
        ])
    );
    assert_eq!(body["packages"][0]["name"], json!("@acme/tool"));
    assert_eq!(
        body["packages"][0]["root"],
        json!(fallback.path().display().to_string())
    );
    assert_eq!(body["packages"][0]["managed"], json!(false));
}

#[tokio::test]
async fn uninstall_refuses_a_package_that_only_the_fallback_holds() {
    let owned = tempfile::tempdir().expect("owned root");
    let fallback = tempfile::tempdir().expect("fallback root");
    installed_package(fallback.path(), "@acme/tool", &CapabilitySchema::default());
    let state = AppState::new(ServerConfig {
        package_store_root: Some(owned.path().to_path_buf()),
        package_fallback_root: Some(fallback.path().to_path_buf()),
        ..in_memory_config::config()
    })
    .expect("AppState");
    let router = app(state.clone());

    let req = Request::builder()
        .method("DELETE")
        .uri("/v1/packages/@acme/tool")
        .body(Body::empty())
        .unwrap();
    let resp = router.oneshot(req).await.unwrap();

    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["error"], json!("not_managed"));
    assert!(
        fallback.path().join("@acme").join("tool").is_dir(),
        "the fallback copy is left in place"
    );
    let (_, listing) = get_with_state(state, "/v1/packages").await;
    assert_eq!(listing["packages"][0]["name"], json!("@acme/tool"));
}

#[tokio::test]
async fn uninstall_removes_a_package() {
    let root = tempfile::tempdir().expect("package store root");
    installed_package(root.path(), "@acme/tool", &CapabilitySchema::default());
    let state = AppState::new(ServerConfig {
        package_store_root: Some(root.path().to_path_buf()),
        ..in_memory_config::config()
    })
    .expect("AppState");
    let router = app(state.clone());

    let req = Request::builder()
        .method("DELETE")
        .uri("/v1/packages/@acme/tool")
        .body(Body::empty())
        .unwrap();
    let resp = router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let (_, body) = get_with_state(state, "/v1/packages").await;
    assert_eq!(body["packages"], json!([]));

    let req = Request::builder()
        .method("DELETE")
        .uri("/v1/packages/@acme/tool")
        .body(Body::empty())
        .unwrap();
    let resp = router.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn stdlib_groups_match_the_interpreter_catalog() {
    let (_, body) = get("/v1/capabilities").await;
    let served: usize = body["groups"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|g| g["kind"] == "stdlib")
        .map(|g| g["capabilities"].as_array().unwrap().len())
        .sum();
    let cataloged: usize = interpreter::stdlib::capabilities::catalog()
        .iter()
        .map(|g| g.capabilities.len())
        .sum();
    assert_eq!(served, cataloged);
}
