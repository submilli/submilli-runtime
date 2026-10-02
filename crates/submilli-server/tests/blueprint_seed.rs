//! Boot-time reconcile of the blueprint store against a read-only seed
//! directory.
//!
//! The cases that carry the design are the ones about *repeat* boots: a pod
//! restarts far more often than it is installed, so "seeding twice is a no-op"
//! and "an API edit loses to the seed" are the properties that decide whether
//! the mechanism is usable, not the first-install happy path.

use std::fs;
use std::path::{Path, PathBuf};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::Value;
use submilli_blueprint::{AuthError, SecretResolver, SecretSource};
use submilli_server::blueprint::{BlueprintStore, FileBlueprintStore, StoredBlueprint};
use submilli_server::blueprint_seed::seed_blueprints;
use submilli_server::config::{VolumeSpec, VolumeTable};
use submilli_server::{AppState, ServerConfig, app};
use tempfile::TempDir;
use tower::ServiceExt;

/// Resolves `env:`/`file:` optimistically and always fails `store:`, standing in
/// for a server whose secret store has not been populated yet.
struct StoreAlwaysMissing;

#[async_trait::async_trait]
impl SecretResolver for StoreAlwaysMissing {
    async fn resolve(&self, name: &str, source: &SecretSource) -> Result<String, AuthError> {
        match source {
            SecretSource::Store(_) => Err(AuthError::MissingSecret(name.to_string())),
            _ => Ok("value".into()),
        }
    }
}

struct AlwaysResolves;

#[async_trait::async_trait]
impl SecretResolver for AlwaysResolves {
    async fn resolve(&self, _name: &str, _source: &SecretSource) -> Result<String, AuthError> {
        Ok("value".into())
    }
}

fn write(dir: &Path, file: &str, contents: &str) {
    fs::write(dir.join(file), contents).expect("writing seed file");
}

/// The table most cases seed against: only the `named`-mode cases care
/// what is declared.
fn no_volumes() -> VolumeTable {
    VolumeTable::new()
}

/// Count the immutable revision files the store has written for `name`. The
/// store never mutates or deletes one, so this is the honest measure of "did
/// that boot append anything".
fn revisions(store_dir: &Path, name: &str) -> usize {
    fs::read_dir(store_dir)
        .expect("reading store dir")
        .filter_map(Result::ok)
        .filter(|e| {
            let file = e.file_name();
            let file = file.to_string_lossy();
            file.starts_with(&format!("{name}.")) && file.ends_with(".yaml")
        })
        .count()
}

fn store_at(dir: &Path) -> FileBlueprintStore {
    FileBlueprintStore::new(dir.to_path_buf()).expect("opening store")
}

/// The seed routine is only useful if `boot` actually calls it, and `boot` is
/// what `serve` awaits before binding the listener. Driving this through the
/// router rather than the store closes the same gap the memory-cap test exists
/// for: a correct routine nothing invokes is indistinguishable from no routine.
async fn seeded_names_over_the_api(seed_dir: Option<PathBuf>, volumes: VolumeTable) -> Vec<String> {
    let store_dir = TempDir::new().unwrap();
    let state = AppState::new(ServerConfig {
        blueprint_dir: Some(store_dir.path().to_path_buf()),
        blueprint_seed_dir: seed_dir,
        volumes,
        ..ServerConfig::default()
    })
    .expect("building app state");
    state.boot().await;

    let response = app(state)
        .oneshot(
            Request::builder()
                .uri("/v1/blueprints")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body).unwrap();
    json["blueprints"]
        .as_array()
        .expect("blueprints array")
        .iter()
        .map(|b| b["name"].as_str().expect("name").to_string())
        .collect()
}

#[tokio::test]
async fn seeds_an_empty_store_from_the_directory() {
    let seed = TempDir::new().unwrap();
    let store_dir = TempDir::new().unwrap();
    write(seed.path(), "demo.yaml", "name: demo\n");
    write(seed.path(), "other.yaml", "name: other\n");

    let store = store_at(store_dir.path());
    let outcome = seed_blueprints(&store, &AlwaysResolves, seed.path(), &no_volumes()).await;

    assert_eq!(outcome.seeded, 2);
    assert_eq!(outcome.skipped, 0);
    assert_eq!(outcome.failed, 0);
    assert_eq!(store.list().await, vec!["demo", "other"]);
    assert_eq!(revisions(store_dir.path(), "demo"), 1);
}

#[tokio::test]
async fn a_second_boot_against_an_unchanged_directory_appends_no_revisions() {
    let seed = TempDir::new().unwrap();
    let store_dir = TempDir::new().unwrap();
    write(seed.path(), "demo.yaml", "name: demo\n");

    let first = seed_blueprints(
        &store_at(store_dir.path()),
        &AlwaysResolves,
        seed.path(),
        &no_volumes(),
    )
    .await;
    assert_eq!(first.seeded, 1);

    // A fresh store over the same directory is what a restarted pod sees.
    let second = seed_blueprints(
        &store_at(store_dir.path()),
        &AlwaysResolves,
        seed.path(),
        &no_volumes(),
    )
    .await;

    assert_eq!(
        second.seeded, 0,
        "an unchanged seed file must not re-register"
    );
    assert_eq!(second.skipped, 1);
    assert_eq!(
        revisions(store_dir.path(), "demo"),
        1,
        "a restart loop would otherwise append a revision per boot and fill the volume",
    );
}

#[tokio::test]
async fn an_edited_seed_file_produces_one_new_revision_for_that_blueprint_only() {
    let seed = TempDir::new().unwrap();
    let store_dir = TempDir::new().unwrap();
    write(seed.path(), "demo.yaml", "name: demo\n");
    write(seed.path(), "other.yaml", "name: other\n");
    seed_blueprints(
        &store_at(store_dir.path()),
        &AlwaysResolves,
        seed.path(),
        &no_volumes(),
    )
    .await;

    write(seed.path(), "demo.yaml", "name: demo\nvfs:\n  mode: none\n");
    let outcome = seed_blueprints(
        &store_at(store_dir.path()),
        &AlwaysResolves,
        seed.path(),
        &no_volumes(),
    )
    .await;

    assert_eq!(outcome.seeded, 1);
    assert_eq!(outcome.skipped, 1);
    assert_eq!(revisions(store_dir.path(), "demo"), 2);
    assert_eq!(revisions(store_dir.path(), "other"), 1);
}

#[tokio::test]
async fn a_blueprint_absent_from_the_seed_directory_is_retained() {
    let seed = TempDir::new().unwrap();
    let store_dir = TempDir::new().unwrap();
    write(seed.path(), "demo.yaml", "name: demo\n");

    let store = store_at(store_dir.path());
    let hand_made = submilli_blueprint::parse("name: manual\n").unwrap();
    store
        .upsert_yaml(StoredBlueprint::new(hand_made, "name: manual\n".into()))
        .await
        .unwrap();

    seed_blueprints(&store, &AlwaysResolves, seed.path(), &no_volumes()).await;

    assert_eq!(
        store.list().await,
        vec!["demo", "manual"],
        "the seed must never prune — it cannot tell a deleted source from an API-created blueprint",
    );
}

#[tokio::test]
async fn an_api_edit_to_a_seeded_blueprint_is_reverted_on_the_next_boot() {
    let seed = TempDir::new().unwrap();
    let store_dir = TempDir::new().unwrap();
    write(seed.path(), "demo.yaml", "name: demo\n");
    seed_blueprints(
        &store_at(store_dir.path()),
        &AlwaysResolves,
        seed.path(),
        &no_volumes(),
    )
    .await;

    let edited = "name: demo\nvfs:\n  mode: none\n";
    let store = store_at(store_dir.path());
    store
        .upsert_yaml(StoredBlueprint::new(
            submilli_blueprint::parse(edited).unwrap(),
            edited.into(),
        ))
        .await
        .unwrap();
    assert_eq!(store.get_yaml("demo").await.as_deref(), Some(edited));

    let store = store_at(store_dir.path());
    seed_blueprints(&store, &AlwaysResolves, seed.path(), &no_volumes()).await;

    assert_eq!(
        store.get_yaml("demo").await.as_deref(),
        Some("name: demo\n"),
        "the seed directory owns a seeded blueprint; this is the documented accepted cost",
    );
    assert_eq!(
        revisions(store_dir.path(), "demo"),
        3,
        "the overwritten edit stays on disk as history, so the revert is auditable",
    );
}

#[tokio::test]
async fn an_api_deletion_of_a_seeded_blueprint_is_undone_on_the_next_boot() {
    let seed = TempDir::new().unwrap();
    let store_dir = TempDir::new().unwrap();
    write(seed.path(), "demo.yaml", "name: demo\n");
    seed_blueprints(
        &store_at(store_dir.path()),
        &AlwaysResolves,
        seed.path(),
        &no_volumes(),
    )
    .await;

    let store = store_at(store_dir.path());
    assert!(store.remove("demo").await.unwrap());
    assert!(store.list().await.is_empty());

    let store = store_at(store_dir.path());
    let outcome = seed_blueprints(&store, &AlwaysResolves, seed.path(), &no_volumes()).await;

    assert_eq!(outcome.seeded, 1);
    assert_eq!(
        store.list().await,
        vec!["demo"],
        "removing a seeded blueprint requires editing the source, not calling the API",
    );
}

#[tokio::test]
async fn a_malformed_seed_file_is_skipped_and_the_rest_are_seeded() {
    let seed = TempDir::new().unwrap();
    let store_dir = TempDir::new().unwrap();
    write(seed.path(), "broken.yaml", "name: [this is not a name\n");
    write(seed.path(), "good.yaml", "name: good\n");

    let store = store_at(store_dir.path());
    let outcome = seed_blueprints(&store, &AlwaysResolves, seed.path(), &no_volumes()).await;

    assert_eq!(outcome.seeded, 1);
    assert_eq!(outcome.failed, 1);
    assert_eq!(store.list().await, vec!["good"]);
}

#[tokio::test]
async fn configmap_symlink_entries_are_ignored() {
    let seed = TempDir::new().unwrap();
    let store_dir = TempDir::new().unwrap();
    // The shape the kubelet actually materialises: the real files live in a
    // timestamped directory, `..data` links to it, and each key is a symlink
    // through `..data`. `ls` hides the dot entries; `read_dir` does not.
    let data = seed.path().join("..2026_08_10_09_15_00.318239");
    fs::create_dir(&data).unwrap();
    fs::write(data.join("demo.yaml"), "name: demo\n").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&data, seed.path().join("..data")).unwrap();
    write(seed.path(), "demo.yaml", "name: demo\n");
    // A dot-prefixed *regular* file is what distinguishes the dotfile guard from
    // the is_file() check — the `..data` entries above are directories, which
    // is_file() excludes on its own.
    write(seed.path(), ".hidden.yaml", "name: hidden\n");

    let store = store_at(store_dir.path());
    let outcome = seed_blueprints(&store, &AlwaysResolves, seed.path(), &no_volumes()).await;

    assert_eq!(
        outcome.seeded, 1,
        "the dot entries must not be read as blueprints",
    );
    assert_eq!(outcome.failed, 0);
    assert_eq!(store.list().await, vec!["demo"]);
}

#[tokio::test]
async fn an_entry_without_a_yaml_suffix_is_ignored() {
    let seed = TempDir::new().unwrap();
    let store_dir = TempDir::new().unwrap();
    // The natural ConfigMap key shape, and the silent-failure this guards.
    write(seed.path(), "demo", "name: demo\n");
    write(seed.path(), "real.yaml", "name: real\n");

    let store = store_at(store_dir.path());
    let outcome = seed_blueprints(&store, &AlwaysResolves, seed.path(), &no_volumes()).await;

    assert_eq!(outcome.seeded, 1);
    assert_eq!(store.list().await, vec!["real"]);
}

#[tokio::test]
async fn a_blueprint_with_an_unresolvable_secret_is_still_seeded() {
    let seed = TempDir::new().unwrap();
    let store_dir = TempDir::new().unwrap();
    write(
        seed.path(),
        "demo.yaml",
        "name: demo\nsecrets:\n  API_KEY: { store: mcp/demo/key }\n",
    );

    let store = store_at(store_dir.path());
    let outcome = seed_blueprints(&store, &StoreAlwaysMissing, seed.path(), &no_volumes()).await;

    assert_eq!(
        outcome.seeded, 1,
        "the secret store is routinely populated after first boot; failing here is a crash loop",
    );
    assert_eq!(outcome.unresolved_secrets, 1);
    assert_eq!(outcome.failed, 0);
    assert_eq!(store.list().await, vec!["demo"]);
}

#[tokio::test]
async fn a_missing_seed_directory_does_not_stop_the_server() {
    let store_dir = TempDir::new().unwrap();
    let store = store_at(store_dir.path());

    let outcome = seed_blueprints(
        &store,
        &AlwaysResolves,
        Path::new("/nonexistent/seed"),
        &no_volumes(),
    )
    .await;

    assert_eq!(outcome.seeded, 0);
    assert_eq!(outcome.failed, 1);
    assert!(store.list().await.is_empty());
}

/// The summary line is the only diagnostic an operator gets, so a file that
/// cannot be read has to reach it. A silent skip reads as "nothing to do".
#[cfg(unix)]
#[tokio::test]
async fn an_unreadable_seed_file_is_counted_not_silently_dropped() {
    use std::os::unix::fs::PermissionsExt;

    let seed = TempDir::new().unwrap();
    let store_dir = TempDir::new().unwrap();
    write(seed.path(), "locked.yaml", "name: locked\n");
    write(seed.path(), "good.yaml", "name: good\n");
    fs::set_permissions(
        seed.path().join("locked.yaml"),
        fs::Permissions::from_mode(0o000),
    )
    .unwrap();

    let store = store_at(store_dir.path());
    let outcome = seed_blueprints(&store, &AlwaysResolves, seed.path(), &no_volumes()).await;

    assert_eq!(outcome.seeded, 1);
    assert_eq!(
        outcome.failed, 1,
        "an unreadable file must show up in the count"
    );
    assert_eq!(store.list().await, vec!["good"]);
}

#[tokio::test]
async fn the_first_seed_file_by_name_wins_a_duplicate_name() {
    let seed = TempDir::new().unwrap();
    let store_dir = TempDir::new().unwrap();
    write(seed.path(), "a.yaml", "name: demo\n");
    write(seed.path(), "b.yaml", "name: demo\nvfs:\n  mode: none\n");

    let store = store_at(store_dir.path());
    let outcome = seed_blueprints(&store, &AlwaysResolves, seed.path(), &no_volumes()).await;

    assert_eq!(outcome.seeded, 1);
    assert_eq!(outcome.failed, 1);
    assert_eq!(
        store.get_yaml("demo").await.as_deref(),
        Some("name: demo\n"),
        "sorted order decides, so the winner is stable across mounts rather than read_dir order",
    );
}

#[tokio::test]
async fn boot_seeds_the_store_so_the_api_serves_the_blueprint() {
    let seed = TempDir::new().unwrap();
    write(seed.path(), "demo.yaml", "name: demo\n");

    let names = seeded_names_over_the_api(Some(seed.path().to_path_buf()), no_volumes()).await;

    assert_eq!(names, vec!["demo"]);
}

#[tokio::test]
async fn boot_without_a_seed_directory_registers_nothing() {
    let names = seeded_names_over_the_api(None, no_volumes()).await;

    assert!(
        names.is_empty(),
        "seeding is opt-in; an unset directory must leave boot exactly as it was",
    );
}

/// The seed channel enforces the same rule as the API — that is the point of
/// R4 — but with the seed's consequence: record the failure and carry on.
#[tokio::test]
async fn a_seed_file_naming_an_undeclared_volume_is_not_registered() {
    let seed = TempDir::new().unwrap();
    let store_dir = TempDir::new().unwrap();
    write(
        seed.path(),
        "escaper.yaml",
        "name: escaper\nvfs:\n  mode: named\n  volume: unknown-vol\n",
    );
    write(seed.path(), "good.yaml", "name: good\n");
    let volumes = VolumeTable::from([(
        "project-alpha".to_string(),
        VolumeSpec::local_path("/srv/project-alpha"),
    )]);

    let store = store_at(store_dir.path());
    let outcome = seed_blueprints(&store, &AlwaysResolves, seed.path(), &volumes).await;

    assert_eq!(outcome.seeded, 1);
    assert_eq!(outcome.failed, 1);
    assert_eq!(store.list().await, vec!["good"]);
}

#[tokio::test]
async fn a_seed_file_naming_a_declared_volume_is_registered() {
    let seed = TempDir::new().unwrap();
    let store_dir = TempDir::new().unwrap();
    write(
        seed.path(),
        "worker.yaml",
        "name: worker\nvfs:\n  mode: named\n  volume: project-alpha\n",
    );
    let volumes = VolumeTable::from([(
        "project-alpha".to_string(),
        VolumeSpec::local_path("/srv/project-alpha"),
    )]);

    let store = store_at(store_dir.path());
    let outcome = seed_blueprints(&store, &AlwaysResolves, seed.path(), &volumes).await;

    assert_eq!(outcome.seeded, 1);
    assert_eq!(outcome.failed, 0);
    assert_eq!(store.list().await, vec!["worker"]);
}

#[tokio::test]
async fn unnamed_seed_files_are_unaffected_by_the_volume_check() {
    let seed = TempDir::new().unwrap();
    let store_dir = TempDir::new().unwrap();
    write(seed.path(), "a.yaml", "name: a\nvfs: none\n");
    write(seed.path(), "b.yaml", "name: b\nvfs: ephemeral\n");
    write(seed.path(), "c.yaml", "name: c\nvfs: per_session\n");

    let store = store_at(store_dir.path());
    let outcome = seed_blueprints(&store, &AlwaysResolves, seed.path(), &no_volumes()).await;

    assert_eq!(outcome.seeded, 3);
    assert_eq!(outcome.failed, 0);
    assert_eq!(store.list().await, vec!["a", "b", "c"]);
}

/// A read-only ConfigMap can hold a blueprint no channel will accept. Booting is
/// still the only useful behavior: the operator gets a log line, and every other
/// seed file lands.
#[tokio::test]
async fn the_server_boots_past_seed_files_it_cannot_register() {
    let seed = TempDir::new().unwrap();
    write(
        seed.path(),
        "retired.yaml",
        "name: retired\nvfs:\n  mode: persistent\n  path: /\n",
    );
    write(
        seed.path(),
        "undeclared.yaml",
        "name: undeclared\nvfs:\n  mode: named\n  volume: unknown-vol\n",
    );
    write(
        seed.path(),
        "worker.yaml",
        "name: worker\nvfs:\n  mode: named\n  volume: project-alpha\n",
    );
    write(seed.path(), "plain.yaml", "name: plain\n");
    let volumes = VolumeTable::from([(
        "project-alpha".to_string(),
        VolumeSpec::local_path("/srv/project-alpha"),
    )]);

    let names = seeded_names_over_the_api(Some(seed.path().to_path_buf()), volumes).await;

    assert_eq!(
        names,
        vec!["plain", "worker"],
        "the retired `path:` form and the undeclared volume must not reach the store",
    );
}
