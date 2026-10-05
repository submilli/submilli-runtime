//! `GET /v1/volumes` tests, in-process via `oneshot`.
//!
//! The endpoint publishes the operator's declared volume names so a client can
//! offer a choice. The host directory behind a name must never appear in the
//! response, so the leak assertion compares against the actual configured
//! paths rather than a substring guess.

#[path = "common/in_memory_config.rs"]
mod in_memory_config;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::Value;
use submilli_server::config::{VolumeSpec, VolumeTable};
use submilli_server::{AppState, ServerConfig, app};
use tower::ServiceExt;

/// A server over `volumes`, plus the raw response body text for `GET
/// /v1/volumes` — the text, not just the parsed value, so a leaked host
/// directory anywhere in the payload is caught.
async fn list(volumes: VolumeTable) -> (StatusCode, Value, String) {
    send(volumes, "GET").await
}

async fn send(volumes: VolumeTable, method: &str) -> (StatusCode, Value, String) {
    let config = ServerConfig {
        volumes,
        ..in_memory_config::config()
    };
    let state = AppState::new(config).expect("AppState");
    let req = Request::builder()
        .method(method)
        .uri("/v1/volumes")
        .body(Body::empty())
        .unwrap();
    let resp = app(state).oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let text = String::from_utf8_lossy(&bytes).into_owned();
    let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, body, text)
}

fn names(body: &Value) -> Vec<&str> {
    body["volumes"]
        .as_array()
        .unwrap_or_else(|| panic!("no `volumes` array in: {body}"))
        .iter()
        .map(|v| v.as_str().expect("volume name is a string"))
        .collect()
}

#[tokio::test]
async fn lists_every_declared_volume_name() {
    let alpha = tempfile::tempdir().expect("alpha dir");
    let scratch = tempfile::tempdir().expect("scratch dir");
    let volumes = VolumeTable::from([
        (
            "project-alpha".to_string(),
            VolumeSpec::local_path(alpha.path()),
        ),
        (
            "scratch".to_string(),
            VolumeSpec::local_path(scratch.path()),
        ),
    ]);

    let (status, body, _) = list(volumes).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(names(&body), ["project-alpha", "scratch"]);
}

#[tokio::test]
async fn never_reveals_the_host_directory_behind_a_name() {
    let alpha = tempfile::tempdir().expect("alpha dir");
    let scratch = tempfile::tempdir().expect("scratch dir");
    let targets = [alpha.path().to_path_buf(), scratch.path().to_path_buf()];
    let volumes = VolumeTable::from([
        (
            "project-alpha".to_string(),
            VolumeSpec::local_path(&targets[0]),
        ),
        ("scratch".to_string(), VolumeSpec::local_path(&targets[1])),
    ]);

    let (status, _, text) = list(volumes).await;

    assert_eq!(status, StatusCode::OK);
    for target in &targets {
        let target = target.to_string_lossy();
        assert!(
            !text.contains(target.as_ref()),
            "host directory {target} leaked into the response: {text}"
        );
    }
}

#[tokio::test]
async fn an_empty_table_is_an_empty_list_not_an_error() {
    let (status, body, _) = list(VolumeTable::new()).await;

    assert_eq!(status, StatusCode::OK);
    assert!(names(&body).is_empty(), "expected no volumes: {body}");
}

#[tokio::test]
async fn write_methods_are_rejected() {
    for method in ["POST", "PUT", "DELETE"] {
        let (status, _, _) = send(VolumeTable::new(), method).await;
        assert_eq!(
            status,
            StatusCode::METHOD_NOT_ALLOWED,
            "{method} /v1/volumes must not be accepted"
        );
    }
}

#[tokio::test]
async fn injected_database_path_is_guarded_even_with_a_different_configured_path() {
    use std::sync::Arc;
    use submilli_server::config::{ServerDirectories, validate_volumes};
    use submilli_server::database::ServerDatabase;

    let directory = tempfile::tempdir().unwrap();
    let database = Arc::new(
        ServerDatabase::open(&directory.path().join("server.db"))
            .await
            .unwrap(),
    );
    for configured_path in [None, Some(directory.path().join("elsewhere/server.db"))] {
        let config = ServerConfig {
            database: Some(Arc::clone(&database)),
            database_path: configured_path,
            ..in_memory_config::config()
        };
        let volumes =
            VolumeTable::from([("exposed".into(), VolumeSpec::local_path(directory.path()))]);
        let error =
            validate_volumes(&volumes, &ServerDirectories::from_config(&config)).unwrap_err();
        assert!(error.to_string().contains("server database"), "{error}");
    }
    database.close().await.unwrap();
}
