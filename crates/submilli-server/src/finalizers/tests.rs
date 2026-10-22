use super::*;
use crate::blueprint::StoredBlueprint;
use crate::config::ServerConfig;
use crate::handlers::sessions::{self, end_session_with_finalizers, session_finalizers};

/// Starts a `finalizers` session as `POST /v1/sessions` would, with no variables
/// or application tools.
async fn start_session(
    state: &crate::AppState,
    secrets: submilli_blueprint::HarnessSecretBindings,
) -> Result<String, sessions::SessionStartError> {
    sessions::start(
        state,
        "finalizers",
        &Default::default(),
        &secrets,
        Vec::new(),
    )
    .await
    .map(|started| started.session_id)
}

/// Runs `code` in a started session, as `POST /v1/sessions/{id}/execute` without
/// an `Idempotency-Key` runs it.
async fn run_in_session(
    state: &crate::AppState,
    session_id: &str,
    code: &str,
) -> Result<crate::handlers::execute::ExecuteResponse, sessions::SessionRunError> {
    let audit = crate::audit::ExecutionAudit::new(
        state.audit().clone(),
        "test",
        "session",
        Some(session_id),
    );
    let bound = sessions::bound_for_run(state, session_id, code, Some(&audit)).await?;
    let outcome = sessions::run(state, session_id, bound, code, None, audit.clone(), None).await;
    audit.finish(outcome.response.error.is_none());
    Ok(outcome.response)
}

/// Ends a session as `DELETE /v1/sessions/{id}` does.
async fn end_session(
    state: &crate::AppState,
    session_id: &str,
) -> Result<bool, crate::session_manager::SessionError> {
    state.session_manager().close_owned(session_id).await
}

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

struct Server {
    state: AppState,
    root: tempfile::TempDir,
}
impl Server {
    async fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        for package in ["@test/a", "@test/b"] {
            publish(root.path(), package, SOURCE, "1");
        }
        let state = open(root.path()).await;
        Self { state, root }
    }
    async fn start(&self) -> String {
        start_session(&self.state, Default::default())
            .await
            .unwrap()
    }
    async fn run(&self, id: &str, code: &str) -> crate::handlers::execute::ExecuteResponse {
        run_in_session(&self.state, id, code).await.unwrap()
    }
}

fn publish(root: &std::path::Path, name: &str, source: &str, version: &str) {
    let compiled = crate::compiler_thread::run(|| {
        interpreter::compile_package(
            name,
            interpreter::ModulePath::from("lib"),
            &[interpreter::PackageSourceModule {
                path: interpreter::ModulePath::from("lib"),
                source,
            }],
            &[],
        )
    })
    .unwrap()
    .unwrap();
    submilli_build::write_package_artifact(
        root.join("packages").join(name),
        &compiled.wasm,
        &compiled.type_info,
        &submilli_build::derive_capability_schema(
            &compiled.declaration,
            &[],
            &compiled.required_capabilities,
        ),
        &compiled.declaration,
        &submilli_build::ArtifactMetadata::new(name, version, vec![]),
    )
    .unwrap();
}

async fn open(root: &std::path::Path) -> AppState {
    open_with(root, |_| {}).await
}

async fn open_with(root: &std::path::Path, configure: impl FnOnce(&mut ServerConfig)) -> AppState {
    let mut config = ServerConfig {
        database_path: Some(root.join("server.db")),
        network_policy: interpreter::runtime::NetworkPolicy::deny_private().allow_localhost(true),
        package_store_root: Some(root.join("packages")),
        volume_root: Some(root.to_owned()),
        ..Default::default()
    };
    configure(&mut config);
    let state = AppState::new(config).await.unwrap();
    if state
        .blueprints()
        .get("finalizers")
        .await
        .unwrap()
        .is_none()
    {
        let policy = "name: finalizers\ndefault: allow\nallow_insecure_http: true\npackages: ['@test/a', '@test/b']\nvfs: per_session\n";
        state
            .blueprints()
            .add_yaml(StoredBlueprint::new(
                submilli_blueprint::parse(policy).unwrap(),
                policy.into(),
            ))
            .await
            .unwrap();
    }
    state.boot().await.unwrap();
    state
}

#[tokio::test]
async fn finalizers_receive_local_keys_are_isolated_and_fail_independently() {
    let server = Server::new().await;
    let id = server.start().await;
    let response = server
        .run(
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
    assert!(response.error.is_none(), "{:?}", response.error);
    let before = session_finalizers(&server.state, &id).await.unwrap();
    assert_eq!(before.len(), 2);
    assert!(before.iter().all(|row| row.status == "registered"));
    end_session(&server.state, &id).await.unwrap();
    let after = session_finalizers(&server.state, &id).await.unwrap();
    assert_eq!(
        after.iter().filter(|row| row.status == "succeeded").count(),
        1,
        "{after:?}"
    );
    assert_eq!(
        after.iter().filter(|row| row.status == "failed").count(),
        1,
        "{after:?}"
    );
    assert!(
        !serde_json::to_string(&after)
            .unwrap()
            .contains("private-finalizer-failure-canary")
    );
    server.state.session_manager().reap_now().await.unwrap();
    assert_eq!(
        serde_json::to_value(&after).unwrap(),
        serde_json::to_value(session_finalizers(&server.state, &id).await.unwrap()).unwrap()
    );
}

#[tokio::test]
async fn early_release_removes_only_the_owning_packages_key() {
    let server = Server::new().await;
    let id = server.start().await;
    let response = server.run(&id, r#"
        import * as a from "@test/a"; import * as b from "@test/b";
        function main(): void { a.retain("same", false); b.retain("same", false); a.release("same"); a.release("absent"); }
    "#).await;
    assert!(response.error.is_none(), "{:?}", response.error);
    let rows = session_finalizers(&server.state, &id).await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].package, "@test/b");
    end_session(&server.state, &id).await.unwrap();
    assert_eq!(
        session_finalizers(&server.state, &id).await.unwrap()[0].status,
        "succeeded"
    );
}

#[tokio::test]
async fn package_upgrade_makes_registered_cleanup_unavailable() {
    let server = Server::new().await;
    let id = server.start().await;
    let response = server.run(&id, r#"import { retain } from "@test/a"; function main(): void { retain("original", false); }"#).await;
    assert!(response.error.is_none(), "{:?}", response.error);
    let Server { state, root } = server;
    let database = state.database().unwrap();
    drop(state);
    database.close().await.unwrap();
    drop(database);
    publish(
        root.path(),
        "@test/a",
        &SOURCE.replace("if (value.fail)", "if (true)"),
        "2",
    );
    let state = open(root.path()).await;
    end_session(&state, &id).await.unwrap();
    assert_eq!(
        session_finalizers(&state, &id).await.unwrap()[0].status,
        "failed"
    );
}

#[tokio::test]
async fn programs_cannot_call_reserved_finalizers_or_register_as_a_package() {
    let server = Server::new().await;
    let id = server.start().await;
    for code in [
        r#"import { finalizer } from "@test/a"; function main(): void { finalizer("k", { id: "k", fail: false }); }"#,
        r#"import * as session from "submilli:session"; function main(): void { session.registerFinalizer("k", 1); }"#,
        r#"import { unregisterFinalizer } from "submilli:session"; function main(): void { unregisterFinalizer("k"); }"#,
    ] {
        let response = server.run(&id, code).await;
        assert!(response.error.is_some());
    }
    assert!(
        session_finalizers(&server.state, &id)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn duplicate_registration_is_catchable_and_keeps_the_original_value() {
    let server = Server::new().await;
    let id = server.start().await;
    let response = server
        .run(
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
    assert!(response.error.is_none(), "{:?}", response.error);
    end_session(&server.state, &id).await.unwrap();
    let rows = session_finalizers(&server.state, &id).await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].status, "succeeded");
}

#[tokio::test]
async fn unreadable_arguments_fail_only_their_registration() {
    let server = Server::new().await;
    let id = server.start().await;
    let response = server.run(&id, r#"import { retain } from "@test/a"; function main(): void { retain("bad", false); retain("good", false); }"#).await;
    assert!(response.error.is_none(), "{:?}", response.error);
    let session = id.clone();
    server.state.database().unwrap().transaction(move |connection| Box::pin(async move {
        sqlx::query("UPDATE session_finalizers SET payload=x'01' WHERE session_id=? AND local_key=?")
            .bind(session).bind("bad".encode_utf16().flat_map(u16::to_be_bytes).collect::<Vec<_>>()).execute(connection).await?;
        Ok(())
    })).await.unwrap();
    end_session(&server.state, &id).await.unwrap();
    let rows = session_finalizers(&server.state, &id).await.unwrap();
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
async fn unreadable_package_versions_fail_only_their_registration() {
    let server = Server::new().await;
    let id = server.start().await;
    let response = server.run(&id, r#"import { retain } from "@test/a"; function main(): void { retain("bad", false); retain("good", false); }"#).await;
    assert!(response.error.is_none(), "{:?}", response.error);
    let session = id.clone();
    server.state.database().unwrap().transaction(move |connection| Box::pin(async move {
        sqlx::query("UPDATE session_finalizers SET package_versions='invalid json' WHERE session_id=? AND local_key=?")
            .bind(session).bind("bad".encode_utf16().flat_map(u16::to_be_bytes).collect::<Vec<_>>()).execute(connection).await?;
        Ok(())
    })).await.unwrap();
    end_session(&server.state, &id).await.unwrap();
    let rows = session_finalizers(&server.state, &id).await.unwrap();
    assert!(
        rows.iter()
            .any(|row| row.key == "bad" && row.status == "failed")
    );
    assert!(
        rows.iter()
            .any(|row| row.key == "good" && row.status == "succeeded")
    );
}

async fn terminate_without_worker(state: &AppState, id: &str) {
    let id = id.to_owned();
    state.database().unwrap().transaction(move |connection| Box::pin(async move {
        crate::adapters::session::finalizers::terminate(connection, &id).await?;
        sqlx::query("UPDATE sessions SET status='closed',closed_reason='deleted' WHERE session_id=?").bind(&id).execute(&mut *connection).await?;
        sqlx::query("INSERT INTO session_cleanup(session_id,folder_path) SELECT session_id,root_vfs_path FROM sessions WHERE session_id=?").bind(id).execute(connection).await?;
        Ok(())
    })).await.unwrap();
}

#[tokio::test]
async fn recovery_reexecutes_interrupted_attempts_but_never_terminal_errors() {
    use crate::application::sessions::finalization::Finalizations;
    let server = Server::new().await;
    let id = server.start().await;
    let response = server.run(&id, r#"import { retain } from "@test/a"; function main(): void { retain("interrupted", false); retain("terminal", true); }"#).await;
    assert!(response.error.is_none(), "{:?}", response.error);
    terminate_without_worker(&server.state, &id).await;
    let store = server.state.session_manager().finalizer_store();
    let mut interrupted = false;
    while let Some(invocation) = store.claim_next(&id).await.unwrap() {
        let outcome = execute(&server.state, &invocation).await;
        if invocation.key == "interrupted".encode_utf16().collect::<Vec<_>>() {
            assert!(outcome.is_ok());
            interrupted = true; // Crash after effect, before recording success.
        } else {
            assert!(outcome.is_err());
            store.complete(&invocation.id, outcome).await.unwrap();
        }
    }
    assert!(interrupted);
    drop(store);
    let Server { state, root } = server;
    let database = state.database().unwrap();
    drop(state);
    database.close().await.unwrap();
    drop(database);
    let state = open(root.path()).await;
    let rows = session_finalizers(&state, &id).await.unwrap();
    assert!(
        rows.iter()
            .any(|row| row.key == "interrupted" && row.status == "succeeded")
    );
    assert!(
        rows.iter()
            .any(|row| row.key == "terminal" && row.status == "failed")
    );
}

#[tokio::test]
async fn current_policy_revocation_is_a_terminal_failure() {
    let server = Server::new().await;
    let id = server.start().await;
    let response = server.run(&id, r#"import { retain } from "@test/a"; function main(): void { retain("revoked", false); }"#).await;
    assert!(response.error.is_none(), "{:?}", response.error);
    let policy = "name: finalizers
default: allow
packages: ['@test/b']
vfs: per_session
";
    server
        .state
        .blueprints()
        .upsert_yaml(StoredBlueprint::new(
            submilli_blueprint::parse(policy).unwrap(),
            policy.into(),
        ))
        .await
        .unwrap();
    let report = end_session_with_finalizers(&server.state, &id)
        .await
        .unwrap();
    assert!(report.closed);
    assert_eq!(report.finalizers[0].status, "failed");
    assert_eq!(
        report.finalizers[0].failure.as_deref(),
        Some("authority_unavailable")
    );
}

#[tokio::test]
async fn unrelated_initializers_are_not_replayed_for_another_packages_cleanup() {
    let server = Server::new().await;
    publish(
        server.root.path(),
        "@test/b",
        r#"
        import * as session from "submilli:session";
        session.set("initializer", 1);
        export function touch(): void {}
    "#,
        "1",
    );
    let id = server.start().await;
    let response = server.run(&id, r#"import { retain } from "@test/a"; import { touch } from "@test/b"; function main(): void { touch(); retain("a", false); }"#).await;
    assert!(response.error.is_none(), "{:?}", response.error);
    end_session(&server.state, &id).await.unwrap();
    assert_eq!(
        session_finalizers(&server.state, &id).await.unwrap()[0].status,
        "succeeded"
    );
}

#[tokio::test]
async fn dependency_package_cleanup_checks_installed_versions() {
    for installed in ["unchanged", "upgraded", "removed"] {
        let server = Server::new().await;
        let dependency =
            submilli_build::read_package_artifact(server.root.path().join("packages/@test/a"))
                .unwrap();
        let compiled = crate::compiler_thread::run(|| interpreter::compile_package(
        "@test/root", interpreter::ModulePath::from("lib"), &[interpreter::PackageSourceModule {
            path: interpreter::ModulePath::from("lib"),
            source: r#"import { retain } from "@test/a"; export function open(): void { retain("dependency", false); }"#,
        }], &[&dependency.package_declaration],
    )).unwrap().unwrap();
        submilli_build::write_package_artifact(
            server.root.path().join("packages/@test/root"),
            &compiled.wasm,
            &compiled.type_info,
            &submilli_build::derive_capability_schema(
                &compiled.declaration,
                &[&dependency.package_declaration],
                &compiled.required_capabilities,
            ),
            &compiled.declaration,
            &submilli_build::ArtifactMetadata::new(
                "@test/root",
                "1",
                vec![submilli_build::ArtifactDependency::new("@test/a", "1")],
            ),
        )
        .unwrap();
        let policy = "name: finalizers
default: allow
packages: ['@test/root']
vfs: per_session
";
        server
            .state
            .blueprints()
            .upsert_yaml(StoredBlueprint::new(
                submilli_blueprint::parse(policy).unwrap(),
                policy.into(),
            ))
            .await
            .unwrap();
        let id = server.start().await;
        let response = server
            .run(
                &id,
                r#"import { open } from "@test/root"; function main(): void { open(); }"#,
            )
            .await;
        assert!(response.error.is_none(), "{:?}", response.error);
        match installed {
            "upgraded" => publish(server.root.path(), "@test/a", SOURCE, "2"),
            "removed" => {
                std::fs::remove_dir_all(server.root.path().join("packages/@test/a")).unwrap();
            }
            _ => {}
        }
        end_session(&server.state, &id).await.unwrap();
        let rows = session_finalizers(&server.state, &id).await.unwrap();
        if installed == "unchanged" {
            assert_eq!(rows[0].status, "succeeded");
        } else {
            assert_eq!(rows[0].failure.as_deref(), Some("artifact_unavailable"));
        }
    }
}

#[tokio::test]
async fn missing_installed_package_fails_cleanup() {
    let server = Server::new().await;
    let id = server.start().await;
    let response = server.run(&id, r#"import { retain } from "@test/a"; function main(): void { retain("missing", false); }"#).await;
    assert!(response.error.is_none(), "{:?}", response.error);
    std::fs::remove_dir_all(server.root.path().join("packages/@test/a")).unwrap();
    end_session(&server.state, &id).await.unwrap();
    let rows = session_finalizers(&server.state, &id).await.unwrap();
    assert_eq!(rows[0].failure.as_deref(), Some("artifact_unavailable"));
}

#[tokio::test]
async fn finalizers_cannot_register_more_cleanup() {
    let server = Server::new().await;
    publish(
        server.root.path(),
        "@test/a",
        &SOURCE.replace(
            "assert(key === value.id,",
            "session.registerFinalizer(key + \"new\", value); assert(key === value.id,",
        ),
        "1",
    );
    let id = server.start().await;
    let response = server.run(&id, r#"import { retain } from "@test/a"; function main(): void { retain("no-recursion", false); }"#).await;
    assert!(response.error.is_none(), "{:?}", response.error);
    end_session(&server.state, &id).await.unwrap();
    let rows = session_finalizers(&server.state, &id).await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].status, "failed");
}

#[tokio::test]
#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
async fn mock_http_resource_survives_executions_and_is_released_on_close() {
    use httpmock::Method::{DELETE, POST};
    let remote = httpmock::MockServer::start_async().await;
    let created = remote
        .mock_async(|when, then| {
            when.method(POST).path("/resource");
            then.status(200).body("resource-1");
        })
        .await;
    let released = remote
        .mock_async(|when, then| {
            when.method(DELETE).path("/resource");
            then.status(204);
        })
        .await;
    let server = Server::new().await;
    publish(
        server.root.path(),
        "@test/a",
        r#"
        import * as http from "submilli:http";
        import * as session from "submilli:session";
        export interface Handle { id: string; url: string; }
        export function open(url: string): Handle {
            const response = http.post(url, "");
            const handle = { id: response.body, url };
            try { session.registerFinalizer(handle.id, handle); }
            catch (error: Error) { http.delete(url); throw error; }
            return handle;
        }
        export function finalizer(key: string, value: Handle): void {
            assert(key === value.id);
            const response = http.delete(value.url);
            if (response.status !== 204 && response.status !== 404) { throw new Error("cleanup failed"); }
        }
    "#,
        "1",
    );
    let id = server.start().await;
    let url = serde_json::to_string(&remote.url("/resource")).unwrap();
    let response = server.run(&id, &format!(r#"import {{ open }} from "@test/a"; import * as session from "submilli:session"; function main(): void {{ session.set("resource", open({url})); }}"#)).await;
    assert!(response.error.is_none(), "{:?}", response.error);
    created.assert_async().await;
    assert_eq!(released.hits_async().await, 0);
    let response = server.run(&id, r#"import type { Handle } from "@test/a"; import * as session from "submilli:session"; function main(): string { const handle = session.get<Handle>("resource"); if (handle === undefined) { throw new Error("missing handle"); } session.remove("resource"); return handle.id; }"#).await;
    assert!(response.error.is_none(), "{:?}", response.error);
    assert_eq!(response.result.as_deref(), Some("resource-1"));
    assert_eq!(released.hits_async().await, 0);
    end_session(&server.state, &id).await.unwrap();
    released.assert_async().await;
    assert_eq!(
        session_finalizers(&server.state, &id).await.unwrap()[0].status,
        "succeeded"
    );
}

#[tokio::test]
async fn exhausted_program_fuel_does_not_spend_the_finalizers_budget() {
    let root = tempfile::tempdir().unwrap();
    publish(root.path(), "@test/a", SOURCE, "1");
    let state = open_with(root.path(), |config| {
        config.runtime.fuel = 2_000_000;
    })
    .await;
    let server = Server { state, root };
    let id = server.start().await;
    let response = server.run(&id, r#"import { retain } from "@test/a"; function main(): void { retain("independent", false); while (true) {} }"#).await;
    assert!(response.error.is_some());
    end_session(&server.state, &id).await.unwrap();
    assert_eq!(
        session_finalizers(&server.state, &id).await.unwrap()[0].status,
        "succeeded"
    );
}

#[tokio::test]
async fn finalizer_fuel_exhaustion_is_terminal_and_does_not_block_another_package() {
    let root = tempfile::tempdir().unwrap();
    publish(
        root.path(),
        "@test/a",
        &SOURCE.replace(
            "if (value.fail)",
            "while (key.length > 0) {} if (value.fail)",
        ),
        "1",
    );
    publish(root.path(), "@test/b", SOURCE, "1");
    let state = open_with(root.path(), |config| {
        config.runtime.fuel = 2_000_000;
    })
    .await;
    let server = Server { state, root };
    let id = server.start().await;
    let response = server.run(&id, r#"import * as a from "@test/a"; import * as b from "@test/b"; function main(): void { a.retain("exhaust", false); b.retain("healthy", false); }"#).await;
    assert!(response.error.is_none(), "{:?}", response.error);
    end_session(&server.state, &id).await.unwrap();
    let rows = session_finalizers(&server.state, &id).await.unwrap();
    assert!(
        rows.iter()
            .any(|row| row.package == "@test/a"
                && row.failure.as_deref() == Some("budget_exhausted")),
        "{rows:?}"
    );
    assert!(
        rows.iter()
            .any(|row| row.package == "@test/b" && row.status == "succeeded")
    );
}

#[tokio::test]
async fn registration_quota_rejection_keeps_existing_cleanup() {
    let root = tempfile::tempdir().unwrap();
    publish(root.path(), "@test/a", SOURCE, "1");
    let state = open_with(root.path(), |config| {
        config.session_kv_limits.max_entries = 1;
    })
    .await;
    let server = Server { state, root };
    let id = server.start().await;
    let response = server
        .run(
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
    assert!(response.error.is_none(), "{:?}", response.error);
    end_session(&server.state, &id).await.unwrap();
    let rows = session_finalizers(&server.state, &id).await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].key, "first");
    assert_eq!(rows[0].status, "succeeded");
}

#[tokio::test]
async fn idle_expiry_waits_for_finalization() {
    let server = Server::new().await;
    let id = server.start().await;
    let response = server.run(&id, r#"import { retain } from "@test/a"; function main(): void { retain("expiry", false); }"#).await;
    assert!(response.error.is_none(), "{:?}", response.error);
    let session = id.clone();
    server
        .state
        .database()
        .unwrap()
        .transaction(move |connection| {
            Box::pin(async move {
                sqlx::query("UPDATE sessions SET last_activity_unix_ms=0 WHERE session_id=?")
                    .bind(session)
                    .execute(connection)
                    .await?;
                Ok(())
            })
        })
        .await
        .unwrap();
    server.state.session_manager().reap_now().await.unwrap();
    assert_eq!(
        session_finalizers(&server.state, &id).await.unwrap()[0].status,
        "succeeded"
    );
}

#[tokio::test]
async fn administrative_blueprint_removal_attempts_cleanup_under_revoked_authority() {
    let server = Server::new().await;
    let id = server.start().await;
    let response = server.run(&id, r#"import { retain } from "@test/a"; function main(): void { retain("admin", false); }"#).await;
    assert!(response.error.is_none(), "{:?}", response.error);
    assert!(server.state.remove_blueprint("finalizers").await.unwrap());
    let rows = session_finalizers(&server.state, &id).await.unwrap();
    assert_eq!(rows[0].status, "failed");
    assert_eq!(rows[0].failure.as_deref(), Some("authority_unavailable"));
}

#[tokio::test]
async fn close_cancels_resource_work_before_finalization_and_rejects_registration() {
    let server = Server::new().await;
    let id = server.start().await;
    let response = server.run(&id, r#"import { retain } from "@test/a"; function main(): void { retain("close-race", false); }"#).await;
    assert!(response.error.is_none(), "{:?}", response.error);
    let permit = server
        .state
        .session_manager()
        .admit_resource_work(&id)
        .await
        .unwrap();
    let manager = server.state.session_manager().clone();
    let session = id.clone();
    let close = tokio::spawn(async move { manager.close_owned(&session).await });
    permit.cancel.cancelled().await;
    assert!(!close.is_finished(), "close must wait for work to drain");
    let provider = server
        .state
        .session_manager()
        .finalizer_provider(&id, &Default::default())
        .unwrap();
    assert!(
        provider
            .unregister("@test/a", &"close-race".encode_utf16().collect::<Vec<_>>())
            .await
            .is_err()
    );
    drop(permit);
    close.await.unwrap().unwrap();
    assert_eq!(
        session_finalizers(&server.state, &id).await.unwrap()[0].status,
        "succeeded"
    );
}

#[tokio::test]
async fn finalizers_retain_encrypted_session_credentials_until_the_attempt_finishes() {
    let root = tempfile::tempdir().unwrap();
    let source = SOURCE.replace("assert(key === value.id,", "const token = secrets.get(\"TOKEN\"); assert(token !== undefined && token.length === 32); assert(key === value.id,");
    publish(
        root.path(),
        "@test/a",
        &format!("import * as secrets from \"submilli:secrets\";\n{source}"),
        "1",
    );
    let state = open_with(root.path(), |config| {
        config.session_cipher = Some(Arc::new(
            submilli_shared::secret_store::SecretCipher::from_key([7; 32]),
        ));
    })
    .await;
    let policy = "name: finalizers
default: allow
packages: ['@test/a']
vfs: per_session
secrets:
  TOKEN:
    harness:
      required: true
";
    state
        .blueprints()
        .upsert_yaml(StoredBlueprint::new(
            submilli_blueprint::parse(policy).unwrap(),
            policy.into(),
        ))
        .await
        .unwrap();
    let canary = "session-finalizer-secret-canary!";
    assert_eq!(canary.len(), 32);
    let id = start_session(
        &state,
        std::collections::BTreeMap::from([("TOKEN".into(), canary.into())]),
    )
    .await
    .unwrap();
    let server = Server { state, root };
    let response = server.run(&id, r#"import { retain } from "@test/a"; function main(): void { retain("secret-bound", false); }"#).await;
    assert!(response.error.is_none(), "{:?}", response.error);
    end_session(&server.state, &id).await.unwrap();
    assert_eq!(
        session_finalizers(&server.state, &id).await.unwrap()[0].status,
        "succeeded"
    );
    for filename in ["server.db", "server.db-wal"] {
        if let Ok(bytes) = std::fs::read(server.root.path().join(filename)) {
            assert!(
                !bytes
                    .windows(canary.len())
                    .any(|part| part == canary.as_bytes()),
                "credentials reached {filename}"
            );
        }
    }
}

#[tokio::test]
async fn concurrent_close_callers_wait_for_the_same_finalization() {
    let server = Server::new().await;
    let id = server.start().await;
    let response = server
        .run(
            &id,
            r#"import { retain } from "@test/a"; function main(): void { retain("once", false); }"#,
        )
        .await;
    assert!(response.error.is_none(), "{:?}", response.error);
    let (first, second) = tokio::join!(
        end_session_with_finalizers(&server.state, &id),
        end_session_with_finalizers(&server.state, &id),
    );
    let first = first.unwrap();
    let second = second.unwrap();
    assert_ne!(first.closed, second.closed);
    assert_eq!(first.finalizers.len(), 1);
    assert_eq!(first.finalizers[0].status, "succeeded");
    assert_eq!(second.finalizers[0].status, "succeeded");
}

#[tokio::test]
async fn failed_credential_removal_keeps_cleanup_pending_until_it_can_complete() {
    let server = Server::new().await;
    let id = server.start().await;
    let response = server.run(&id, r#"import { retain } from "@test/a"; function main(): void { retain("cleanup-boundary", false); }"#).await;
    assert!(response.error.is_none(), "{:?}", response.error);
    let database = server.state.database().unwrap();
    database.transaction(|connection| Box::pin(async move {
        sqlx::query("CREATE TRIGGER reject_context_removal BEFORE DELETE ON finalizer_context BEGIN SELECT RAISE(ABORT,'injected context deletion failure'); END")
            .execute(connection).await?;
        Ok(())
    })).await.unwrap();
    assert!(end_session(&server.state, &id).await.is_err());
    let (queued, contexts): (i64, i64) = database.read(|connection| Box::pin(async move {
        sqlx::query_as("SELECT (SELECT COUNT(*) FROM session_cleanup),(SELECT COUNT(*) FROM finalizer_context)").fetch_one(connection).await.map_err(Into::into)
    })).await.unwrap();
    assert_eq!((queued, contexts), (1, 1));
    assert_eq!(
        session_finalizers(&server.state, &id).await.unwrap()[0].status,
        "succeeded"
    );
    database
        .transaction(|connection| {
            Box::pin(async move {
                sqlx::query("DROP TRIGGER reject_context_removal")
                    .execute(connection)
                    .await?;
                Ok(())
            })
        })
        .await
        .unwrap();
    assert!(
        !end_session(&server.state, &id).await.unwrap(),
        "the session was already closed; cleanup still completes"
    );
    let (queued, contexts): (i64, i64) = database.read(|connection| Box::pin(async move {
        sqlx::query_as("SELECT (SELECT COUNT(*) FROM session_cleanup),(SELECT COUNT(*) FROM finalizer_context)").fetch_one(connection).await.map_err(Into::into)
    })).await.unwrap();
    assert_eq!((queued, contexts), (0, 0));
}

#[tokio::test]
async fn recovery_does_not_redeliver_a_live_attempt_in_the_same_runtime() {
    use crate::application::sessions::finalization::Finalizations;
    let server = Server::new().await;
    let id = server.start().await;
    let response = server.run(&id, r#"import { retain } from "@test/a"; function main(): void { retain("live-owner", false); }"#).await;
    assert!(response.error.is_none(), "{:?}", response.error);
    terminate_without_worker(&server.state, &id).await;
    let store = server.state.session_manager().finalizer_store();
    let invocation = store.claim_next(&id).await.unwrap().unwrap();
    store.recover().await.unwrap();
    assert!(store.claim_next(&id).await.unwrap().is_none());
    store
        .complete(&invocation.id, execute(&server.state, &invocation).await)
        .await
        .unwrap();
}
