use std::collections::BTreeMap;
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use interpreter::{ModulePath, PackageSourceModule, compile_package};
use serde_json::{Value, json};
use submilli_build::{ArtifactMetadata, write_package_artifact};
use submilli_server::audit::{Allows, AuditConfig, AuditLog};
use submilli_server::auth::{ApiToken, AuthConfig, Role};
use submilli_server::blueprint::InMemoryBlueprintStore;
use submilli_server::{AppState, ServerConfig, app};
use tower::ServiceExt;

const TOKEN: &str = "inbound-token-canary-1234567890123456789";

fn configured(path: std::path::PathBuf, allows: Allows, policy: &str) -> AppState {
    let audit = AuditConfig {
        file: Some(path),
        allows,
        ..Default::default()
    };
    let blueprints = Arc::new(
        InMemoryBlueprintStore::seed([submilli_blueprint::parse(policy).unwrap()]).unwrap(),
    );
    AppState::new(ServerConfig {
        blueprints: Some(blueprints),
        audit,
        ..Default::default()
    })
    .unwrap()
}

fn configured_with_package(
    path: std::path::PathBuf,
    allows: Allows,
    policy: &str,
    package_store_root: std::path::PathBuf,
) -> AppState {
    let blueprints = Arc::new(
        InMemoryBlueprintStore::seed([submilli_blueprint::parse(policy).unwrap()]).unwrap(),
    );
    AppState::new(ServerConfig {
        blueprints: Some(blueprints),
        package_store_root: Some(package_store_root),
        audit: AuditConfig {
            file: Some(path),
            allows,
            ..Default::default()
        },
        ..Default::default()
    })
    .unwrap()
}

fn install_credits_package(store_root: &std::path::Path) {
    let package = compile_package(
        "@acme/credits",
        ModulePath::from("lib"),
        &[PackageSourceModule {
            path: ModulePath::from("lib"),
            source: r#"
import { check } from "submilli:security";
/** @capability acme.com/credits.apply { customerId: string, amount: number, customerClass: string } */
export function applyCredits(): void {
    check("acme.com/credits.apply", {
        customerId: "cus_northwind", amount: 42, customerClass: "business"
    });
}
"#,
        }],
        &[],
    )
    .unwrap();
    write_package_artifact(
        store_root.join("@acme").join("credits"),
        &package.wasm,
        &package.type_info,
        &submilli_build::derive_capability_schema(
            &package.declaration,
            &[],
            &package.required_capabilities,
        ),
        &package.declaration,
        &ArtifactMetadata::new("@acme/credits", "0.0.0-test", Vec::new()),
    )
    .unwrap();
}

async fn request(router: Router, method: &str, path: &str, body: Value) -> (StatusCode, Value) {
    let response = router
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

fn parse(mut input: &str) -> BTreeMap<String, String> {
    let mut fields = BTreeMap::new();
    while !input.trim().is_empty() {
        input = input.trim_start();
        let (key, rest) = input.split_once('=').expect("logfmt key=value");
        let (value, remaining) = if rest.starts_with('"') {
            let mut values = serde_json::Deserializer::from_str(rest).into_iter::<String>();
            let value = values.next().unwrap().unwrap();
            (value, &rest[values.byte_offset()..])
        } else {
            let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
            (rest[..end].to_owned(), &rest[end..])
        };
        assert!(
            fields.insert(key.to_owned(), value).is_none(),
            "duplicate key"
        );
        input = remaining;
    }
    fields
}

fn records(path: &std::path::Path) -> Vec<BTreeMap<String, String>> {
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(parse)
        .collect()
}

#[tokio::test]
async fn package_and_stdlib_decisions_record_complete_json_payloads() {
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("packages");
    install_credits_package(&store);
    let code = r#"
import { applyCredits } from "@acme/credits";
import { writeText } from "submilli:fs";
function main(): void {
    try { applyCredits(); } catch (e: PermissionDeniedError) {}
    try { writeText("/denied", "body-canary"); } catch (e: PermissionDeniedError) {}
}
"#;
    let denied_path = dir.path().join("denied.log");
    let denied = app(configured_with_package(
        denied_path.clone(),
        Allows::Summary,
        "name: test\ndefault: deny\npackages:\n  - \"@acme/credits\"\n",
        store.clone(),
    ));
    let (_, response) = request(
        denied,
        "POST",
        "/v1/execute",
        json!({"blueprint": "test", "code": code}),
    )
    .await;
    assert!(response["error"].is_null(), "{response}");
    let rows = records(&denied_path);
    let package = rows
        .iter()
        .find(|row| row.get("capability") == Some(&"acme.com/credits.apply".to_string()))
        .unwrap();
    let package_payload: Value = serde_json::from_str(&package["context.payload_json"]).unwrap();
    assert_eq!(
        package_payload,
        json!({
            "customerId": "cus_northwind", "amount": 42, "customerClass": "business"
        })
    );
    assert_eq!(package["decision"], "deny");
    let stdlib = rows
        .iter()
        .find(|row| row.get("capability") == Some(&"fs.write".to_string()))
        .unwrap();
    let stdlib_payload: Value = serde_json::from_str(&stdlib["context.payload_json"]).unwrap();
    assert_eq!(stdlib_payload["path"], "/denied");
    assert_eq!(stdlib_payload["length"], 11);

    let allowed_path = dir.path().join("allowed.log");
    let allowed = app(configured_with_package(
        allowed_path.clone(),
        Allows::Summary,
        "name: test\ndefault: deny\npackages:\n  - \"@acme/credits\"\npermissions:\n  main:\n    - capability: acme.com/credits.apply\n      action: allow\n    - capability: fs.write\n      action: allow\n",
        store,
    ));
    let (_, response) = request(
        allowed,
        "POST",
        "/v1/execute",
        json!({"blueprint": "test", "code": code}),
    )
    .await;
    assert!(response["error"].is_null(), "{response}");
    let rows = records(&allowed_path);
    let package = rows
        .iter()
        .find(|row| row.get("capability") == Some(&"acme.com/credits.apply".to_string()))
        .unwrap();
    assert_eq!(package["decision"], "allow");
    let package_payload: Value = serde_json::from_str(&package["contexts.0.payload_json"]).unwrap();
    assert_eq!(package_payload["customerId"], "cus_northwind");
    let stdlib = rows
        .iter()
        .find(|row| row.get("capability") == Some(&"fs.write".to_string()))
        .unwrap();
    assert_eq!(stdlib["decision"], "allow");
    let stdlib_payload: Value = serde_json::from_str(&stdlib["contexts.0.payload_json"]).unwrap();
    assert_eq!(stdlib_payload["length"], 11);
}

#[tokio::test]
async fn allow_modes_keep_denials_and_attribute_rules_without_charging_fuel() {
    let mut fuel = Vec::new();
    for allows in [Allows::All, Allows::Summary, Allows::None] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("audit.log");
        let router = app(configured(
            path.clone(),
            allows,
            "name: test\npermissions:\n  main:\n    - capability: fs.read\n      action: allow\n",
        ));
        let code = r#"import { readText, writeText } from "submilli:fs";
function main(): void {
    for (let i = 0; i < 1000; i++) { try { readText("/file" + String(i)); } catch (e: Error) {} }
    try { writeText("/denied", "body-canary"); } catch (e: PermissionDeniedError) {}
}"#;
        let (_, response) = request(
            router,
            "POST",
            "/v1/execute",
            json!({"blueprint": "test", "code": code}),
        )
        .await;
        assert!(response["error"].is_null(), "{response}");
        let id = response["execution_id"].as_str().unwrap();
        let rows = records(&path);
        let decisions: Vec<_> = rows
            .iter()
            .filter(|r| r.get("type").map(String::as_str) == Some("decision"))
            .collect();
        let allows_rows: Vec<_> = decisions
            .iter()
            .filter(|r| r["decision"] == "allow")
            .collect();
        assert_eq!(
            decisions.iter().filter(|r| r["decision"] == "deny").count(),
            1
        );
        match allows {
            Allows::All => assert_eq!(allows_rows.len(), 1000),
            Allows::Summary => {
                assert_eq!(allows_rows.len(), 991);
                let summary = allows_rows
                    .iter()
                    .find(|row| row.contains_key("count"))
                    .unwrap();
                assert_eq!(summary["count"], "10");
                assert!(summary.contains_key("contexts.9.payload_json"));
                assert!(!summary.contains_key("contexts.10.payload_json"));
                assert_eq!(
                    allows_rows
                        .iter()
                        .filter(
                            |row| row.get("summary_overflow").map(String::as_str) == Some("true")
                        )
                        .count(),
                    990
                );
            }
            Allows::None => assert!(allows_rows.is_empty()),
        }
        for row in allows_rows {
            assert_eq!(row["rule"], "0");
        }
        let execution: Vec<_> = rows.iter().filter(|r| r["type"] == "execution").collect();
        assert_eq!(execution.len(), 2);
        assert_eq!(execution[0]["event"], "started");
        assert_eq!(execution[1]["event"], "finished");
        assert_eq!(execution[1]["outcome"], "ok");
        fuel.push(execution[1]["fuel"].clone());
        for row in execution {
            assert_eq!(row["execution_id"], id);
        }
        assert!(
            !std::fs::read_to_string(path)
                .unwrap()
                .contains("body-canary")
        );
    }
    assert!(fuel.windows(2).all(|w| w[0] == w[1]));
}

#[tokio::test]
async fn unavailable_output_does_not_stop_execution_and_retries_when_directory_appears() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().join("later");
    let path = parent.join("audit.log");
    let router = app(configured(
        path.clone(),
        Allows::All,
        "name: test\ndefault: allow\n",
    ));
    let body = json!({"blueprint": "test", "code": "function main(): string { return \"ok\"; }"});
    let (_, response) = request(router.clone(), "POST", "/v1/execute", body.clone()).await;
    assert_eq!(response["result"], "ok");
    std::fs::create_dir(&parent).unwrap();
    let (_, response) = request(router, "POST", "/v1/execute", body).await;
    assert_eq!(response["result"], "ok");
    assert!(
        records(&path)
            .iter()
            .any(|r| r["type"] == "execution" && r["event"] == "finished")
    );
}

#[tokio::test]
async fn auth_refusals_have_safe_metadata_and_never_the_token() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("audit.log");
    let config = ServerConfig {
        auth: AuthConfig::Tokens(vec![ApiToken::new("harness", Role::User, TOKEN).unwrap()]),
        audit: AuditConfig {
            file: Some(path.clone()),
            ..Default::default()
        },
        ..Default::default()
    };
    let router = app(AppState::new(config).unwrap());
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/status")
                .extension(axum::extract::ConnectInfo(
                    "127.0.0.1:12345".parse::<std::net::SocketAddr>().unwrap(),
                ))
                .header("authorization", format!("Bearer {TOKEN}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let response = router
        .oneshot(
            Request::builder()
                .uri("/v1/status")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(!text.contains(TOKEN));
    let rows = records(&path);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["reason"], "admin_required");
    assert_eq!(rows[0]["remote_address"], "127.0.0.1:12345");
    assert_eq!(rows[1]["reason"], "missing_token");
}

#[tokio::test]
async fn compile_validation_and_malformed_requests_have_execution_ids() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("audit.log");
    let router = app(configured(path.clone(), Allows::Summary, "name: test\n"));
    for body in [
        json!({"blueprint": "missing", "code": "function main(): void {}"}),
        json!({"blueprint": "test", "code": "function main(): missing {}"}),
        json!({}),
    ] {
        let (_, response) = request(router.clone(), "POST", "/v1/execute", body).await;
        assert!(uuid::Uuid::parse_str(response["execution_id"].as_str().unwrap()).is_ok());
        assert!(!response["error"].is_null());
    }
    let rows = records(&path);
    assert_eq!(
        rows.iter()
            .filter(|r| r["type"] == "execution" && r["event"] == "finished")
            .count(),
        3
    );
}

#[tokio::test]
async fn replay_returns_the_original_execution_id_without_another_run_record() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("audit.log");
    let router = app(configured(
        path.clone(),
        Allows::Summary,
        "name: test\ndefault: allow\n",
    ));
    let (_, session) = request(
        router.clone(),
        "POST",
        "/v1/sessions",
        json!({"blueprint": "test"}),
    )
    .await;
    let uri = format!(
        "/v1/sessions/{}/execute",
        session["session_id"].as_str().unwrap()
    );
    let mut responses = Vec::new();
    for _ in 0..2 {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(&uri)
                    .header("content-type", "application/json")
                    .header("idempotency-key", "same")
                    .body(Body::from(
                        json!({"code": "function main(): string { return \"ok\"; }"}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        responses.push(response.into_body().collect().await.unwrap().to_bytes());
    }
    assert_eq!(responses[0], responses[1]);
    assert_eq!(
        records(&path)
            .iter()
            .filter(|r| r["type"] == "execution" && r["event"] == "started")
            .count(),
        1
    );
}

#[tokio::test]
async fn shared_output_records_remain_whole_under_concurrent_runs() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("shared.log");
    let output = submilli_server::logging::LogOutput::open(Some(path.clone())).unwrap();
    let audit = AuditLog::new(AuditConfig::default(), Some(output.clone()));
    let blueprint = submilli_blueprint::parse("name: test\ndefault: allow\n").unwrap();
    let state = AppState::new(ServerConfig {
        audit_log: Some(audit),
        blueprints: Some(Arc::new(InMemoryBlueprintStore::seed([blueprint]).unwrap())),
        ..Default::default()
    })
    .unwrap();
    let router = app(state);
    let mut tasks = Vec::new();
    for i in 0..12 {
        let router = router.clone();
        let output = output.clone();
        tasks.push(tokio::spawn(async move {
            output
                .write_record(&format!("stream=log msg=run{i}\n"))
                .unwrap();
            request(
                router,
                "POST",
                "/v1/execute",
                json!({"blueprint": "test", "code": "function main(): void {}"}),
            )
            .await
        }));
    }
    for task in tasks {
        assert!(task.await.unwrap().1["error"].is_null());
    }
    let rows = records(&path);
    assert_eq!(rows.iter().filter(|r| r["stream"] == "log").count(), 12);
    for row in rows.iter().filter(|r| r["stream"] == "audit") {
        assert_eq!(row["schema"], "submilli.audit/1");
        assert!(row.contains_key("ts"));
    }
}

#[tokio::test]
async fn invariant_egress_and_quota_decisions_log_only_their_check_context() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("audit.log");
    let mut policy = submilli_blueprint::parse("name: test\ndefault: allow\n").unwrap();
    policy.secrets.insert(
        "CREDENTIAL".into(),
        submilli_blueprint::SecretSource::Harness(Default::default()),
    );
    let state = AppState::new(ServerConfig {
        audit: AuditConfig {
            file: Some(path.clone()),
            ..Default::default()
        },
        blueprints: Some(Arc::new(InMemoryBlueprintStore::seed([policy]).unwrap())),
        network_policy: interpreter::runtime::NetworkPolicy::deny_private(),
        session_kv_limits: interpreter::runtime::SessionKvLimits {
            max_entries: 0,
            ..Default::default()
        },
        ..Default::default()
    })
    .unwrap();
    let router = app(state);
    let programs = [
        r#"import { get } from "submilli:secrets"; function main(): void { try { get("CREDENTIAL"); } catch (e: PermissionDeniedError) {} }"#,
        r#"import { get } from "submilli:http"; function main(): void { try { get("https://127.0.0.1:9/path?token=query-canary"); } catch (e: Error) {} }"#,
        r#"import { set } from "submilli:session"; function main(): void { try { set("key", "state-canary"); } catch (e: QuotaExceededError) {} }"#,
    ];
    for code in programs {
        let (_, response) = request(
            router.clone(),
            "POST",
            "/v1/execute",
            json!({"blueprint": "test", "code": code, "secrets": {"CREDENTIAL": "secret-canary"}}),
        )
        .await;
        assert!(response["error"].is_null(), "{response}");
    }
    let rows = records(&path);
    for source in ["invariant", "egress_guard", "quota"] {
        assert!(
            rows.iter()
                .any(|r| r.get("source").map(String::as_str) == Some(source)
                    && r["decision"] == "deny"
                    && r.contains_key("context.payload_json")),
            "missing {source}: {rows:?}"
        );
    }
    let text = std::fs::read_to_string(path).unwrap();
    for canary in ["query-canary", "state-canary", "secret-canary"] {
        assert!(!text.contains(canary));
    }
}

#[tokio::test]
async fn unusual_variable_names_and_long_urls_keep_execution_records() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("audit.log");
    let router = app(configured(
        path.clone(),
        Allows::Summary,
        "name: test\nvariables:\n  'customer id':\n    default: default-customer\n  endpoint: {}\n  api_token: {}\n",
    ));
    let (_, response) = request(router, "POST", "/v1/execute", json!({"blueprint": "test", "code": "function main(): void {}", "variables": {"endpoint": format!("https://user:credential-canary@example.com/{}?token=query-canary", "x".repeat(1_100_000)), "api_token": "token-canary"}})).await;
    assert!(response["error"].is_null(), "{response}");
    let text = std::fs::read_to_string(path).unwrap();
    assert_eq!(text.matches("event=finished").count(), 1);
    assert!(text.contains("default-customer"));
    for canary in ["credential-canary", "query-canary", "token-canary"] {
        assert!(!text.contains(canary));
    }
}

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[tokio::test]
async fn authentication_audit_keeps_peer_addresses_for_http_and_https() {
    for use_tls in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let audit_path = dir.path().join("audit.log");
        let identity = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
        let mut roots = rustls::RootCertStore::empty();
        roots.add(identity.cert.der().clone()).unwrap();
        let client = Arc::new(
            rustls::ClientConfig::builder_with_provider(Arc::new(
                rustls::crypto::ring::default_provider(),
            ))
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_root_certificates(roots)
            .with_no_client_auth(),
        );
        let tls = use_tls.then(|| {
            Arc::new(
                rustls::ServerConfig::builder_with_provider(Arc::new(
                    rustls::crypto::ring::default_provider(),
                ))
                .with_safe_default_protocol_versions()
                .unwrap()
                .with_no_client_auth()
                .with_single_cert(
                    vec![identity.cert.der().clone()],
                    rustls::pki_types::PrivatePkcs8KeyDer::from(
                        identity.signing_key.serialize_der(),
                    )
                    .into(),
                )
                .unwrap(),
            )
        });
        let reservation = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = reservation.local_addr().unwrap();
        drop(reservation);
        let config = ServerConfig {
            tls,
            auth: AuthConfig::Tokens(vec![ApiToken::new("admin", Role::Admin, TOKEN).unwrap()]),
            audit: AuditConfig {
                file: Some(audit_path.clone()),
                ..Default::default()
            },
            session_storage_root: Some(dir.path().join("sessions")),
            ..Default::default()
        };
        let server = tokio::spawn(submilli_server::serve(
            addr,
            config,
            std::time::Duration::from_secs(5),
        ));
        tokio::task::spawn_blocking(move || {
            let client = use_tls.then_some(client);
            let response = socket_request(addr, client.clone(), "GET", "/v1/status", None);
            assert!(response.starts_with("HTTP/1.1 401"), "{response}");
            let response = socket_request(addr, client, "POST", "/v1/shutdown", Some(TOKEN));
            assert!(response.starts_with("HTTP/1.1 200"), "{response}");
        })
        .await
        .unwrap();
        server.await.unwrap().unwrap();
        let rows = records(&audit_path);
        let refusal = rows.iter().find(|r| r["type"] == "auth").unwrap();
        let peer: std::net::SocketAddr = refusal["remote_address"].parse().unwrap();
        assert!(peer.ip().is_loopback());
        assert!(
            !std::fs::read_to_string(&audit_path)
                .unwrap()
                .contains(TOKEN)
        );
        assert_eq!(
            rows.iter()
                .filter(|r| r["type"] == "server" && r["event"] == "started")
                .count(),
            1
        );
        assert_eq!(
            rows.iter()
                .filter(|r| r["type"] == "server" && r["event"] == "stopped")
                .count(),
            1
        );
    }
}

fn socket_request(
    addr: std::net::SocketAddr,
    tls: Option<Arc<rustls::ClientConfig>>,
    method: &str,
    path: &str,
    token: Option<&str>,
) -> String {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let socket = loop {
        match std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_secs(1)) {
            Ok(socket) => break socket,
            Err(error) => {
                assert!(
                    std::time::Instant::now() < deadline,
                    "server did not start: {error}"
                );
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
        }
    };
    socket
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .unwrap();
    socket
        .set_write_timeout(Some(std::time::Duration::from_secs(5)))
        .unwrap();
    let authorization = token.map_or_else(String::new, |token| {
        format!("Authorization: Bearer {token}\r\n")
    });
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: localhost\r\n{authorization}Content-Length: 0\r\nConnection: close\r\n\r\n"
    );
    if let Some(tls) = tls {
        let connection = rustls::ClientConnection::new(
            tls,
            rustls::pki_types::ServerName::try_from("localhost").unwrap(),
        )
        .unwrap();
        return exchange(&mut rustls::StreamOwned::new(connection, socket), &request);
    }
    exchange(&mut { socket }, &request)
}

fn exchange(stream: &mut (impl std::io::Read + std::io::Write), request: &str) -> String {
    stream.write_all(request.as_bytes()).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    response
}
