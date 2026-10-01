//! Blueprint capability rules apply to every redirect hop of `submilli:http`,
//! through `/v1/execute` against a local server. `127.0.0.1` is the permitted
//! host and `localhost` (the same server, reached by another name) the denied
//! one, so a denied destination is observable as a mock that is never hit.

use std::path::Path;
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use httpmock::{Method, MockServer};
use interpreter::{ModulePath, PackageSourceModule, compile_package};
use serde_json::{Value, json};
use submilli_build::{ArtifactMetadata, write_package_artifact};
use submilli_server::blueprint::InMemoryBlueprintStore;
use submilli_server::{AppState, ServerConfig, app};
use tower::ServiceExt;

fn router(yaml: &str, package_store_root: Option<&Path>) -> Router {
    let blueprint = submilli_blueprint::parse(yaml).expect("valid blueprint");
    let config = ServerConfig {
        blueprints: Some(Arc::new(InMemoryBlueprintStore::seed([blueprint]))),
        package_store_root: package_store_root.map(Path::to_path_buf),
        ..ServerConfig::default()
    };
    app(AppState::new(config).expect("build AppState"))
}

async fn execute(router: Router, code: &str) -> Value {
    let body = json!({ "code": code, "blueprint": "redirects" }).to_string();
    let req = Request::builder()
        .method("POST")
        .uri("/v1/execute")
        .header("content-type", "application/json")
        .body(Body::from(body))
        .unwrap();
    let resp = router.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

fn blueprint(permissions: &str) -> String {
    format!("name: redirects\nallow_insecure_http: true\npermissions:\n{permissions}")
}

/// `http://localhost:<port>/<path>` on the same server as `server.url(path)`.
fn via_localhost(server: &MockServer, path: &str) -> String {
    format!("http://localhost:{}{path}", server.port())
}

/// A script that runs `call` and returns the denial's caller and capability, or
/// the response body when nothing is denied.
fn catching(imports: &str, call: &str) -> String {
    format!(
        r#"
import {{ {imports} }} from "submilli:http";
function main(): string {{
    try {{
        return {call};
    }} catch (e: PermissionDeniedError) {{
        return "denied " + e.caller + " " + e.capability;
    }}
}}
"#
    )
}

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_post_replayed_by_307_or_308_to_a_denied_host_is_never_sent() {
    let server = MockServer::start_async().await;
    let collect = server
        .mock_async(|when, then| {
            when.path("/collect");
            then.status(200).body("leaked");
        })
        .await;
    for status in [307, 308] {
        server
            .mock_async(|when, then| {
                when.path(format!("/start{status}"));
                then.status(status)
                    .header("location", via_localhost(&server, "/collect"));
            })
            .await;
        let yaml = blueprint(
            "  main:\n  - capability: http.post\n    filter: host == \"127.0.0.1\"\n    action: allow\n",
        );
        let code = catching(
            "post",
            &format!(
                "post(\"{}\", \"secret-body\").body",
                server.url(format!("/start{status}"))
            ),
        );
        let body = execute(router(&yaml, None), &code).await;
        assert_eq!(body["result"], json!("denied main http.post"), "{body}");
    }
    collect.assert_hits_async(0).await;
}

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn allowed_redirects_still_follow_and_rewritten_posts_need_http_get() {
    let server = MockServer::start_async().await;
    server
        .mock_async(|when, then| {
            when.path("/moved");
            then.status(302).header("location", "/final");
        })
        .await;
    server
        .mock_async(|when, then| {
            when.path("/submit");
            then.status(303).header("location", "/final");
        })
        .await;
    let final_hits = server
        .mock_async(|when, then| {
            when.method(Method::GET).path("/final");
            then.status(200).body("final");
        })
        .await;

    let get_rule =
        "  - capability: http.get\n    filter: host == \"127.0.0.1\"\n    action: allow\n";
    let post_rule =
        "  - capability: http.post\n    filter: host == \"127.0.0.1\"\n    action: allow\n";
    let moved = catching("get", &format!("get(\"{}\").body", server.url("/moved")));
    let body = execute(
        router(&blueprint(&format!("  main:\n{get_rule}")), None),
        &moved,
    )
    .await;
    assert_eq!(body["result"], json!("final"), "{body}");

    // A 303 turns the POST into a GET, which needs its own permission.
    let submit = catching(
        "post",
        &format!("post(\"{}\", \"form\").body", server.url("/submit")),
    );
    let body = execute(
        router(&blueprint(&format!("  main:\n{post_rule}")), None),
        &submit,
    )
    .await;
    assert_eq!(body["result"], json!("denied main http.get"), "{body}");
    let both = blueprint(&format!("  main:\n{post_rule}{get_rule}"));
    let body = execute(router(&both, None), &submit).await;
    assert_eq!(body["result"], json!("final"), "{body}");

    final_hits.assert_hits_async(2).await;
}

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn same_origin_redirects_respect_path_filters() {
    let server = MockServer::start_async().await;
    server
        .mock_async(|when, then| {
            when.path("/public/start");
            then.status(302).header("location", "/admin");
        })
        .await;
    let admin = server
        .mock_async(|when, then| {
            when.path("/admin");
            then.status(200).body("admin");
        })
        .await;
    let yaml = blueprint(
        "  main:\n  - capability: http.get\n    filter: host == \"127.0.0.1\" and path glob \"/public/*\"\n    action: allow\n",
    );
    let code = catching(
        "get",
        &format!("get(\"{}\").body", server.url("/public/start")),
    );
    let body = execute(router(&yaml, None), &code).await;
    assert_eq!(body["result"], json!("denied main http.get"), "{body}");
    admin.assert_hits_async(0).await;
}

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn downloads_check_each_redirect_hop() {
    let server = MockServer::start_async().await;
    server
        .mock_async(|when, then| {
            when.path("/file");
            then.status(302)
                .header("location", via_localhost(&server, "/payload"));
        })
        .await;
    let payload = server
        .mock_async(|when, then| {
            when.path("/payload");
            then.status(200).body("payload");
        })
        .await;
    let yaml = blueprint(
        "  main:\n  - capability: http.download\n    filter: host == \"127.0.0.1\"\n    action: allow\n  - capability: fs.write\n    action: allow\n  - capability: fs.stat\n    action: allow\n",
    );
    let code = format!(
        r#"
import {{ download }} from "submilli:http";
import {{ exists }} from "submilli:fs";
function main(): string {{
    try {{
        download("{}", "/out.bin");
        return "downloaded";
    }} catch (e: PermissionDeniedError) {{
        return "denied " + e.capability + " " + exists("/out.bin").toString();
    }}
}}
"#,
        server.url("/file")
    );
    let body = execute(router(&yaml, None), &code).await;
    assert_eq!(
        body["result"],
        json!("denied http.download false"),
        "{body}"
    );
    payload.assert_hits_async(0).await;
}

const FETCHER_SOURCE: &str = r#"
import { get } from "submilli:http";

/** The body served at `url`, after redirects. */
export function fetchBody(url: string): string {
    return get(url).body;
}
"#;

fn write_fetcher_package(store_root: &Path) {
    let package = compile_package(
        "@acme/fetcher",
        ModulePath::from("lib"),
        &[PackageSourceModule {
            path: ModulePath::from("lib"),
            source: FETCHER_SOURCE,
        }],
        &[],
    )
    .expect("compile @acme/fetcher");
    write_package_artifact(
        store_root.join("@acme").join("fetcher"),
        &package.wasm,
        &package.type_info,
        &submilli_build::derive_capability_schema(
            &package.declaration,
            &[],
            &package.required_capabilities,
        ),
        &package.declaration,
        &ArtifactMetadata::new("@acme/fetcher", "0.0.0-test", Vec::new()),
    )
    .expect("write @acme/fetcher artifact");
}

/// Hops are checked for the caller that made the request, and one caller's
/// allowed redirect grants nothing to another on the same session client.
#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn package_rules_apply_to_their_own_redirect_hops() {
    let server = MockServer::start_async().await;
    server
        .mock_async(|when, then| {
            when.path("/moved");
            then.status(302)
                .header("location", via_localhost(&server, "/elsewhere"));
        })
        .await;
    let elsewhere = server
        .mock_async(|when, then| {
            when.path("/elsewhere");
            then.status(200).body("elsewhere");
        })
        .await;
    let store = tempfile::tempdir().expect("store tempdir");
    write_fetcher_package(store.path());
    let restricted = "    filter: host == \"127.0.0.1\"\n    action: allow\n";
    let run = |package_rule: String, main_rule: String| {
        let yaml = format!(
            "name: redirects\nallow_insecure_http: true\npackages:\n  - \"@acme/fetcher\"\npermissions:\n  \"@acme/fetcher\":\n  - capability: http.get\n{package_rule}  main:\n  - capability: http.get\n{main_rule}"
        );
        let code = format!(
            r#"
import {{ get }} from "submilli:http";
import {{ fetchBody }} from "@acme/fetcher";
function main(): string {{
    let out = "";
    try {{
        out = fetchBody("{moved}");
    }} catch (e: PermissionDeniedError) {{
        out = "denied " + e.caller;
    }}
    try {{
        return out + " / " + get("{moved}").body;
    }} catch (e: PermissionDeniedError) {{
        return out + " / denied " + e.caller;
    }}
}}
"#,
            moved = server.url("/moved")
        );
        let router = router(&yaml, Some(store.path()));
        async move { execute(router, &code).await }
    };

    let body = run(restricted.to_string(), "    action: allow\n".to_string()).await;
    assert_eq!(
        body["result"],
        json!("denied @acme/fetcher / elsewhere"),
        "{body}"
    );
    let body = run("    action: allow\n".to_string(), restricted.to_string()).await;
    assert_eq!(body["result"], json!("elsewhere / denied main"), "{body}");
    elsewhere.assert_hits_async(2).await;
}
