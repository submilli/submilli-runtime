//! `GET /v1/volumes` tests, in-process via `oneshot`.
//!
//! The endpoint publishes the operator's declared volume names so a client can
//! offer a choice. The host directory behind a name must never appear in the
//! response, so the leak assertion compares against the actual configured
//! paths rather than a substring guess.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::Value;
use submilli_server::config::VolumeTable;
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
        ..ServerConfig::default()
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
        ("project-alpha".to_string(), alpha.path().to_path_buf()),
        ("scratch".to_string(), scratch.path().to_path_buf()),
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
        ("project-alpha".to_string(), targets[0].clone()),
        ("scratch".to_string(), targets[1].clone()),
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
