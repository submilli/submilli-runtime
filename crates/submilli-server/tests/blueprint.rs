use std::path::PathBuf;
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use submilli_server::blueprint::InMemoryBlueprintStore;
use submilli_server::config::{VolumeSpec, VolumeTable};
use submilli_server::{AppState, ServerConfig, app};
use tower::ServiceExt;

fn router() -> Router {
    app(AppState::new(ServerConfig::default()).expect("build AppState"))
}

/// Router over pre-parsed blueprints, bypassing the HTTP add path (and its
/// secret verification) — for list/show tests that need rich declarations.
fn seeded_router(yamls: &[&str]) -> Router {
    let blueprints = yamls
        .iter()
        .map(|yaml| submilli_blueprint::parse(yaml).expect("valid blueprint"));
    let store = Arc::new(InMemoryBlueprintStore::seed(blueprints));
    app(AppState::new(ServerConfig {
        blueprints: Some(store),
        ..ServerConfig::default()
    })
    .expect("build AppState"))
}

/// Router over a server declaring `names` as volumes. The targets never have to
/// exist: registration checks the name against the table, and mounting (which
/// would touch the directory) is a different unit's boundary.
fn router_with_volumes(names: &[&str]) -> Router {
    let volumes: VolumeTable = names
        .iter()
        .map(|name| {
            (
                name.to_string(),
                VolumeSpec::local_path(PathBuf::from(format!("/srv/{name}"))),
            )
        })
        .collect();
    app(AppState::new(ServerConfig {
        volumes,
        ..ServerConfig::default()
    })
    .expect("build AppState"))
}

/// Router over a store that already holds a blueprint in a form this binary no longer
/// accepts — the on-disk shape a pre-upgrade server left behind.
fn router_over_retired_form(dir: &std::path::Path) -> Router {
    std::fs::write(
        dir.join("tenant-alpha.000001.yaml"),
        "name: tenant-alpha\nvfs:\n  mode: persistent\n  volume: tenant-alpha\n",
    )
    .expect("plant revision");
    std::fs::write(
        dir.join("index.json"),
        json!({ "tenant-alpha": 1 }).to_string(),
    )
    .expect("plant index");
    let store = Arc::new(
        submilli_server::blueprint::FileBlueprintStore::new(dir.to_path_buf())
            .expect("boots with a retired-form revision"),
    );
    app(AppState::new(ServerConfig {
        blueprints: Some(store),
        ..ServerConfig::default()
    })
    .expect("build AppState"))
}

/// The reservation is only half a fix if any route still says "unknown blueprint":
/// the operator would go looking for a blueprint that is sitting right there. Every
/// entry point that resolves a name has to render the reason instead — the two REST
/// ones, the MCP OAuth admin surface behind `submilli server mcp`, and
/// `/mcp/{blueprint}`, which is where an agent meets the name.
///
/// The in-service MCP sites (the tool bodies, and rmcp's `initialize_session`) render
/// the same reason but are only reachable once a *live* service loses its blueprint
/// mid-connection, which no store transition here can produce: an unusable revision
/// is held out from the moment the store loads it.
///
/// The error *code* each route reports is pinned, not just the prose: a client needs
/// one predicate for "this name is not runnable", and a substring match on the body
/// would not notice the code changing under it.
#[tokio::test]
async fn a_reserved_name_reports_why_it_cannot_run_rather_than_not_found() {
    let dir = tempfile::tempdir().expect("tempdir");
    let router = router_over_retired_form(dir.path());

    // `/v1/execute` reports failure in its envelope rather than in the status line;
    // `/v1/sessions` uses the status. Both must carry the reason.
    for (route, expected, body, code_at, code) in [
        (
            "/v1/execute",
            StatusCode::OK,
            json!({ "blueprint": "tenant-alpha", "code": "function main(): void {}" }),
            "/error/kind",
            json!("blueprint_not_found"),
        ),
        (
            "/v1/sessions",
            StatusCode::NOT_FOUND,
            json!({ "blueprint": "tenant-alpha" }),
            "/error",
            json!("unknown blueprint"),
        ),
    ] {
        let (status, body) = post(&router, route, body).await;
        let rendered = body.to_string();
        assert_eq!(status, expected, "{route}: {rendered}");
        assert!(
            !rendered.contains("unknown blueprint: "),
            "{route} must not claim the name is unknown: {rendered}",
        );
        assert!(
            rendered.contains("`persistent` was removed") && rendered.contains("mode: named"),
            "{route} must name the retired mode and its replacement: {rendered}",
        );
        assert_eq!(
            body.pointer(code_at),
            Some(&code),
            "{route} must keep reporting an unrunnable name under one code: {rendered}",
        );
    }

    // The MCP OAuth admin surface resolves the same name for the CLI. It is a GET,
    // and its failure body is the shared error envelope.
    let (status, body) = get(&router, "/v1/mcp/tenant-alpha/auth-status").await;
    let rendered = body.to_string();
    assert_eq!(status, StatusCode::NOT_FOUND, "auth-status: {rendered}");
    assert!(
        !rendered.contains("is not registered"),
        "auth-status must not claim the name is unregistered: {rendered}",
    );
    assert!(
        rendered.contains("`persistent` was removed") && rendered.contains("mode: named"),
        "auth-status must name the retired mode and its replacement: {rendered}",
    );

    // The MCP endpoint answers in plain text, not JSON, so it is checked on its own
    // rather than through `post`.
    let req = Request::builder()
        .method("POST")
        .uri("/mcp/tenant-alpha")
        .header("host", "localhost")
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .body(Body::from(
            json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {} }).to_string(),
        ))
        .unwrap();
    let resp = router.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let rendered = String::from_utf8_lossy(&bytes).into_owned();
    assert_eq!(status, StatusCode::NOT_FOUND, "/mcp: {rendered}");
    assert!(
        !rendered.contains("unknown blueprint: "),
        "/mcp must not claim the name is unknown: {rendered}",
    );
    assert!(
        rendered.contains("`persistent` was removed") && rendered.contains("mode: named"),
        "/mcp must name the retired mode and its replacement: {rendered}",
    );
}

fn listed_names(body: &Value) -> Vec<&str> {
    body["blueprints"]
        .as_array()
        .expect("blueprints array")
        .iter()
        .map(|b| b["name"].as_str().expect("name"))
        .collect()
}

async fn post(router: &Router, path: &str, body: Value) -> (StatusCode, Value) {
    let req = Request::builder()
        .method("POST")
        .uri(path)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    collect(router.clone().oneshot(req).await.unwrap()).await
}

async fn get(router: &Router, path: &str) -> (StatusCode, Value) {
    send(router, "GET", path, Body::empty()).await
}

async fn put(router: &Router, path: &str, body: Value) -> (StatusCode, Value) {
    let req = Request::builder()
        .method("PUT")
        .uri(path)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    collect(router.clone().oneshot(req).await.unwrap()).await
}

async fn delete(router: &Router, path: &str) -> (StatusCode, Value) {
    send(router, "DELETE", path, Body::empty()).await
}

async fn send(router: &Router, method: &str, path: &str, body: Body) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(method)
        .uri(path)
        .body(body)
        .unwrap();
    collect(router.clone().oneshot(req).await.unwrap()).await
}

async fn collect(resp: axum::response::Response) -> (StatusCode, Value) {
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let parsed: Value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, parsed)
}

#[tokio::test]
async fn add_then_list_round_trip() {
    let router = router();
    let (status, body) = post(
        &router,
        "/v1/blueprints",
        json!({ "yaml": "name: production\n" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["name"], json!("production"));

    let (status, body) = get(&router, "/v1/blueprints").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listed_names(&body), ["production"]);

    let summary = &body["blueprints"][0];
    assert_eq!(summary["vfs_mode"], json!("ephemeral"));
    assert_eq!(summary["idle_timeout_secs"], json!(86400));
    assert_eq!(summary["packages"], json!([]));
    assert_eq!(summary["secrets"], json!([]));
    assert_eq!(summary["variables"], json!([]));
    assert_eq!(summary["mcp_servers"], json!([]));
    assert_eq!(summary["auth_proxy_hosts"], json!([]));
    assert_eq!(summary["default_action"], json!(null));
    assert_eq!(summary["caller_count"], json!(0));
    assert_eq!(summary["rule_count"], json!(0));
    assert_eq!(summary["capability_count"], json!(0));
}

#[tokio::test]
async fn list_summarizes_blueprint_shape() {
    let router = seeded_router(&["\
name: rich
idle_timeout: 1h
vfs: per_session
packages:
  - \"@acme/tool\"
secrets:
  API_KEY:
    store: api-key
  TOKEN:
    harness:
      required: true
variables:
  tenant:
    required: true
auth_proxy:
  - host: api.example.com
    auth:
      bearer: API_KEY
default: deny
permissions:
  main:
    - capability: fs.read
      action: allow
    - capability: http.get
      action: allow
  \"@acme/tool\":
    - capability: http.get
      action: allow
mcp:
  linear:
    url: https://mcp.linear.app/mcp
"]);

    let (status, body) = get(&router, "/v1/blueprints").await;
    assert_eq!(status, StatusCode::OK);
    let summary = &body["blueprints"][0];
    assert_eq!(summary["name"], json!("rich"));
    assert_eq!(summary["vfs_mode"], json!("per_session"));
    assert_eq!(summary["idle_timeout_secs"], json!(3600));
    assert_eq!(summary["packages"], json!(["@acme/tool"]));
    assert_eq!(
        summary["secrets"],
        json!([
            { "name": "API_KEY", "source": "store", "store_key": "api-key" },
            { "name": "TOKEN", "source": "harness" },
        ])
    );
    assert_eq!(summary["variables"], json!(["tenant"]));
    assert_eq!(summary["mcp_servers"], json!(["linear"]));
    assert_eq!(summary["auth_proxy_hosts"], json!(["api.example.com"]));
    assert_eq!(summary["default_action"], json!("deny"));
    assert_eq!(summary["caller_count"], json!(2));
    assert_eq!(summary["rule_count"], json!(3));
    assert_eq!(summary["capability_count"], json!(2));
}

#[tokio::test]
async fn list_is_sorted_by_name() {
    let router = router();
    for name in ["zebra", "alpha", "mike"] {
        let (status, _) = post(
            &router,
            "/v1/blueprints",
            json!({ "yaml": format!("name: {name}\n") }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
    }
    let (_, body) = get(&router, "/v1/blueprints").await;
    assert_eq!(listed_names(&body), ["alpha", "mike", "zebra"]);
}

#[tokio::test]
async fn duplicate_name_conflicts() {
    let router = router();
    let (status, _) = post(
        &router,
        "/v1/blueprints",
        json!({ "yaml": "name: production\n" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = post(
        &router,
        "/v1/blueprints",
        json!({ "yaml": "name: production\n" }),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"], json!("already_exists"));
    assert_eq!(body["name"], json!("production"));
    assert!(body.get("diagnostics").is_none(), "got {body}");
}

#[tokio::test]
async fn malformed_yaml_rejected() {
    let router = router();
    let (status, body) = post(
        &router,
        "/v1/blueprints",
        json!({ "yaml": ":\n  not yaml: -" }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], json!("parse_error"));
}

#[tokio::test]
async fn unknown_field_rejected() {
    let router = router();
    let (status, body) = post(
        &router,
        "/v1/blueprints",
        json!({ "yaml": "name: production\nbogus: {}\n" }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], json!("parse_error"));
}

#[tokio::test]
async fn env_secret_source_rejected_over_the_api() {
    let router = router();
    let (status, body) = post(
        &router,
        "/v1/blueprints",
        json!({ "yaml": "name: leaky\nsecrets:\n  KEY:\n    env: SUBMILLI_SECRET_STORE_KEY\n" }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], json!("parse_error"));
    assert_eq!(
        body["diagnostics"][0]["path"],
        json!(["secrets", "KEY", "env"])
    );
}

#[tokio::test]
async fn file_secret_source_rejected_over_the_api() {
    let router = router();
    let (status, body) = put(
        &router,
        "/v1/blueprints/leaky",
        json!({ "yaml": "name: leaky\nsecrets:\n  KEY:\n    file: /root/.aws/credentials\n" }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], json!("parse_error"));
}

#[tokio::test]
async fn invalid_name_rejected() {
    let router = router();
    let (status, body) = post(
        &router,
        "/v1/blueprints",
        json!({ "yaml": "name: 'has space'\n" }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], json!("invalid_name"));
    assert_eq!(body["diagnostics"][0]["path"], json!(["name"]));
}

#[tokio::test]
async fn invalid_permission_rule_diagnostic_points_at_the_rule() {
    let router = router();
    let yaml = "\
name: x
permissions:
  main:
    - capability: fs.read
      action: allow
    - capability: \"\"
      action: allow
";
    let (status, body) = post(&router, "/v1/blueprints", json!({ "yaml": yaml })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], json!("invalid_permissions"));
    assert_eq!(
        body["diagnostics"][0]["path"],
        json!(["permissions", "main", 1, "capability"])
    );
    assert!(
        body["diagnostics"][0]["message"]
            .as_str()
            .unwrap()
            .contains("empty capability"),
        "got {body}"
    );
}

#[tokio::test]
async fn parse_error_diagnostic_carries_line_and_col() {
    let router = router();
    let (status, body) = post(
        &router,
        "/v1/blueprints",
        json!({ "yaml": "name: x\nvfs: [\n" }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], json!("parse_error"));
    assert!(
        body["diagnostics"][0]["line"].is_u64() && body["diagnostics"][0]["col"].is_u64(),
        "got {body}"
    );
}

#[tokio::test]
async fn malformed_filter_diagnostic_carries_the_serde_path() {
    let router = router();
    let yaml = "\
name: x
permissions:
  main:
    - capability: c
      filter: amount <> 5
      action: allow
";
    let (status, body) = post(&router, "/v1/blueprints", json!({ "yaml": yaml })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], json!("parse_error"));
    assert_eq!(
        body["diagnostics"][0]["path"],
        json!(["permissions", "main", 0, "filter"])
    );
}

#[tokio::test]
async fn empty_yaml_rejected() {
    let router = router();
    let (status, body) = post(&router, "/v1/blueprints", json!({ "yaml": "" })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], json!("parse_error"));
}

#[tokio::test]
async fn empty_list_on_fresh_server() {
    let router = router();
    let (status, body) = get(&router, "/v1/blueprints").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["blueprints"], json!([]));
}

#[tokio::test]
async fn apply_creates_then_replaces() {
    let router = router();
    let (status, body) = put(
        &router,
        "/v1/blueprints/production",
        json!({ "yaml": "name: production\n" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["name"], json!("production"));
    assert_eq!(body["created"], json!(true));

    let (status, body) = put(
        &router,
        "/v1/blueprints/production",
        json!({ "yaml": "name: production\n" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["created"], json!(false));
}

#[tokio::test]
async fn apply_name_mismatch_rejected() {
    let router = router();
    let (status, body) = put(
        &router,
        "/v1/blueprints/staging",
        json!({ "yaml": "name: production\n" }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], json!("name_mismatch"));
    assert_eq!(body["diagnostics"][0]["path"], json!(["name"]));
}

#[tokio::test]
async fn apply_malformed_yaml_rejected() {
    let router = router();
    let (status, body) = put(
        &router,
        "/v1/blueprints/production",
        json!({ "yaml": ":\n  not yaml: -" }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], json!("parse_error"));
}

#[tokio::test]
async fn show_round_trip() {
    let router = router();
    let (status, _) = post(
        &router,
        "/v1/blueprints",
        json!({ "yaml": "name: production\n" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = get(&router, "/v1/blueprints/production").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["name"], json!("production"));
    assert!(
        body["yaml"].as_str().unwrap().contains("name: production"),
        "yaml was {:?}",
        body["yaml"]
    );
}

/// The REST prompt endpoint must serve exactly what the MCP `execute` tool
/// description serves — the HTTP client harness teaches its model from this,
/// so a drift here means a REST agent is taught a different language than an
/// MCP client.
#[tokio::test]
async fn prompt_uses_rest_discovery_vocabulary() {
    let yaml = "name: prompted\nvfs: per_session\n";
    let router = seeded_router(&[yaml]);

    let (status, body) = get(&router, "/v1/blueprints/prompted/prompt").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["name"], json!("prompted"));

    let blueprint = submilli_blueprint::parse(yaml).expect("valid blueprint");
    let expected = submilli_shared::prompt::execute_tool_description(
        &blueprint,
        submilli_shared::prompt::PromptSurface::Rest,
    );
    assert_eq!(body["prompt"].as_str().unwrap(), expected);
    for tool in ["`search`", "`docs`", "`builtins`"] {
        assert!(expected.contains(tool), "{expected}");
    }
    assert!(!expected.contains("submilli__typescript__"));

    // Placeholders resolve rather than leaking through to the model.
    let prompt = body["prompt"].as_str().unwrap();
    assert!(!prompt.contains("{vfs_mode}"), "got: {prompt}");
    assert!(!prompt.contains("{builtins}"), "got: {prompt}");
    assert!(prompt.contains("function main()"), "got: {prompt}");
}

/// A REST harness can only publish `lastRun` if the endpoint tells it what the
/// tool is for — and it must be the same text MCP publishes, or the two
/// surfaces teach different ways to recover a successful run's console.
#[tokio::test]
async fn prompt_serves_the_shared_tool_descriptions() {
    use submilli_shared::prompt::tools as shared;

    let router = seeded_router(&["name: prompted\nvfs: per_session\n"]);

    let (status, body) = get(&router, "/v1/blueprints/prompted/prompt").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["tools"]["search"], json!(shared::PACKAGES_SEARCH));
    assert_eq!(body["tools"]["docs"], json!(shared::PACKAGES_DOCS));
    assert_eq!(body["tools"]["builtins"], json!(shared::BUILTINS_MERGED));
    assert_eq!(body["tools"]["last_run"], json!(shared::LAST_RUN));
    assert!(
        shared::LAST_RUN.contains("FULL console"),
        "got: {}",
        shared::LAST_RUN
    );
}

#[tokio::test]
async fn prompt_missing_is_not_found() {
    let router = router();
    let (status, body) = get(&router, "/v1/blueprints/ghost/prompt").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"], json!("not_found"));
}

#[tokio::test]
async fn show_missing_is_not_found() {
    let router = router();
    let (status, body) = get(&router, "/v1/blueprints/ghost").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"], json!("not_found"));
    assert!(body.get("diagnostics").is_none(), "got {body}");
}

#[tokio::test]
async fn remove_then_gone() {
    let router = router();
    let (status, _) = post(
        &router,
        "/v1/blueprints",
        json!({ "yaml": "name: production\n" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = delete(&router, "/v1/blueprints/production").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["name"], json!("production"));

    let (status, _) = get(&router, "/v1/blueprints/production").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn remove_missing_is_not_found() {
    let router = router();
    let (status, body) = delete(&router, "/v1/blueprints/ghost").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"], json!("not_found"));
}

#[tokio::test]
async fn auth_proxy_with_harness_secret_accepted() {
    // Harness values bind at execution time, not blueprint registration.
    let router = router();
    let yaml = "name: ap-ok\nsecrets:\n  K:\n    harness:\n      required: true\nauth_proxy:\n  - host: api.example.com\n    headers:\n      Authorization: \"Bearer ${secrets.K}\"\n";
    let (status, body) = post(&router, "/v1/blueprints", json!({ "yaml": yaml })).await;
    assert_eq!(status, StatusCode::OK, "got {body}");
    assert_eq!(body["name"], json!("ap-ok"));
}

#[tokio::test]
async fn harness_secret_accepted_at_add() {
    // Harness secrets bind per session; registration must not require a value.
    let router = router();
    let yaml = "name: harness-ok\nsecrets:\n  TOKEN:\n    harness:\n      required: true\n";
    let (status, body) = post(&router, "/v1/blueprints", json!({ "yaml": yaml })).await;
    assert_eq!(status, StatusCode::OK, "got {body}");
    assert_eq!(body["name"], json!("harness-ok"));
}

#[tokio::test]
async fn add_with_undeclared_volume_rejected_and_stores_nothing() {
    let router = router_with_volumes(&["project-alpha"]);
    let (status, body) = post(
        &router,
        "/v1/blueprints",
        json!({ "yaml": "name: escaper\nvfs:\n  mode: named\n  volume: unknown-vol\n" }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "got {body}");
    assert_eq!(body["error"], json!("undeclared_volume"));
    assert_eq!(body["name"], json!("escaper"));
    assert_eq!(body["diagnostics"][0]["path"], json!(["vfs", "volume"]));
    let message = body["message"].as_str().expect("message");
    assert!(message.contains("unknown-vol"), "got {message}");
    assert!(message.contains("project-alpha"), "got {message}");

    let (_, listed) = get(&router, "/v1/blueprints").await;
    assert!(listed_names(&listed).is_empty(), "got {listed}");
}

#[tokio::test]
async fn apply_with_undeclared_volume_rejected_and_stores_nothing() {
    let router = router_with_volumes(&["project-alpha"]);
    let (status, body) = put(
        &router,
        "/v1/blueprints/escaper",
        json!({ "yaml": "name: escaper\nvfs:\n  mode: named\n  volume: unknown-vol\n" }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "got {body}");
    assert_eq!(body["error"], json!("undeclared_volume"));
    assert_eq!(body["diagnostics"][0]["path"], json!(["vfs", "volume"]));

    let (status, _) = get(&router, "/v1/blueprints/escaper").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn add_with_declared_volume_succeeds() {
    let router = router_with_volumes(&["project-alpha"]);
    let (status, body) = post(
        &router,
        "/v1/blueprints",
        json!({ "yaml": "name: worker\nvfs:\n  mode: named\n  volume: project-alpha\n" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "got {body}");
    assert_eq!(body["name"], json!("worker"));

    let (_, listed) = get(&router, "/v1/blueprints").await;
    assert_eq!(listed_names(&listed), ["worker"]);
}

#[tokio::test]
async fn undeclared_volume_with_no_volumes_declared_says_how_to_declare_one() {
    let router = router_with_volumes(&[]);
    let (status, body) = post(
        &router,
        "/v1/blueprints",
        json!({ "yaml": "name: escaper\nvfs:\n  mode: named\n  volume: anything\n" }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "got {body}");
    assert_eq!(body["error"], json!("undeclared_volume"));
    let message = body["message"].as_str().expect("message");
    assert!(message.contains("no volumes"), "got {message}");
    assert!(message.contains("volumes:"), "got {message}");
    assert!(message.contains("server config"), "got {message}");
}

#[tokio::test]
async fn unnamed_modes_are_unaffected_by_the_volume_check() {
    let router = router_with_volumes(&[]);
    for (name, vfs) in [
        ("no-vfs", "vfs: none\n"),
        ("scratch", "vfs: ephemeral\n"),
        ("per-sess", "vfs: per_session\n"),
    ] {
        let (status, body) = post(
            &router,
            "/v1/blueprints",
            json!({ "yaml": format!("name: {name}\n{vfs}") }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{name}: got {body}");
    }
}
