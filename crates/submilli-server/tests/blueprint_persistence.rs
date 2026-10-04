//! File-backed blueprint store: versioned, crash-safe persistence. Revisions
//! are append-only `<name>.<rev>.yaml` files; `index.json` points at the
//! current revision of each active blueprint. Tests exercise durability across
//! restarts, history retention, and crash-recovery semantics — both directly
//! against `FileBlueprintStore` and through the HTTP surface.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use submilli_blueprint::Blueprint;
use submilli_server::blueprint::{BlueprintStore, FileBlueprintStore, StoreError};
use submilli_server::{AppState, ServerConfig, app};
use tower::ServiceExt;

static COUNTER: AtomicU32 = AtomicU32::new(0);

/// A fresh, empty temp directory unique to each test invocation.
fn temp_dir() -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let dir = std::env::temp_dir().join(format!("submilli-bp-{pid}-{n}"));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

fn bp(name: &str) -> Blueprint {
    Blueprint {
        name: name.to_string(),
        ..Default::default()
    }
}

fn exists(dir: &Path, file: &str) -> bool {
    dir.join(file).exists()
}

#[tokio::test]
async fn add_writes_revision_and_index_and_persists_across_restart() {
    let dir = temp_dir();
    let store = FileBlueprintStore::new(dir.clone()).expect("new store");
    store.add(bp("production")).await.expect("add");

    assert!(exists(&dir, "production.000001.yaml"), "revision file");
    assert!(exists(&dir, "index.json"), "index file");
    assert!(
        fs::read_to_string(dir.join("production.000001.yaml"))
            .unwrap()
            .contains("name: production")
    );

    // Simulate a restart: drop the store, rebuild over the same dir.
    drop(store);
    let reloaded = FileBlueprintStore::new(dir).expect("reload store");
    assert_eq!(
        reloaded.list().await.expect("read blueprint store"),
        vec!["production".to_string()]
    );
    assert_eq!(
        reloaded
            .get("production")
            .await
            .expect("read blueprint store"),
        Some(bp("production"))
    );
}

#[tokio::test]
async fn apply_appends_revisions_and_keeps_history() {
    let dir = temp_dir();
    let store = FileBlueprintStore::new(dir.clone()).expect("new store");

    assert!(store.upsert(bp("x")).await.expect("create"));
    assert!(!store.upsert(bp("x")).await.expect("replace"));
    assert!(!store.upsert(bp("x")).await.expect("replace again"));

    // Every apply left an immutable revision behind.
    assert!(exists(&dir, "x.000001.yaml"));
    assert!(exists(&dir, "x.000002.yaml"));
    assert!(exists(&dir, "x.000003.yaml"));

    // The reloaded store sees the latest revision as current.
    drop(store);
    let reloaded = FileBlueprintStore::new(dir.clone()).expect("reload");
    assert_eq!(
        reloaded.get("x").await.expect("read blueprint store"),
        Some(bp("x"))
    );
    let index: serde_json::Value =
        serde_json::from_slice(&fs::read(dir.join("index.json")).unwrap()).unwrap();
    assert_eq!(index["x"], json!(3));
}

#[tokio::test]
async fn add_duplicate_is_already_exists() {
    let store = FileBlueprintStore::new(temp_dir()).expect("new store");
    store.add(bp("dup")).await.expect("first add");
    let err = store.add(bp("dup")).await.expect_err("second add");
    assert!(matches!(err, StoreError::AlreadyExists));
}

#[tokio::test]
async fn remove_drops_from_active_set_but_keeps_history_and_continues_numbering() {
    let dir = temp_dir();
    let store = FileBlueprintStore::new(dir.clone()).expect("new store");
    store.add(bp("gone")).await.expect("add");

    assert!(store.remove("gone").await.expect("remove"));
    assert_eq!(store.get("gone").await.expect("read blueprint store"), None);
    assert!(store.list().await.expect("read blueprint store").is_empty());
    // History is retained; the index no longer references it.
    assert!(exists(&dir, "gone.000001.yaml"), "history kept");
    let index: serde_json::Value =
        serde_json::from_slice(&fs::read(dir.join("index.json")).unwrap()).unwrap();
    assert!(index.get("gone").is_none());

    assert!(!store.remove("gone").await.expect("remove again"));

    // Re-adding continues numbering past the retained history.
    store.add(bp("gone")).await.expect("re-add");
    assert!(exists(&dir, "gone.000002.yaml"));
    assert!(exists(&dir, "gone.000001.yaml"), "old revision still there");
}

#[tokio::test]
async fn malformed_current_revision_is_skipped_on_boot() {
    let dir = temp_dir();
    let store = FileBlueprintStore::new(dir.clone()).expect("new store");
    store.add(bp("good")).await.expect("add good");
    store.add(bp("bad")).await.expect("add bad");
    drop(store);

    // Corrupt the revision the index points at for `bad`.
    fs::write(dir.join("bad.000001.yaml"), "this: is: not: valid:\n").unwrap();

    let reloaded = FileBlueprintStore::new(dir).expect("boots despite corrupt revision");
    assert_eq!(
        reloaded.list().await.expect("read blueprint store"),
        vec!["good".to_string()]
    );
}

#[tokio::test]
async fn orphan_revision_without_index_is_ignored_and_number_not_reused() {
    let dir = temp_dir();
    // Simulate a crash between writing a revision and committing the index:
    // an immutable revision file with no index entry pointing at it.
    fs::write(dir.join("x.000005.yaml"), "name: x\n").unwrap();

    let store = FileBlueprintStore::new(dir.clone()).expect("new store");
    assert!(
        store.list().await.expect("read blueprint store").is_empty(),
        "orphan is not active"
    );

    // A fresh add must not reuse the orphan's number.
    store.add(bp("x")).await.expect("add");
    assert!(
        exists(&dir, "x.000006.yaml"),
        "numbering continues past orphan"
    );
    assert!(exists(&dir, "x.000005.yaml"), "orphan retained");
    assert_eq!(
        store.get("x").await.expect("read blueprint store"),
        Some(bp("x"))
    );
}

#[tokio::test]
async fn no_temp_files_left_behind() {
    let dir = temp_dir();
    let store = FileBlueprintStore::new(dir.clone()).expect("new store");
    store.add(bp("a")).await.expect("add");
    store.upsert(bp("a")).await.expect("apply");
    store.remove("a").await.expect("remove");

    let leftover: Vec<_> = fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".tmp") || n.starts_with('.'))
        .collect();
    assert!(leftover.is_empty(), "unexpected temp files: {leftover:?}");
}

async fn get_json(router: &Router, path: &str) -> (StatusCode, Value) {
    let req = Request::builder()
        .method("GET")
        .uri(path)
        .body(Body::empty())
        .unwrap();
    let resp = router.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

#[tokio::test]
async fn http_add_survives_appstate_rebuild() {
    let dir = temp_dir();
    let router = app(AppState::new(ServerConfig {
        blueprint_dir: Some(dir.clone()),
        ..ServerConfig::default()
    })
    .expect("AppState"));

    let yaml = "\
name: production

# policy survives restart
permissions:
  main:
    - capability: fs.read
      action: allow

mcp:
  linear:
    # endpoint survives restart
    url: https://mcp.linear.app/mcp
";
    let req = Request::builder()
        .method("POST")
        .uri("/v1/blueprints")
        .header("content-type", "application/json")
        .body(Body::from(json!({ "yaml": yaml }).to_string()))
        .unwrap();
    assert_eq!(router.oneshot(req).await.unwrap().status(), StatusCode::OK);

    // Rebuild AppState over the same dir — the blueprint loads from disk.
    let router = app(AppState::new(ServerConfig {
        blueprint_dir: Some(dir),
        ..ServerConfig::default()
    })
    .expect("AppState reload"));
    let (status, body) = get_json(&router, "/v1/blueprints").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["blueprints"][0]["name"], json!("production"));
    assert_eq!(body["blueprints"][0]["mcp_servers"], json!(["linear"]));

    let (status, body) = get_json(&router, "/v1/blueprints/production").await;
    assert_eq!(status, StatusCode::OK);
    let shown = body["yaml"].as_str().expect("yaml string");
    assert!(shown.contains("# policy survives restart"), "{shown}");
    assert!(shown.contains("# endpoint survives restart"), "{shown}");
    assert!(
        shown.rfind("permissions:").unwrap() > shown.rfind("mcp:").unwrap(),
        "{shown}"
    );
}

/// A blueprint stored before the `persistent` vfs mode was replaced by `named`.
/// Its current revision no longer parses, which is the upgrade case R23 covers.
const RETIRED_FORM: &str = "\
name: tenant-alpha
vfs:
  mode: persistent
  volume: tenant-alpha
";

/// Plant `<name>.000001.yaml` + an index pointing at it, the on-disk shape a
/// pre-upgrade server left behind.
fn plant_revision(dir: &Path, name: &str, yaml: &str) {
    fs::write(dir.join(format!("{name}.000001.yaml")), yaml).unwrap();
    fs::write(dir.join("index.json"), json!({ name: 1 }).to_string()).unwrap();
}

fn index_of(dir: &Path) -> Value {
    serde_json::from_slice(&fs::read(dir.join("index.json")).unwrap()).unwrap()
}

#[tokio::test]
async fn retired_form_blueprint_keeps_its_name_reserved() {
    let dir = temp_dir();
    plant_revision(&dir, "tenant-alpha", RETIRED_FORM);

    let store = FileBlueprintStore::new(dir).expect("boots with a retired-form revision");

    // Out of the active set — it does not parse, so there is nothing to run.
    assert_eq!(
        store
            .get("tenant-alpha")
            .await
            .expect("read blueprint store"),
        None
    );
    assert!(store.list().await.expect("read blueprint store").is_empty());

    // But the name is not free for the next caller to claim.
    let err = store
        .add(bp("tenant-alpha"))
        .await
        .expect_err("name is reserved");
    assert!(matches!(err, StoreError::AlreadyExists));
}

/// The diagnostic an execute against a reserved name renders instead of
/// "unknown blueprint": it has to name the retired mode and its replacement.
#[tokio::test]
async fn retired_form_blueprint_reports_the_retired_mode_and_its_replacement() {
    let dir = temp_dir();
    plant_revision(&dir, "tenant-alpha", RETIRED_FORM);
    let store = FileBlueprintStore::new(dir).expect("new store");

    let reason = store
        .unusable_reason("tenant-alpha")
        .await
        .expect("read blueprint store")
        .expect("a reason, not silence");
    assert!(reason.contains("tenant-alpha"), "{reason}");
    assert!(
        reason.contains("`persistent` was removed"),
        "names the retired form: {reason}"
    );
    assert!(
        reason.contains("write `mode: named`"),
        "names the replacement: {reason}"
    );

    // The operator can still read what is stored, to fix it.
    assert_eq!(
        store
            .get_yaml("tenant-alpha")
            .await
            .expect("read blueprint store")
            .as_deref(),
        Some(RETIRED_FORM)
    );
}

#[tokio::test]
async fn healthy_and_unknown_names_have_no_reason() {
    let store = FileBlueprintStore::new(temp_dir()).expect("new store");
    store.add(bp("fine")).await.expect("add");
    assert_eq!(
        store
            .unusable_reason("fine")
            .await
            .expect("read blueprint store"),
        None
    );
    assert_eq!(
        store
            .unusable_reason("never-seen")
            .await
            .expect("read blueprint store"),
        None
    );
}

#[tokio::test]
async fn reserved_entry_survives_writes_to_other_names_and_reloads() {
    let dir = temp_dir();
    plant_revision(&dir, "tenant-alpha", RETIRED_FORM);
    let store = FileBlueprintStore::new(dir.clone()).expect("new store");

    store.add(bp("other")).await.expect("add another blueprint");

    // Rewriting the index for `other` must not drop the reserved name from it.
    assert_eq!(index_of(&dir)["tenant-alpha"], json!(1));
    assert_eq!(index_of(&dir)["other"], json!(1));

    drop(store);
    let reloaded = FileBlueprintStore::new(dir).expect("reload");
    assert!(
        reloaded
            .unusable_reason("tenant-alpha")
            .await
            .expect("read blueprint store")
            .is_some()
    );
    assert!(
        matches!(
            reloaded.add(bp("tenant-alpha")).await,
            Err(StoreError::AlreadyExists)
        ),
        "still reserved after a restart"
    );
}

#[tokio::test]
async fn re_registering_over_a_reserved_name_clears_the_reservation() {
    let dir = temp_dir();
    plant_revision(&dir, "tenant-alpha", RETIRED_FORM);
    let store = FileBlueprintStore::new(dir.clone()).expect("new store");

    let created = store
        .upsert(bp("tenant-alpha"))
        .await
        .expect("apply the fixed form");
    assert!(!created, "the name existed before, so this replaced it");

    assert_eq!(
        store
            .get("tenant-alpha")
            .await
            .expect("read blueprint store"),
        Some(bp("tenant-alpha"))
    );
    assert_eq!(
        store
            .unusable_reason("tenant-alpha")
            .await
            .expect("read blueprint store"),
        None
    );
    assert_eq!(
        store.list().await.expect("read blueprint store"),
        vec!["tenant-alpha".to_string()]
    );
    // Numbering continues past the retired revision, which is kept as history.
    assert!(exists(&dir, "tenant-alpha.000002.yaml"));
    assert!(exists(&dir, "tenant-alpha.000001.yaml"));
    assert_eq!(index_of(&dir)["tenant-alpha"], json!(2));
}

#[tokio::test]
async fn removing_a_reserved_name_frees_it() {
    let dir = temp_dir();
    plant_revision(&dir, "tenant-alpha", RETIRED_FORM);
    let store = FileBlueprintStore::new(dir.clone()).expect("new store");

    assert!(store.remove("tenant-alpha").await.expect("remove"));
    assert!(index_of(&dir).get("tenant-alpha").is_none());
    assert_eq!(
        store
            .unusable_reason("tenant-alpha")
            .await
            .expect("read blueprint store"),
        None
    );
    assert_eq!(
        store
            .get_yaml("tenant-alpha")
            .await
            .expect("read blueprint store"),
        None
    );

    store.add(bp("tenant-alpha")).await.expect("name is free");
    assert!(!store.remove("never-registered").await.expect("no-op"));
}

#[tokio::test]
async fn sqlx_import_and_http_changes_survive_restart_without_reimport() {
    use std::sync::Arc;
    use submilli_server::database::ServerDatabase;
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("blueprints");
    let files = FileBlueprintStore::new(source.clone()).unwrap();
    files.add(bp("tenant")).await.unwrap();
    let path = directory.path().join("db/server.db");
    let database = Arc::new(ServerDatabase::open(&path).await.unwrap());
    let store = submilli_server::blueprint::SqliteBlueprintStore::new(
        database.clone(),
        Some(source.clone()),
    );
    store.migrate().await.unwrap();
    let state = AppState::new(ServerConfig {
        blueprints: Some(Arc::new(store)),
        database: Some(database.clone()),
        blueprint_dir: Some(source.clone()),
        ..Default::default()
    })
    .unwrap();
    state.boot().await.unwrap();
    let router = app(state);
    let (status, _) = get_json(&router, "/v1/blueprints/tenant").await;
    assert_eq!(status, StatusCode::OK);
    let yaml = "# retained\nname: tenant\n";
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/v1/blueprints/tenant")
                .header("content-type", "application/json")
                .body(Body::from(json!({"yaml": yaml}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri("/v1/blueprints/tenant")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/v1/blueprints/tenant")
                .header("content-type", "application/json")
                .body(Body::from(json!({"yaml": yaml}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    drop(router);
    database.close().await.unwrap();

    let database = Arc::new(ServerDatabase::open(&path).await.unwrap());
    let store = submilli_server::blueprint::SqliteBlueprintStore::new(
        database.clone(),
        Some(source.clone()),
    );
    store.migrate().await.unwrap();
    let state = AppState::new(ServerConfig {
        blueprints: Some(Arc::new(store)),
        database: Some(database.clone()),
        blueprint_dir: Some(source.clone()),
        ..Default::default()
    })
    .unwrap();
    // The concrete store is migrated before the router can handle requests.
    let router = app(state);
    let (status, body) = get_json(&router, "/v1/blueprints/tenant").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["yaml"], yaml);
    assert_eq!(
        fs::read_to_string(directory.path().join("archive/blueprints/index.json"))
            .unwrap()
            .trim(),
        "{\n  \"tenant\": 1\n}"
    );
    drop(router);
    database.close().await.unwrap();
}
