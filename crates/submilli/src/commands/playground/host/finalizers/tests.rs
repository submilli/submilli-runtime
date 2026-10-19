use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use submilli_build::PackageStore;
use submilli_server::config::VolumeTable;

use super::super::super::state::StateDir;
use super::super::blueprints::AppliedBlueprint;
use super::super::run::{RunEntry, RunHostConfig, RunOutput, RunRequest, VfsSource};
use super::super::sessions::Sessions;
use super::super::volumes::Volumes;
use super::*;

const SOURCE: &str = r#"
import * as session from "submilli:session";
export interface Handle { id: string; fail: boolean; }
export function retain(key: string, fail: boolean): void {
    session.registerFinalizer(key, { id: key, fail });
}
export function release(key: string): void { session.unregisterFinalizer(key); }
export function finalizer(key: string, value: Handle): void {
    assert(key === value.id, "original local key and value");
    if (value.fail) { throw new Error("private-finalizer-failure-canary"); }
}
"#;

const POLICY: &str = "name: finalizers\ndefault: allow\npackages: ['@test/a', '@test/b']\nvfs:\n  mode: per_session\n";

const SECRET_POLICY: &str = "name: finalizers\ndefault: allow\npackages: ['@test/a']\nvfs:\n  mode: per_session\n\
     secrets:\n  TOKEN:\n    harness:\n      required: true\n";

/// The blueprint in force, which a test can change.
struct Current(Mutex<Option<Arc<Blueprint>>>);

impl CurrentBlueprint for Current {
    fn current(&self) -> BoxFuture<'_, Option<Arc<Blueprint>>> {
        let blueprint = self.0.lock().unwrap().clone();
        Box::pin(async move { blueprint })
    }
}

struct Host {
    dir: tempfile::TempDir,
    state: StateDir,
    sessions: Arc<Sessions>,
    host: Arc<RunHost>,
    blueprint: AppliedBlueprint,
    current: Arc<Current>,
}

fn store_root(dir: &Path) -> PathBuf {
    dir.join("store")
}

fn publish(root: &Path, name: &str, source: &str, version: &str) {
    publish_with(root, name, source, version, &[]);
}

fn publish_with(root: &Path, name: &str, source: &str, version: &str, dependencies: &[&str]) {
    let store = PackageStore::new(root.to_path_buf());
    let declarations: Vec<_> = dependencies
        .iter()
        .map(|dependency| store.load(dependency).unwrap().package_declaration)
        .collect();
    let refs: Vec<_> = declarations.iter().collect();
    let compiled = super::super::run::on_compiler_thread(|| {
        interpreter::compile_package(
            name,
            interpreter::ModulePath::from("lib"),
            &[interpreter::PackageSourceModule {
                path: interpreter::ModulePath::from("lib"),
                source,
            }],
            &refs,
        )
    })
    .unwrap()
    .unwrap();
    submilli_build::write_package_artifact(
        root.join(name),
        &compiled.wasm,
        &compiled.type_info,
        &submilli_build::derive_capability_schema(
            &compiled.declaration,
            &refs,
            &compiled.required_capabilities,
        ),
        &compiled.declaration,
        &submilli_build::ArtifactMetadata::new(
            name,
            version,
            dependencies
                .iter()
                .map(|dependency| submilli_build::ArtifactDependency::new(*dependency, "1"))
                .collect(),
        ),
    )
    .unwrap();
}

fn applied(yaml: &str) -> AppliedBlueprint {
    AppliedBlueprint {
        blueprint: Arc::new(submilli_blueprint::parse(yaml).unwrap()),
        yaml: yaml.to_owned(),
        version_tag: "v1".to_owned(),
    }
}

async fn host() -> Host {
    let dir = tempfile::tempdir().unwrap();
    for package in ["@test/a", "@test/b"] {
        publish(&store_root(dir.path()), package, SOURCE, "1");
    }
    boot(dir, POLICY, |_, _| {}).await
}

/// Boot a host over `dir`, its finalizers installed unless `configure` says not.
async fn boot(
    dir: tempfile::TempDir,
    yaml: &str,
    configure: impl FnOnce(&mut RunHostConfig, &mut Option<SessionKvLimits>),
) -> Host {
    boot_with(dir, yaml, true, configure).await
}

async fn boot_with(
    dir: tempfile::TempDir,
    yaml: &str,
    install: bool,
    configure: impl FnOnce(&mut RunHostConfig, &mut Option<SessionKvLimits>),
) -> Host {
    let state = StateDir::for_project(dir.path());
    let mut config = RunHostConfig::new(PackageStore::new(store_root(dir.path())));
    let mut limits = None;
    configure(&mut config, &mut limits);
    let mut sessions = Sessions::boot(&state).await.unwrap().sessions;
    if let Some(limits) = limits {
        sessions = sessions.with_limits(limits);
    }
    let sessions = Arc::new(sessions);
    config.finalizers = Some(Arc::new(sessions.finalizers().clone()));
    let volumes = Arc::new(Volumes::boot(&state, VolumeTable::new()));
    let host = Arc::new(RunHost::new(config, Arc::clone(&sessions), volumes).unwrap());
    let blueprint = applied(yaml);
    let current = Arc::new(Current(Mutex::new(Some(Arc::clone(&blueprint.blueprint)))));
    if install {
        HostFinalizers::install(&host, current.clone());
    }
    Host {
        dir,
        state,
        sessions,
        host,
        blueprint,
        current,
    }
}

impl Host {
    async fn open(&self) -> String {
        self.sessions
            .open(&self.blueprint.blueprint, &BTreeMap::new())
            .await
            .unwrap()
            .id
    }

    fn request(&self, session_id: &str, code: &str) -> RunRequest {
        RunRequest {
            session_id: session_id.to_owned(),
            code: code.to_owned(),
            label: "test".to_owned(),
            entry: RunEntry::Mcp,
            blueprint: self.blueprint.clone(),
            variables: Arc::new(VarBindings::new()),
            harness_secrets: Arc::new(HarnessSecretBindings::new()),
            client: None,
            tool_call_id: None,
            test_of: None,
            vfs: VfsSource::Session,
            finalizers: None,
            world: None,
        }
    }

    async fn run(&self, session_id: &str, code: &str) -> RunOutput {
        self.host.run(self.request(session_id, code)).await
    }

    async fn run_ok(&self, session_id: &str, code: &str) {
        let output = self.run(session_id, code).await;
        assert!(
            output.response.error.is_none(),
            "{:?}",
            output.response.error
        );
    }

    async fn rows(&self, session_id: &str) -> Vec<FinalizerRow> {
        self.sessions.finalizers().rows(session_id).await.unwrap()
    }

    /// Stop the host and start it again over the same project.
    async fn restart(self, install: bool) -> Host {
        let Host {
            dir,
            sessions,
            host,
            blueprint,
            ..
        } = self;
        sessions.close_database().await.unwrap();
        drop(host);
        drop(sessions);
        boot_with(dir, &blueprint.yaml, install, |_, _| {}).await
    }

    fn database_bytes(&self) -> Vec<u8> {
        let path = self.state.host_database_path();
        let mut bytes = std::fs::read(&path).unwrap_or_default();
        let mut wal = path.into_os_string();
        wal.push("-wal");
        bytes.extend(std::fs::read(wal).unwrap_or_default());
        bytes
    }
}

fn contains(haystack: &[u8], needle: &str) -> bool {
    haystack
        .windows(needle.len())
        .any(|part| part == needle.as_bytes())
}

async fn context_rows(sessions: &Sessions) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM finalizer_context")
        .fetch_one(&mut *sessions.finalizers().database.acquire().await.unwrap())
        .await
        .unwrap()
}

#[test]
fn only_interruption_can_redeliver_an_attempt() {
    for outcome in [Ok(()), Err(FinalizerFailure::Execution)] {
        let mut registration = Finalization::restore(FinalizationStatus::Registered);
        assert!(registration.terminate());
        assert!(!registration.terminate());
        assert!(registration.claim());
        assert!(registration.recover());
        assert!(registration.claim());
        assert!(registration.complete(outcome));
        assert!(!registration.recover());
        assert!(!registration.claim());
        assert!(!registration.complete(Ok(())));
    }
}

#[test]
fn a_finalizer_without_a_deadline_gets_thirty_seconds() {
    assert_eq!(deadline(None), Duration::from_secs(30));
    assert_eq!(deadline(Some(Duration::ZERO)), Duration::from_secs(30));
    assert_eq!(
        deadline(Some(Duration::from_secs(2))),
        Duration::from_secs(2)
    );
}

#[tokio::test]
async fn closing_runs_each_finalizer_once_and_a_failure_does_not_stop_the_close() {
    let host = host().await;
    let id = host.open().await;
    host.run_ok(
        &id,
        r#"
        import * as a from "@test/a"; import * as b from "@test/b";
        import * as session from "submilli:session";
        function main(): void {
            a.retain("same", false); b.retain("same", true);
            session.set("ordinary", { id: "same" }); session.remove("ordinary");
        }
    "#,
    )
    .await;
    let before = host.rows(&id).await;
    assert_eq!(before.len(), 2);
    assert!(before.iter().all(|row| row.status == "registered"));
    assert!(host.sessions.close(&id).await.unwrap());
    let after = host.rows(&id).await;
    assert!(
        after
            .iter()
            .any(|row| row.package == "@test/a" && row.status == "succeeded"),
        "{after:?}"
    );
    assert!(
        after.iter().any(|row| row.package == "@test/b"
            && row.status == "failed"
            && row.failure.as_deref() == Some("execution_failed")),
        "{after:?}"
    );
    assert!(!format!("{after:?}").contains("private-finalizer-failure-canary"));
    assert!(host.sessions.get(&id).await.unwrap().is_none());
    assert!(!host.sessions.session_dir(&id).exists());
    // Nothing runs a second time.
    host.sessions
        .reap(SystemTime::now() + Duration::from_secs(86_400))
        .await
        .unwrap();
    assert_eq!(host.rows(&id).await, after);
}

#[tokio::test]
async fn early_release_removes_only_the_owning_packages_key() {
    let host = host().await;
    let id = host.open().await;
    host.run_ok(&id, r#"
        import * as a from "@test/a"; import * as b from "@test/b";
        function main(): void { a.retain("same", false); b.retain("same", false); a.release("same"); a.release("absent"); }
    "#).await;
    let rows = host.rows(&id).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].package, "@test/b");
    host.sessions.close(&id).await.unwrap();
    assert_eq!(host.rows(&id).await[0].status, "succeeded");
}

#[tokio::test]
async fn programs_cannot_call_reserved_finalizers_or_register_as_a_package() {
    let host = host().await;
    let id = host.open().await;
    for code in [
        r#"import { finalizer } from "@test/a"; function main(): void { finalizer("k", { id: "k", fail: false }); }"#,
        r#"import * as session from "submilli:session"; function main(): void { session.registerFinalizer("k", 1); }"#,
        r#"import { unregisterFinalizer } from "submilli:session"; function main(): void { unregisterFinalizer("k"); }"#,
    ] {
        let output = host.run(&id, code).await;
        assert!(output.response.error.is_some(), "{code}");
    }
    assert!(host.rows(&id).await.is_empty());
}

#[tokio::test]
async fn a_package_without_a_validated_finalizer_cannot_register() {
    let host = host().await;
    let id = host.open().await;
    let provider = host
        .sessions
        .finalizers()
        .provider(&id, BTreeMap::new())
        .unwrap();
    let error = provider
        .register("@test/a", &"k".encode_utf16().collect::<Vec<_>>(), &[])
        .await
        .unwrap_err();
    assert!(
        error.to_string().contains("no validated finalizer"),
        "{error}"
    );
    assert!(host.rows(&id).await.is_empty());
}

#[tokio::test]
async fn duplicate_registration_is_catchable_and_keeps_the_original_value() {
    let host = host().await;
    let id = host.open().await;
    host.run_ok(
        &id,
        r#"
        import * as a from "@test/a";
        function main(): void {
            a.retain("same", false);
            let caught = false;
            try { a.retain("same", true); } catch (error: Error) { caught = true; }
            assert(caught);
        }
    "#,
    )
    .await;
    host.sessions.close(&id).await.unwrap();
    let rows = host.rows(&id).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].status, "succeeded");
}

#[tokio::test]
async fn registering_past_the_session_limit_is_refused_and_keeps_existing_cleanup() {
    let dir = tempfile::tempdir().unwrap();
    publish(&store_root(dir.path()), "@test/a", SOURCE, "1");
    let host = boot(dir, POLICY, |_, limits| {
        *limits = Some(SessionKvLimits {
            max_entries: 1,
            ..SessionKvLimits::default()
        });
    })
    .await;
    let id = host.open().await;
    host.run_ok(
        &id,
        r#"
        import { retain } from "@test/a";
        function main(): void {
            retain("first", false);
            let rejected = false;
            try { retain("second", false); } catch (error: Error) { rejected = true; }
            assert(rejected);
        }
    "#,
    )
    .await;
    host.sessions.close(&id).await.unwrap();
    let rows = host.rows(&id).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].key, "first");
    assert_eq!(rows[0].status, "succeeded");
}

#[tokio::test]
async fn unreadable_arguments_fail_only_their_registration() {
    let host = host().await;
    let id = host.open().await;
    host.run_ok(&id, r#"import { retain } from "@test/a"; function main(): void { retain("bad", false); retain("good", false); }"#).await;
    sqlx::query("UPDATE session_finalizers SET payload=x'01' WHERE local_key=?")
        .bind(units_to_bytes(&"bad".encode_utf16().collect::<Vec<_>>()))
        .execute(&mut *host.sessions.finalizers().database.acquire().await.unwrap())
        .await
        .unwrap();
    host.sessions.close(&id).await.unwrap();
    let rows = host.rows(&id).await;
    assert!(
        rows.iter()
            .any(|row| row.key == "bad" && row.status == "failed")
    );
    assert!(
        rows.iter()
            .any(|row| row.key == "good" && row.status == "succeeded")
    );
}

#[tokio::test]
async fn a_removed_or_upgraded_package_version_makes_cleanup_unavailable() {
    for installed in ["upgraded", "removed"] {
        let host = host().await;
        let id = host.open().await;
        host.run_ok(
            &id,
            r#"import { retain } from "@test/a"; function main(): void { retain("gone", false); }"#,
        )
        .await;
        let root = store_root(host.dir.path());
        match installed {
            "upgraded" => publish(&root, "@test/a", SOURCE, "2"),
            _ => std::fs::remove_dir_all(root.join("@test/a")).unwrap(),
        }
        host.sessions.close(&id).await.unwrap();
        let rows = host.rows(&id).await;
        assert_eq!(
            rows[0].failure.as_deref(),
            Some("artifact_unavailable"),
            "{installed}"
        );
    }
}

#[tokio::test]
async fn a_dependency_packages_finalizer_runs_against_its_registered_versions() {
    let dir = tempfile::tempdir().unwrap();
    let root = store_root(dir.path());
    publish(&root, "@test/a", SOURCE, "1");
    publish_with(
        &root,
        "@test/root",
        r#"import { retain } from "@test/a"; export function open(): void { retain("dependency", false); }"#,
        "1",
        &["@test/a"],
    );
    let host = boot(
        dir,
        "name: finalizers\ndefault: allow\npackages: ['@test/root']\nvfs:\n  mode: per_session\n",
        |_, _| {},
    )
    .await;
    let id = host.open().await;
    host.run_ok(
        &id,
        r#"import { open } from "@test/root"; function main(): void { open(); }"#,
    )
    .await;
    host.sessions.close(&id).await.unwrap();
    assert_eq!(host.rows(&id).await[0].status, "succeeded");
}

#[tokio::test]
async fn finalizers_cannot_register_more_cleanup() {
    let dir = tempfile::tempdir().unwrap();
    publish(
        &store_root(dir.path()),
        "@test/a",
        &SOURCE.replace(
            "assert(key === value.id,",
            "session.registerFinalizer(key + \"new\", value); assert(key === value.id,",
        ),
        "1",
    );
    let host = boot(dir, POLICY, |_, _| {}).await;
    let id = host.open().await;
    host.run_ok(&id, r#"import { retain } from "@test/a"; function main(): void { retain("no-recursion", false); }"#).await;
    host.sessions.close(&id).await.unwrap();
    let rows = host.rows(&id).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].status, "failed");
}

#[tokio::test]
async fn a_finalizer_spends_its_own_fuel_and_exhausting_it_does_not_block_another() {
    let dir = tempfile::tempdir().unwrap();
    let root = store_root(dir.path());
    publish(
        &root,
        "@test/a",
        &SOURCE.replace(
            "if (value.fail)",
            "while (key.length > 0) {} if (value.fail)",
        ),
        "1",
    );
    publish(&root, "@test/b", SOURCE, "1");
    let host = boot(dir, POLICY, |config, _| {
        config.runtime.fuel = 2_000_000;
    })
    .await;
    let id = host.open().await;
    // The program spends all of its own fuel after registering.
    let output = host.run(&id, r#"import * as a from "@test/a"; import * as b from "@test/b"; function main(): void { a.retain("exhaust", false); b.retain("healthy", false); while (true) {} }"#).await;
    assert!(output.response.error.is_some());
    host.sessions.close(&id).await.unwrap();
    let rows = host.rows(&id).await;
    assert!(
        rows.iter()
            .any(|row| row.package == "@test/a"
                && row.failure.as_deref() == Some("budget_exhausted")),
        "{rows:?}"
    );
    assert!(
        rows.iter()
            .any(|row| row.package == "@test/b" && row.status == "succeeded"),
        "{rows:?}"
    );
}

#[tokio::test]
async fn a_finalizer_stops_at_its_deadline() {
    let dir = tempfile::tempdir().unwrap();
    publish(
        &store_root(dir.path()),
        "@test/a",
        &SOURCE.replace(
            "if (value.fail)",
            "while (key.length > 0) {} if (value.fail)",
        ),
        "1",
    );
    let host = boot(dir, POLICY, |config, _| {
        config.runtime.timeout = Some(Duration::from_millis(300));
        config.runtime.fuel = u64::MAX;
    })
    .await;
    let id = host.open().await;
    host.run_ok(
        &id,
        r#"import { retain } from "@test/a"; function main(): void { retain("slow", false); }"#,
    )
    .await;
    let started = std::time::Instant::now();
    host.sessions.close(&id).await.unwrap();
    assert!(started.elapsed() < Duration::from_secs(20));
    assert_eq!(
        host.rows(&id).await[0].failure.as_deref(),
        Some("budget_exhausted")
    );
}

#[tokio::test]
async fn idle_expiry_runs_the_finalizers() {
    let host = host().await;
    let id = host.open().await;
    host.run_ok(
        &id,
        r#"import { retain } from "@test/a"; function main(): void { retain("expiry", false); }"#,
    )
    .await;
    let closed = host
        .sessions
        .reap(SystemTime::now() + Duration::from_secs(365 * 86_400))
        .await
        .unwrap();
    assert_eq!(closed, std::slice::from_ref(&id));
    assert_eq!(host.rows(&id).await[0].status, "succeeded");
}

#[tokio::test]
async fn a_blueprint_that_no_longer_lists_the_package_fails_cleanup_for_authority() {
    let host = host().await;
    let id = host.open().await;
    host.run_ok(
        &id,
        r#"import { retain } from "@test/a"; function main(): void { retain("revoked", false); }"#,
    )
    .await;
    *host.current.0.lock().unwrap() = Some(Arc::new(
        submilli_blueprint::parse("name: finalizers\ndefault: allow\npackages: ['@test/b']\n")
            .unwrap(),
    ));
    host.sessions.close(&id).await.unwrap();
    assert_eq!(
        host.rows(&id).await[0].failure.as_deref(),
        Some("authority_unavailable")
    );
}

#[tokio::test]
async fn close_drains_work_first_and_refuses_new_runs_and_registrations() {
    let host = host().await;
    let id = host.open().await;
    host.run_ok(&id, r#"import { retain } from "@test/a"; function main(): void { retain("close-race", false); }"#).await;
    let permit = host.sessions.admit_resource_work(&id).await.unwrap();
    let sessions = Arc::clone(&host.sessions);
    let session = id.clone();
    let close = tokio::spawn(async move { sessions.close(&session).await });
    permit.cancel.cancelled().await;
    assert!(!close.is_finished(), "close must wait for work to drain");
    let provider = host
        .sessions
        .finalizers()
        .provider(&id, BTreeMap::new())
        .unwrap();
    assert!(
        provider
            .unregister("@test/a", &"close-race".encode_utf16().collect::<Vec<_>>())
            .await
            .is_err()
    );
    let refused = host.run(&id, "function main(): number { return 1; }").await;
    assert!(refused.response.error.is_some());
    assert!(!refused.dispatched);
    drop(permit);
    assert!(close.await.unwrap().unwrap());
    assert_eq!(host.rows(&id).await[0].status, "succeeded");
}

#[tokio::test]
async fn two_closes_at_once_run_the_finalizers_once() {
    let host = host().await;
    let id = host.open().await;
    host.run_ok(
        &id,
        r#"import { retain } from "@test/a"; function main(): void { retain("once", false); }"#,
    )
    .await;
    let (first, second) = tokio::join!(host.sessions.close(&id), host.sessions.close(&id));
    assert_ne!(first.unwrap(), second.unwrap());
    let rows = host.rows(&id).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].status, "succeeded");
}

#[tokio::test]
async fn a_test_replay_is_refused_registration() {
    let host = host().await;
    let id = host.open().await;
    let mut request = host.request(
        &id,
        r#"import { retain } from "@test/a"; function main(): void { retain("replayed", false); }"#,
    );
    request.finalizers = Some(Arc::new(ReplayFinalizers::new(SessionKvLimits::default())));
    let output = host.host.run(request).await;
    let error = output.response.error.expect("registration is refused");
    assert!(error.message.contains("test replay"), "{}", error.message);
    assert!(host.rows(&id).await.is_empty());
    let replay = ReplayFinalizers::new(SessionKvLimits::default());
    assert!(
        replay
            .unregister("@test/a", &"k".encode_utf16().collect::<Vec<_>>())
            .await
            .is_ok()
    );
}

/// A secret the finalizer checks it can read.
const CANARY: &str = "session-finalizer-secret-canary!";

async fn secret_host() -> Host {
    let dir = tempfile::tempdir().unwrap();
    let source = SOURCE.replace(
        "assert(key === value.id,",
        &format!("const token = secrets.get(\"TOKEN\"); assert(token === \"{CANARY}\"); assert(key === value.id,"),
    );
    publish(
        &store_root(dir.path()),
        "@test/a",
        &format!("import * as secrets from \"submilli:secrets\";\n{source}"),
        "1",
    );
    boot_with(dir, SECRET_POLICY, false, |_, _| {}).await
}

#[tokio::test]
async fn a_close_cut_short_finishes_after_a_restart_with_its_secrets_sealed() {
    let host = secret_host().await;
    let id = host.open().await;
    host.sessions.bind_secrets(
        &id,
        Arc::new(BTreeMap::from([("TOKEN".to_owned(), CANARY.to_owned())])),
    );
    host.run_ok(&id, r#"import { retain } from "@test/a"; function main(): void { retain("secret-bound", false); }"#).await;
    // Nothing runs finalizers yet, so the close stops with them pending.
    assert!(host.sessions.close(&id).await.is_err());
    assert_eq!(host.rows(&id).await[0].status, "pending");
    assert_eq!(context_rows(&host.sessions).await, 1);
    assert!(host.sessions.session_dir(&id).exists());
    assert!(
        !contains(&host.database_bytes(), CANARY),
        "the bound secret is sealed"
    );

    let host = host.restart(true).await;
    assert!(
        host.sessions.get(&id).await.unwrap().is_none(),
        "it is not open"
    );
    assert!(
        host.sessions.session_dir(&id).exists(),
        "its files wait for cleanup"
    );
    host.sessions.finish_closing().await.unwrap();
    assert_eq!(
        host.rows(&id).await[0].status,
        "succeeded",
        "it read the secret"
    );
    assert_eq!(context_rows(&host.sessions).await, 0);
    assert!(!host.sessions.session_dir(&id).exists());
    host.sessions.close_database().await.unwrap();
    assert!(!contains(&host.database_bytes(), CANARY));
}

#[tokio::test]
async fn an_attempt_cut_short_runs_again_after_a_restart_but_a_failure_does_not() {
    let host = host().await;
    let id = host.open().await;
    host.run_ok(&id, r#"import { retain } from "@test/a"; function main(): void { retain("interrupted", false); retain("terminal", true); }"#).await;
    let host = host.restart(false).await;
    assert!(host.sessions.close(&id).await.is_err());
    let registrations = host.sessions.finalizers().clone();
    let mut interrupted = false;
    while let Some(invocation) = registrations.claim_next(&id).await.unwrap() {
        if invocation.key == "interrupted".encode_utf16().collect::<Vec<_>>() {
            // Stopped mid-attempt: nothing is recorded.
            interrupted = true;
        } else {
            registrations
                .complete(&invocation.id, Err(FinalizerFailure::Execution))
                .await
                .unwrap();
        }
    }
    assert!(interrupted);
    let host = host.restart(true).await;
    host.sessions.finish_closing().await.unwrap();
    let rows = host.rows(&id).await;
    assert!(
        rows.iter()
            .any(|row| row.key == "interrupted" && row.status == "succeeded"),
        "{rows:?}"
    );
    assert!(
        rows.iter()
            .any(|row| row.key == "terminal" && row.failure.as_deref() == Some("execution_failed")),
        "{rows:?}"
    );
}
