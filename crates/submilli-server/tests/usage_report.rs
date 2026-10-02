//! Usage is logged once per execution, including limits, without changing the response.
use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::Body;
use axum::http::Request;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use submilli_blueprint::Blueprint;
use submilli_server::blueprint::InMemoryBlueprintStore;
use submilli_server::{AppState, RuntimeConfig, ServerConfig, app};
use tower::ServiceExt;

#[derive(Clone, Default)]
struct LogBuffer(Arc<Mutex<Vec<u8>>>);

impl Write for LogBuffer {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl tracing_subscriber::fmt::MakeWriter<'_> for LogBuffer {
    type Writer = Self;
    fn make_writer(&self) -> Self {
        self.clone()
    }
}

async fn execute(config: RuntimeConfig, code: &str) -> Value {
    let blueprints = Arc::new(InMemoryBlueprintStore::seed([Blueprint {
        name: "usage-test".into(),
        ..Default::default()
    }]));
    let state = AppState::new(ServerConfig {
        blueprints: Some(blueprints),
        runtime: config,
        ..Default::default()
    })
    .unwrap();
    let request = Request::builder()
        .method("POST")
        .uri("/v1/execute")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({"blueprint": "usage-test", "code": code}).to_string(),
        ))
        .unwrap();
    let response = app(state).oneshot(request).await.unwrap();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn logs_usage_for_success_and_each_execution_failure() {
    let logs = LogBuffer::default();
    tracing::subscriber::set_global_default(
        tracing_subscriber::fmt()
            .without_time()
            .with_ansi(false)
            .with_max_level(tracing::Level::INFO)
            .with_writer(logs.clone())
            .finish(),
    )
    .unwrap();
    let cases = [
        (
            RuntimeConfig::default(),
            "function main(): number { return 42; }",
            "ok",
        ),
        (
            RuntimeConfig {
                fuel: 100_000,
                ..Default::default()
            },
            "while (true) {} function main(): void {}",
            "fuel_exhausted",
        ),
        (
            RuntimeConfig {
                timeout: Some(Duration::from_millis(10)),
                ..Default::default()
            },
            "function main(): void { while (true) {} }",
            "timeout",
        ),
        (
            RuntimeConfig {
                max_store_bytes: 256 * 1024,
                ..Default::default()
            },
            "function main(): string { return \"x\".repeat(1000000); }",
            "memory_exhausted",
        ),
        (
            RuntimeConfig::default(),
            "function main(): void { throw new Error(\"failed\"); }",
            "error",
        ),
        (
            RuntimeConfig::default(),
            "function main(): number { return false; }",
            "error",
        ),
    ];
    for (config, code, expected) in cases {
        logs.0.lock().unwrap().clear();
        let response = execute(config, code).await;
        assert!(response.get("usage").is_none(), "{response}");
        assert!(response.get("fuel").is_none(), "{response}");
        if expected == "ok" {
            assert!(response["error"].is_null(), "{response}");
        } else {
            assert!(!response["error"].is_null(), "{response}");
        }
        let captured = String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
        let lines: Vec<_> = captured
            .lines()
            .filter(|line| line.contains("execution finished"))
            .collect();
        assert_eq!(lines.len(), 1, "{captured}");
        let line = lines[0];
        assert!(line.contains("INFO submilli_server::execute:"), "{line}");
        assert!(line.contains("blueprint=\"usage-test\""), "{line}");
        let session = response["session_id"].as_str().unwrap();
        assert!(line.contains(&format!("session=\"{session}\"")), "{line}");
        assert!(line.contains(&format!("outcome=\"{expected}\"")), "{line}");
        for field in ["fuel", "memory_peak", "wall_ms"] {
            let value = line
                .split_whitespace()
                .find_map(|part| part.strip_prefix(&format!("{field}=")))
                .unwrap();
            let number: u64 = value.parse().unwrap();
            if expected == "ok" && field != "wall_ms" {
                assert!(number > 0, "{line}");
            }
            if expected == "fuel_exhausted" && field == "fuel" {
                assert_eq!(number, 100_000);
            }
        }
    }
}
