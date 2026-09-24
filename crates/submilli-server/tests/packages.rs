//! REST package-discovery endpoint tests, in-process via `oneshot`.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::Value;
use submilli_server::{AppState, ServerConfig, app};
use tower::ServiceExt;

async fn get(uri: &str) -> (StatusCode, Value) {
    let state = AppState::new(ServerConfig::default()).expect("AppState");
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

#[tokio::test]
async fn search_lists_all_and_filters_by_symbol() {
    let (status, all) = get("/v1/packages/search").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(all["results"].as_array().unwrap().len(), 9, "got: {all}");

    let (_, hit) = get("/v1/packages/search?q=sha256").await;
    let names: Vec<&str> = hit["results"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, vec!["submilli:crypto"]);
}

#[tokio::test]
async fn docs_host_unknown_and_mcp() {
    let (status, doc) = get("/v1/packages/docs?name=submilli:http").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(doc["source"], "host");
    assert!(
        doc["declarations"]
            .as_str()
            .unwrap_or("")
            .contains("function get("),
        "got: {doc}"
    );

    let (status, code) = get("/v1/packages/docs?name=submilli:code").await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        code["declarations"]
            .as_str()
            .unwrap()
            .contains("function diffText(")
    );

    let (status, body) = get("/v1/packages/docs?name=nope").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"], "unknown_package");

    // `@mcp/<server>` docs are blueprint-scoped; the REST endpoint isn't bound to
    // one, so they resolve through the per-blueprint MCP tool, not here.
    let (status, body) = get("/v1/packages/docs?name=@mcp/linear").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"], "unknown_mcp_server");
}

#[tokio::test]
async fn docs_redirects_a_builtin_name_rather_than_erroring() {
    // Returned `404 unknown_package` before the cross-namespace redirect.
    let (status, doc) = get("/v1/packages/docs?name=Temporal").await;
    assert_eq!(status, StatusCode::OK, "got: {doc}");
    assert_eq!(doc["source"], "builtin");
    assert!(
        doc["declarations"]
            .as_str()
            .unwrap_or("")
            .starts_with("namespace Temporal {"),
        "got: {doc}"
    );
    // A `source` tag is a machine field; the prose has to say it too, or a
    // model reading the description writes an `import` for a global.
    let description = doc["description"].as_str().unwrap_or("");
    assert!(description.contains("import"), "got: {description}");
    assert!(description.contains("built-in"), "got: {description}");
}

#[tokio::test]
async fn docs_redirect_resolves_a_dotted_builtin_path() {
    // The composed case: the redirect has to run the path-aware lookup, not an
    // exact-name one, or the dot makes it miss.
    let (status, slice) = get("/v1/packages/docs?name=Temporal.Instant").await;
    assert_eq!(status, StatusCode::OK, "got: {slice}");
    assert_eq!(slice["source"], "builtin");
    let declarations = slice["declarations"].as_str().unwrap_or("");
    assert!(
        declarations.contains("interface InstantConstructor {"),
        "got: {declarations}"
    );
    assert!(
        !declarations.contains("interface ZonedDateTime {"),
        "got: {declarations}"
    );

    let (_, full) = get("/v1/packages/docs?name=Temporal").await;
    assert!(
        declarations.len() * 4 < full["declarations"].as_str().unwrap_or("").len(),
        "the slice is meant to be much smaller than the namespace"
    );
}

#[tokio::test]
async fn docs_miss_in_both_catalogs_suggests_the_closest_name() {
    let (status, body) = get("/v1/packages/docs?name=Temporel").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["did_you_mean"], "Temporal", "got: {body}");

    // Dropping the scheme is the likeliest package typo and sits 9 edits from
    // the full name — bare edit distance cannot reach it.
    let (status, body) = get("/v1/packages/docs?name=http").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["did_you_mean"], "submilli:http", "got: {body}");
}

#[tokio::test]
async fn docs_unknown_member_lists_the_members_that_exist() {
    let (status, body) = get("/v1/packages/docs?name=Temporal.Foo").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let message = body["message"].as_str().unwrap_or("");
    assert!(message.contains("Instant"), "got: {message}");
    assert!(message.contains("Now"), "got: {message}");
}

#[tokio::test]
async fn zero_hit_search_lists_what_is_available() {
    let (status, body) = get("/v1/packages/search?q=nothingmatchesthis").await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        body["results"].as_array().unwrap().is_empty(),
        "got: {body}"
    );

    let listed = body["available_packages"].as_array().expect("a catalog");
    assert!(!listed.is_empty(), "got: {body}");
    assert!(
        listed.iter().any(|e| e["name"] == "submilli:http"),
        "got: {body}"
    );
    // Summary-only: a pointer, not a payload.
    for entry in listed {
        assert!(entry["declarations"].is_null(), "got: {entry}");
        assert!(entry["name"].is_string() && entry["source"].is_string());
        assert!(entry["description"].as_str().unwrap_or("").len() > 1);
    }
    // Built-ins are named, not folded into results — folding them in would
    // teach a model to write an `import` for a global.
    // Over REST the pointer names the endpoint, not the MCP tool a REST caller
    // has no way to invoke.
    let pointer = body["builtins"].as_str().unwrap_or("");
    assert!(pointer.contains("/v1/builtins/docs"), "got: {pointer}");
    assert!(
        !listed.iter().any(|e| e["name"] == "Temporal"),
        "got: {body}"
    );
}

#[tokio::test]
async fn a_search_that_hits_is_unchanged() {
    let (_, body) = get("/v1/packages/search?q=sha256").await;
    assert_eq!(body["results"].as_array().unwrap().len(), 1, "got: {body}");
    assert!(body["available_packages"].is_null(), "got: {body}");
    assert!(body["builtins"].is_null(), "got: {body}");
}

#[tokio::test]
async fn builtins_catalog_and_docs() {
    let (status, cat) = get("/v1/builtins").await;
    assert_eq!(status, StatusCode::OK);
    let types: Vec<&str> = cat["types"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t.as_str().unwrap())
        .collect();
    assert!(
        types.contains(&"Array") && types.contains(&"Map"),
        "got: {cat}"
    );
    assert!(
        cat["namespaces"]
            .as_array()
            .unwrap()
            .iter()
            .any(|n| n == "Temporal"),
        "got: {cat}"
    );

    let (status, doc) = get("/v1/builtins/docs?name=Array").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(doc["name"], "Array");
    assert!(
        doc["declarations"]
            .as_str()
            .unwrap_or("")
            .contains("interface Array<"),
        "got: {doc}"
    );

    let (status, body) = get("/v1/builtins/docs?name=Promise").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"], "unknown_builtin");
}

/// The HTTP client’s `search`/`docs` tools ride these REST endpoints, so
/// without a blueprint scope an agent cannot see the packages its own
/// blueprint declares — only stdlib.
#[tokio::test]
async fn builtin_docs_names_the_packages_docs_call_for_a_package_name() {
    let (status, body) = get("/v1/builtins/docs?name=submilli:http").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"], "not_a_builtin", "got: {body}");
    // The correcting call names the REST route, since this is the REST surface.
    let message = body["message"].as_str().unwrap_or("");
    assert!(
        message.contains("/v1/packages/docs?name=submilli:http"),
        "got: {message}"
    );
    assert!(body["declarations"].is_null(), "got: {body}");
}

#[tokio::test]
async fn builtin_docs_resolves_a_dotted_path_and_reports_a_bad_member() {
    let (status, body) = get("/v1/builtins/docs?name=Temporal.Instant").await;
    assert_eq!(status, StatusCode::OK, "got: {body}");
    assert!(
        body["declarations"]
            .as_str()
            .unwrap_or("")
            .contains("interface InstantConstructor {"),
        "got: {body}"
    );

    let (status, body) = get("/v1/builtins/docs?name=Temporal.Foo").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(
        body["message"].as_str().unwrap_or("").contains("Instant"),
        "got: {body}"
    );
}

mod blueprint_scoped {
    use std::collections::BTreeSet;
    use std::path::Path;
    use std::sync::Arc;

    use axum::Router;
    use interpreter::{ModulePath, PackageSourceModule, compile_package};
    use submilli_blueprint::Blueprint;
    use submilli_build::{
        ArtifactMetadata, ArtifactSource, write_package_artifact_with_docs_and_sources,
    };
    use submilli_server::blueprint::InMemoryBlueprintStore;

    use super::*;

    const BLUEPRINT: &str = "scoped";

    fn write_package(store_root: &Path) {
        const SOURCE: &str = "export function ping(): number { return 1; }";
        let package = compile_package(
            "@acme/tools",
            ModulePath::from("lib"),
            &[PackageSourceModule {
                path: ModulePath::from("lib"),
                source: SOURCE,
            }],
            &[],
        )
        .expect("compile synthetic package");
        write_package_artifact_with_docs_and_sources(
            store_root.join("@acme").join("tools"),
            &package.wasm,
            &package.type_info,
            &submilli_build::derive_capability_schema(
                &package.declaration,
                &package.required_capabilities,
            ),
            &package.declaration,
            &ArtifactMetadata::new("@acme/tools", "0.0.0-test", Vec::new()),
            "Acme tooling for tests.",
            &[ArtifactSource {
                path: ModulePath::from("lib"),
                text: SOURCE.to_string(),
            }],
        )
        .expect("write synthetic package artifact");
    }

    fn router(store_root: &Path) -> Router {
        router_declaring(store_root, ["@acme/tools".to_string()])
    }

    /// A router whose blueprint declares `packages`, of which only
    /// `@acme/tools` has an artifact on disk.
    fn router_declaring(store_root: &Path, packages: impl IntoIterator<Item = String>) -> Router {
        write_package(store_root);
        let blueprints = Arc::new(InMemoryBlueprintStore::seed([Blueprint {
            name: BLUEPRINT.into(),
            packages: packages.into_iter().collect::<BTreeSet<String>>(),
            ..Default::default()
        }]));
        let config = ServerConfig {
            blueprints: Some(blueprints),
            package_store_root: Some(store_root.to_path_buf()),
            ..ServerConfig::default()
        };
        app(AppState::new(config).expect("build AppState"))
    }

    async fn get_from(router: Router, uri: &str) -> (StatusCode, Value) {
        let req = Request::builder()
            .method("GET")
            .uri(uri)
            .body(Body::empty())
            .unwrap();
        let resp = router.oneshot(req).await.unwrap();
        let status = resp.status();
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    async fn get_text_from(router: Router, uri: &str) -> (StatusCode, String, String) {
        let req = Request::builder()
            .method("GET")
            .uri(uri)
            .body(Body::empty())
            .unwrap();
        let resp = router.oneshot(req).await.unwrap();
        let status = resp.status();
        let content_type = resp
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_string();
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        (
            status,
            String::from_utf8_lossy(&bytes).to_string(),
            content_type,
        )
    }

    fn names(body: &Value) -> Vec<String> {
        body["results"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["name"].as_str().unwrap().to_string())
            .collect()
    }

    #[tokio::test]
    async fn search_includes_blueprint_packages_only_when_scoped() {
        let store = tempfile::tempdir().expect("tempdir");

        let (status, unscoped) = get_from(router(store.path()), "/v1/packages/search").await;
        assert_eq!(status, StatusCode::OK);
        assert!(
            !names(&unscoped).contains(&"@acme/tools".to_string()),
            "unscoped search must stay stdlib-only, got: {unscoped}"
        );

        let (status, scoped) =
            get_from(router(store.path()), "/v1/packages/search?blueprint=scoped").await;
        assert_eq!(status, StatusCode::OK);
        assert!(
            names(&scoped).contains(&"@acme/tools".to_string()),
            "scoped search must list the blueprint's packages, got: {scoped}"
        );

        // An unknown blueprint degrades to stdlib rather than erroring.
        let (status, unknown) =
            get_from(router(store.path()), "/v1/packages/search?blueprint=ghost").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(names(&unknown), names(&unscoped));
    }

    #[tokio::test]
    async fn a_zero_hit_scoped_search_lists_every_source_it_can_see() {
        let store = tempfile::tempdir().expect("tempdir");
        let (status, body) = get_from(
            router(store.path()),
            "/v1/packages/search?q=nothingmatchesthis&blueprint=scoped",
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(
            body["results"].as_array().unwrap().is_empty(),
            "got: {body}"
        );

        let listed = body["available_packages"].as_array().expect("a catalog");
        // The listing covers every source the call site can see, not just
        // the stdlib half `search_json_with_catalog` would have reached.
        assert!(
            listed.iter().any(|e| e["name"] == "submilli:http"),
            "got: {body}"
        );
        let registry = listed
            .iter()
            .find(|e| e["name"] == "@acme/tools")
            .unwrap_or_else(|| panic!("registry package missing from listing: {body}"));
        assert_eq!(registry["source"], "registry", "got: {registry}");
        assert!(body["builtins"].is_string(), "got: {body}");
    }

    #[tokio::test]
    async fn a_registry_only_hit_is_a_hit_not_a_miss() {
        let store = tempfile::tempdir().expect("tempdir");
        // Nothing in the stdlib matches `ping`, so the stdlib-and-mcp result set
        // is empty at the point `search_json_results` returns. The catalog must
        // not appear beside this hit — the case that fails if the miss branch is
        // placed inside `search_json_results` rather than after the extension.
        let (status, body) = get_from(
            router(store.path()),
            "/v1/packages/search?q=ping&blueprint=scoped",
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(names(&body), vec!["@acme/tools".to_string()], "got: {body}");
        assert!(body["available_packages"].is_null(), "got: {body}");
        assert!(body["builtins"].is_null(), "got: {body}");
    }

    #[tokio::test]
    async fn docs_resolve_blueprint_packages_only_when_scoped() {
        let store = tempfile::tempdir().expect("tempdir");

        let (status, _) =
            get_from(router(store.path()), "/v1/packages/docs?name=@acme/tools").await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        // Registry packages carry a readme, so — exactly like the MCP tool —
        // they come back as markdown text, not JSON with escaped newlines.
        let (status, body, content_type) = get_text_from(
            router(store.path()),
            "/v1/packages/docs?name=@acme/tools&blueprint=scoped",
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(
            content_type.starts_with("text/markdown"),
            "got: {content_type}"
        );
        assert!(body.contains("Acme tooling for tests."), "got: {body}");
        assert!(body.contains("function ping("), "got: {body}");
    }

    #[tokio::test]
    async fn docs_suggestions_draw_on_every_catalog_the_call_site_can_see() {
        let store = tempfile::tempdir().expect("tempdir");

        // Scoped: the blueprint's registry names join the candidate pool, so a
        // near-miss on one is recoverable.
        let (status, body) = get_from(
            router(store.path()),
            "/v1/packages/docs?name=@acme/tool&blueprint=scoped",
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["did_you_mean"], "@acme/tools", "got: {body}");

        // Unscoped, the same name has no registry catalog to be matched against.
        let (status, body) =
            get_from(router(store.path()), "/v1/packages/docs?name=@acme/tool").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert!(body["did_you_mean"].is_null(), "got: {body}");
    }

    #[tokio::test]
    async fn a_declared_package_missing_its_artifact_names_the_install() {
        let store = tempfile::tempdir().expect("tempdir");
        let (status, body) = get_from(
            router_declaring(
                store.path(),
                ["@acme/tools".to_string(), "@acme/absent".to_string()],
            ),
            "/v1/packages/docs?name=@acme/absent&blueprint=scoped",
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        // A declared-but-unfetched package is not an unknown name, and above all
        // must not be offered as its own did-you-mean.
        assert!(body["did_you_mean"].is_null(), "got: {body}");
        let message = body["message"].as_str().unwrap_or("");
        assert!(message.contains("submilli install"), "got: {body}");
    }

    #[tokio::test]
    async fn docs_redirect_does_not_shadow_a_declared_registry_package() {
        let store = tempfile::tempdir().expect("tempdir");
        // A built-in redirect must not preempt the package sources: exact hits
        // on every namespace stay exactly as they were.
        let (status, body, _) = get_text_from(
            router(store.path()),
            "/v1/packages/docs?name=@acme/tools&blueprint=scoped",
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("function ping("), "got: {body}");
    }
}
