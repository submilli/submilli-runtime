//! Package finalizers: cleanup a package registers in a session, run once when the
//! session closes, on its own bounded engine, and again after a restart when an
//! attempt was cut short.
//!
//! Copied from `submilli-server`: `adapters/session/finalizers.rs` (the
//! registration provider, the stored rows and `terminate`),
//! `domain/finalization.rs` (the lifecycle), `application/sessions/finalization.rs`
//! (`finalize_session`), `finalizers.rs` (running one finalizer) and
//! `record/throwaway.rs` (`ReplayFinalizers`). The host is one process, so an
//! attempt has no owner generation: every attempt still running at boot was cut
//! short.

use std::collections::BTreeMap;
use std::sync::{Arc, Weak};
use std::time::Duration;

use anyhow::{Result, anyhow};
use futures::future::BoxFuture;
use interpreter::runtime::session_finalizers::{FinalizerFuture, SessionFinalizers};
use interpreter::runtime::session_kv::{SessionKvError, SessionKvLimits};
use interpreter::runtime::{
    FinalizerCall, FinalizerError, LinkedPackageModule, install_tenant_limits,
    invoke_finalizer_async,
};
use sqlx::{Row, SqliteConnection};
use submilli_blueprint::{Blueprint, HarnessSecretBindings, VarBindings};
use submilli_durable::codec::Sealer;

use super::blueprints::Blueprints;
use super::run::{FinalizerRegistry, RunHost};
use super::sessions::store::Database;

/// How long a finalizer may run when runs have no deadline.
const DEFAULT_DEADLINE: Duration = Duration::from_secs(30);

// ---- What an attempt is --------------------------------------------------------

/// Why an attempt failed. Every one is final; only an interrupted attempt runs
/// again.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FinalizerFailure {
    Execution,
    AuthorityUnavailable,
    ArtifactUnavailable,
    BudgetExhausted,
}

impl FinalizerFailure {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Execution => "execution_failed",
            Self::AuthorityUnavailable => "authority_unavailable",
            Self::ArtifactUnavailable => "artifact_unavailable",
            Self::BudgetExhausted => "budget_exhausted",
        }
    }
}

/// One registration, already claimed for an attempt.
pub(crate) struct FinalizerInvocation {
    pub(crate) id: String,
    pub(crate) package: String,
    pub(crate) key: Vec<u16>,
    pub(crate) value: Result<Vec<u16>, FinalizerFailure>,
    pub(crate) package_versions: Result<BTreeMap<String, String>, FinalizerFailure>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FinalizationStatus {
    Registered,
    Pending,
    Running,
    Succeeded,
    Failed,
}

impl FinalizationStatus {
    fn name(self) -> &'static str {
        match self {
            Self::Registered => "registered",
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
        }
    }
}

/// A terminal failure is distinct from an interrupted attempt: only the latter
/// can become pending again.
struct Finalization {
    status: FinalizationStatus,
    failure: Option<FinalizerFailure>,
}

impl Finalization {
    fn restore(status: FinalizationStatus) -> Self {
        Self {
            status,
            failure: None,
        }
    }

    fn terminate(&mut self) -> bool {
        self.advance(FinalizationStatus::Registered, FinalizationStatus::Pending)
    }

    fn claim(&mut self) -> bool {
        self.advance(FinalizationStatus::Pending, FinalizationStatus::Running)
    }

    fn complete(&mut self, outcome: Result<(), FinalizerFailure>) -> bool {
        if self.status != FinalizationStatus::Running {
            return false;
        }
        self.failure = outcome.err();
        self.status = if self.failure.is_some() {
            FinalizationStatus::Failed
        } else {
            FinalizationStatus::Succeeded
        };
        true
    }

    fn recover(&mut self) -> bool {
        self.advance(FinalizationStatus::Running, FinalizationStatus::Pending)
    }

    fn advance(&mut self, from: FinalizationStatus, to: FinalizationStatus) -> bool {
        if self.status != from {
            return false;
        }
        self.status = to;
        true
    }
}

/// The closing session a finalizer runs for, as its runs saw it.
pub(crate) struct ClosingSession {
    pub(crate) id: String,
    pub(crate) blueprint_name: String,
    pub(crate) variables: Arc<VarBindings>,
    pub(crate) secrets: Arc<HarnessSecretBindings>,
}

/// Runs one claimed finalizer.
#[async_trait::async_trait]
pub(crate) trait FinalizerExecutor: Send + Sync {
    async fn execute(
        &self,
        session: &ClosingSession,
        invocation: &FinalizerInvocation,
    ) -> Result<(), FinalizerFailure>;
}

/// Attempt every pending registration of the session once, each independently:
/// one failing does not stop the rest. `session` is what they run under, or why
/// it could not be read, which fails each of them.
pub(super) async fn finalize_session(
    registrations: &Registrations,
    executor: &dyn FinalizerExecutor,
    session_id: &str,
    session: Result<ClosingSession, FinalizerFailure>,
) -> Result<()> {
    while let Some(invocation) = registrations.claim_next(session_id).await? {
        let outcome = match (&session, &invocation.value, &invocation.package_versions) {
            (Err(failure), _, _) | (_, Err(failure), _) | (_, _, Err(failure)) => Err(*failure),
            (Ok(session), Ok(_), Ok(_)) => executor.execute(session, &invocation).await,
        };
        registrations.complete(&invocation.id, outcome).await?;
    }
    Ok(())
}

// ---- Stored registrations -------------------------------------------------------

/// The session database's finalizer rows.
#[derive(Clone)]
pub(crate) struct Registrations {
    pub(super) database: Database,
    pub(super) sealer: Sealer,
    pub(super) limits: SessionKvLimits,
}

/// A registration as stored, for reading its outcome.
#[cfg(test)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FinalizerRow {
    pub(crate) package: String,
    pub(crate) key: String,
    pub(crate) status: String,
    pub(crate) failure: Option<String>,
}

impl FinalizerRegistry for Registrations {
    fn provider(
        &self,
        session_id: &str,
        package_versions: BTreeMap<String, String>,
    ) -> Result<Arc<dyn SessionFinalizers>, String> {
        Ok(Arc::new(RegistrationProvider {
            registrations: self.clone(),
            session: session_id.to_owned(),
            package_versions,
        }))
    }
}

impl Registrations {
    /// Claim the session's next pending registration for an attempt.
    pub(crate) async fn claim_next(&self, session: &str) -> Result<Option<FinalizerInvocation>> {
        let mut tx = self.database.begin().await?;
        let row = sqlx::query(
            "SELECT id,package,local_key,payload,package_versions FROM session_finalizers \
             WHERE session_id=? AND status='pending' ORDER BY rowid LIMIT 1",
        )
        .bind(session)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let id: String = row.try_get("id")?;
        let value = row
            .try_get::<Option<Vec<u8>>, _>("payload")?
            .ok_or(FinalizerFailure::ArtifactUnavailable)
            .and_then(|payload| {
                self.sealer
                    .open(&owner(&id), &payload, "finalizer arguments")
                    .map_err(|_| FinalizerFailure::AuthorityUnavailable)
            })
            .and_then(|plain| bytes_to_units(&plain).ok_or(FinalizerFailure::ArtifactUnavailable));
        let package_versions = row
            .try_get::<Option<String>, _>("package_versions")?
            .and_then(|versions| serde_json::from_str(&versions).ok())
            .ok_or(FinalizerFailure::ArtifactUnavailable);
        let mut lifecycle = Finalization::restore(FinalizationStatus::Pending);
        if !lifecycle.claim() {
            return Err(anyhow!("finalizer attempt conflict"));
        }
        let invocation = FinalizerInvocation {
            id: id.clone(),
            package: row.try_get("package")?,
            key: bytes_to_units(&row.try_get::<Vec<u8>, _>("local_key")?)
                .ok_or_else(|| anyhow!("corrupt finalizer key"))?,
            value,
            package_versions,
        };
        sqlx::query("UPDATE session_finalizers SET status=? WHERE id=? AND status='pending'")
            .bind(lifecycle.status.name())
            .bind(id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(Some(invocation))
    }

    /// Record how a claimed attempt ended, dropping its arguments.
    pub(crate) async fn complete(
        &self,
        id: &str,
        outcome: Result<(), FinalizerFailure>,
    ) -> Result<()> {
        let mut lifecycle = Finalization::restore(FinalizationStatus::Running);
        if !lifecycle.complete(outcome) {
            return Err(anyhow!("finalizer attempt conflict"));
        }
        sqlx::query(
            "UPDATE session_finalizers SET status=?,failure=?,payload=NULL,package_versions=NULL \
             WHERE id=? AND status='running'",
        )
        .bind(lifecycle.status.name())
        .bind(lifecycle.failure.map(FinalizerFailure::as_str))
        .bind(id)
        .execute(&mut *self.database.acquire().await?)
        .await?;
        Ok(())
    }

    /// Make every attempt cut short by a stop pending again. Called at boot,
    /// before anything can claim one.
    pub(crate) async fn recover(&self) -> Result<()> {
        let mut lifecycle = Finalization::restore(FinalizationStatus::Running);
        if lifecycle.recover() {
            sqlx::query("UPDATE session_finalizers SET status=? WHERE status='running'")
                .bind(lifecycle.status.name())
                .execute(&mut *self.database.acquire().await?)
                .await?;
        }
        Ok(())
    }

    /// Whether any of the session's registrations has yet to finish.
    pub(crate) async fn has_unfinished(&self, session: &str) -> Result<bool> {
        Ok(sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM session_finalizers WHERE session_id=? \
             AND status IN ('registered','pending','running'))",
        )
        .bind(session)
        .fetch_one(&mut *self.database.acquire().await?)
        .await?)
    }

    /// The session's registrations, oldest first.
    #[cfg(test)]
    pub(crate) async fn rows(&self, session: &str) -> Result<Vec<FinalizerRow>> {
        let rows = sqlx::query(
            "SELECT package,local_key,status,failure FROM session_finalizers \
             WHERE session_id=? ORDER BY rowid",
        )
        .bind(session)
        .fetch_all(&mut *self.database.acquire().await?)
        .await?;
        rows.into_iter()
            .map(|row| {
                let key = bytes_to_units(&row.try_get::<Vec<u8>, _>("local_key")?)
                    .ok_or_else(|| anyhow!("corrupt finalizer key"))?;
                Ok(FinalizerRow {
                    package: row.try_get("package")?,
                    key: String::from_utf16_lossy(&key),
                    status: row.try_get("status")?,
                    failure: row.try_get("failure")?,
                })
            })
            .collect()
    }

    /// The secret bindings kept for the closing session, or none.
    pub(super) async fn secrets(
        &self,
        session: &str,
    ) -> Result<HarnessSecretBindings, FinalizerFailure> {
        let sealed: Option<Vec<u8>> = async {
            Ok::<_, anyhow::Error>(
                sqlx::query_scalar(
                    "SELECT sealed_secrets FROM finalizer_context WHERE session_id=?",
                )
                .bind(session)
                .fetch_optional(&mut *self.database.acquire().await?)
                .await?,
            )
        }
        .await
        .map_err(|_| FinalizerFailure::AuthorityUnavailable)?;
        let Some(sealed) = sealed else {
            return Ok(HarnessSecretBindings::new());
        };
        let plain = self
            .sealer
            .open(&secrets_owner(session), &sealed, "session secrets")
            .map_err(|_| FinalizerFailure::AuthorityUnavailable)?;
        serde_json::from_slice(&plain).map_err(|_| FinalizerFailure::AuthorityUnavailable)
    }
}

/// Called in the transaction that starts closing the session: its registrations
/// become pending, and, when it has any, its secret bindings are kept sealed
/// until its cleanup is done.
pub(super) async fn terminate(
    connection: &mut SqliteConnection,
    sealer: &Sealer,
    session: &str,
    secrets: &HarnessSecretBindings,
) -> Result<()> {
    let mut lifecycle = Finalization::restore(FinalizationStatus::Registered);
    if !lifecycle.terminate() {
        return Err(anyhow!("finalizer attempt conflict"));
    }
    let registered: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM session_finalizers WHERE session_id=? AND status='registered')",
    )
    .bind(session)
    .fetch_one(&mut *connection)
    .await?;
    if !registered {
        return Ok(());
    }
    let plain = serde_json::to_vec(secrets)?;
    let sealed = sealer
        .seal(&secrets_owner(session), &plain)
        .map_err(|error| anyhow!("{error}"))?;
    sqlx::query("INSERT OR REPLACE INTO finalizer_context(session_id,sealed_secrets) VALUES (?,?)")
        .bind(session)
        .bind(sealed)
        .execute(&mut *connection)
        .await?;
    sqlx::query(
        "UPDATE session_finalizers SET status=? WHERE session_id=? AND status='registered'",
    )
    .bind(lifecycle.status.name())
    .bind(session)
    .execute(connection)
    .await?;
    Ok(())
}

/// One run's registrations: the engine names the package, and only a package
/// that exports a validated `finalizer` may register.
struct RegistrationProvider {
    registrations: Registrations,
    session: String,
    package_versions: BTreeMap<String, String>,
}

/// Why a registration was refused, inside its transaction.
enum Refusal {
    Rejected(&'static str),
    Backend(anyhow::Error),
}

impl<E: Into<anyhow::Error>> From<E> for Refusal {
    fn from(error: E) -> Self {
        Self::Backend(error.into())
    }
}

impl SessionFinalizers for RegistrationProvider {
    fn register<'a>(
        &'a self,
        package: &'a str,
        key: &'a [u16],
        value: &'a [u16],
    ) -> FinalizerFuture<'a> {
        Box::pin(async move {
            let limits = self.registrations.limits;
            limits.check_write(key, value).map_err(|mut error| {
                if let SessionKvError::LimitExceeded { operation, .. }
                | SessionKvError::InvalidKey { operation, .. } = &mut error
                {
                    *operation = "registerFinalizer";
                }
                error
            })?;
            let versions = self
                .package_versions
                .get(package)
                .ok_or_else(|| backend("package has no validated finalizer"))?;
            let id = uuid::Uuid::new_v4().to_string();
            let payload = self
                .registrations
                .sealer
                .seal(&owner(&id), &units_to_bytes(value))
                .map_err(|_| backend("cannot seal finalizer arguments"))?;
            let key = units_to_bytes(key);
            let retained = (key.len() as u64)
                .saturating_add((value.len() as u64).saturating_mul(2))
                .saturating_add(versions.len() as u64)
                .saturating_add(64);
            let outcome: Result<(), Refusal> = async {
                let mut tx = self.registrations.database.begin().await?;
                require_open(&mut tx, &self.session).await?;
                let duplicate: bool = sqlx::query_scalar(
                    "SELECT EXISTS(SELECT 1 FROM session_finalizers WHERE session_id=? \
                     AND package=? AND local_key=? AND status IN ('registered','pending','running'))",
                )
                .bind(&self.session)
                .bind(package)
                .bind(&key)
                .fetch_one(&mut *tx)
                .await?;
                if duplicate {
                    return Err(Refusal::Rejected("duplicate package finalizer key"));
                }
                let (count, bytes): (i64, i64) = sqlx::query_as(
                    "SELECT COUNT(*),COALESCE(SUM(retained_bytes),0) FROM session_finalizers \
                     WHERE session_id=? AND status IN ('registered','pending','running')",
                )
                .bind(&self.session)
                .fetch_one(&mut *tx)
                .await?;
                let total = u64::try_from(bytes)?.saturating_add(retained);
                if u64::try_from(count)? >= limits.max_entries || total > limits.max_session_bytes
                {
                    return Err(Refusal::Rejected("finalizer registration quota exceeded"));
                }
                sqlx::query(
                    "INSERT INTO session_finalizers(id,session_id,package,local_key,\
                     package_versions,payload,retained_bytes,status) \
                     VALUES (?,?,?,?,?,?,?,'registered')",
                )
                .bind(id)
                .bind(&self.session)
                .bind(package)
                .bind(&key)
                .bind(versions)
                .bind(payload)
                .bind(i64::try_from(retained)?)
                .execute(&mut *tx)
                .await?;
                tx.commit().await?;
                Ok(())
            }
            .await;
            outcome.map_err(registration_error)
        })
    }

    fn unregister<'a>(&'a self, package: &'a str, key: &'a [u16]) -> FinalizerFuture<'a> {
        Box::pin(async move {
            self.registrations
                .limits
                .check_key("unregisterFinalizer", key)?;
            let key = units_to_bytes(key);
            let outcome: Result<(), Refusal> = async {
                let mut tx = self.registrations.database.begin().await?;
                require_open(&mut tx, &self.session).await?;
                sqlx::query(
                    "DELETE FROM session_finalizers WHERE session_id=? AND package=? \
                     AND local_key=? AND status='registered'",
                )
                .bind(&self.session)
                .bind(package)
                .bind(key)
                .execute(&mut *tx)
                .await?;
                tx.commit().await?;
                Ok(())
            }
            .await;
            outcome.map_err(registration_error)
        })
    }
}

async fn require_open(connection: &mut SqliteConnection, session: &str) -> Result<(), Refusal> {
    if super::sessions::store::is_open(connection, session).await? {
        Ok(())
    } else {
        Err(Refusal::Rejected("session is closed"))
    }
}

fn registration_error(refusal: Refusal) -> SessionKvError {
    match refusal {
        Refusal::Rejected(reason) => SessionKvError::InvalidKey {
            operation: "finalizer",
            key: String::new(),
            reason,
        },
        Refusal::Backend(error) => backend(&format!("{error:#}")),
    }
}

fn backend(message: &str) -> SessionKvError {
    SessionKvError::Backend {
        operation: "finalizer",
        message: message.to_owned(),
    }
}

fn owner(id: &str) -> String {
    format!("submilli:finalizer:v1:{id}")
}

fn secrets_owner(session: &str) -> String {
    format!("submilli:finalizer-secrets:v1:{session}")
}

fn units_to_bytes(units: &[u16]) -> Vec<u8> {
    units.iter().flat_map(|unit| unit.to_be_bytes()).collect()
}

fn bytes_to_units(bytes: &[u8]) -> Option<Vec<u16>> {
    let (pairs, rest) = bytes.as_chunks::<2>();
    rest.is_empty()
        .then(|| pairs.iter().map(|pair| u16::from_be_bytes(*pair)).collect())
}

// ---- Test replays ----------------------------------------------------------------

/// A test replay has no session to clean up after and may mix recorded handles
/// with live ones, so registering is refused: the package can release a resource
/// it just made through the replay's own connectors. Unregistering does nothing.
pub(crate) struct ReplayFinalizers {
    limits: SessionKvLimits,
}

impl ReplayFinalizers {
    pub(crate) fn new(limits: SessionKvLimits) -> Self {
        Self { limits }
    }
}

impl SessionFinalizers for ReplayFinalizers {
    fn register<'a>(&'a self, _: &'a str, key: &'a [u16], value: &'a [u16]) -> FinalizerFuture<'a> {
        Box::pin(async move {
            self.limits.check_write(key, value)?;
            Err(SessionKvError::InvalidKey {
                operation: "registerFinalizer",
                key: String::from_utf16_lossy(key),
                reason: "durable finalizer registration is unavailable during test replay; release the resource immediately",
            })
        })
    }

    fn unregister<'a>(&'a self, _: &'a str, key: &'a [u16]) -> FinalizerFuture<'a> {
        Box::pin(async move {
            self.limits.check_key("unregisterFinalizer", key)?;
            Ok(())
        })
    }
}

// ---- Running a finalizer ---------------------------------------------------------

/// The blueprint in force, which a finalizer runs under.
pub(crate) trait CurrentBlueprint: Send + Sync {
    fn current(&self) -> BoxFuture<'_, Option<Arc<Blueprint>>>;
}

impl CurrentBlueprint for Blueprints {
    fn current(&self) -> BoxFuture<'_, Option<Arc<Blueprint>>> {
        Box::pin(async move { self.for_run().await.map(|applied| applied.blueprint) })
    }
}

/// Runs finalizers with the host's packages and connectors.
pub(crate) struct HostFinalizers {
    host: Weak<RunHost>,
    blueprints: Arc<dyn CurrentBlueprint>,
}

impl HostFinalizers {
    /// Install on `host`'s sessions, before they finish any close.
    pub(crate) fn install(host: &Arc<RunHost>, blueprints: Arc<dyn CurrentBlueprint>) {
        host.sessions().set_finalizer_executor(Arc::new(Self {
            host: Arc::downgrade(host),
            blueprints,
        }));
    }
}

#[async_trait::async_trait]
impl FinalizerExecutor for HostFinalizers {
    async fn execute(
        &self,
        session: &ClosingSession,
        invocation: &FinalizerInvocation,
    ) -> Result<(), FinalizerFailure> {
        let host = self.host.upgrade().ok_or(FinalizerFailure::Execution)?;
        let blueprint = self
            .blueprints
            .current()
            .await
            .filter(|blueprint| blueprint.name == session.blueprint_name)
            .ok_or(FinalizerFailure::AuthorityUnavailable)?;
        let artifacts = load_artifacts(&host, invocation, &blueprint)?;
        run_finalizer(&host, session, invocation, &blueprint, &artifacts).await
    }
}

fn load_artifacts(
    host: &RunHost,
    invocation: &FinalizerInvocation,
    blueprint: &Blueprint,
) -> Result<Vec<submilli_build::Artifact>, FinalizerFailure> {
    let versions = invocation
        .package_versions
        .as_ref()
        .map_err(|failure| *failure)?;
    let store = host.package_store();
    let artifacts = store
        .load_closure([invocation.package.as_str()])
        .map_err(|_| FinalizerFailure::ArtifactUnavailable)?;
    if artifacts.len() != versions.len()
        || artifacts.iter().any(|artifact| {
            versions.get(&artifact.metadata.package_name)
                != Some(&artifact.metadata.package_version)
        })
    {
        return Err(FinalizerFailure::ArtifactUnavailable);
    }
    for artifact in &artifacts {
        artifact
            .package_declaration
            .check_type_limits()
            .map_err(|_| FinalizerFailure::ArtifactUnavailable)?;
    }
    if artifacts
        .iter()
        .any(|artifact| artifact.metadata.schema_version != submilli_build::ARTIFACT_SCHEMA_VERSION)
    {
        return Err(FinalizerFailure::ArtifactUnavailable);
    }
    let permitted = blueprint.packages.contains(&invocation.package)
        || blueprint.packages.iter().any(|root| {
            store.load_closure([root.as_str()]).is_ok_and(|artifacts| {
                artifacts
                    .iter()
                    .any(|artifact| artifact.metadata.package_name == invocation.package)
            })
        });
    if !permitted {
        return Err(FinalizerFailure::AuthorityUnavailable);
    }
    Ok(artifacts)
}

/// The deadline a finalizer runs under: the runs' own, or a finite default.
fn deadline(configured: Option<Duration>) -> Duration {
    configured
        .filter(|time| !time.is_zero())
        .unwrap_or(DEFAULT_DEADLINE)
}

async fn run_finalizer(
    host: &RunHost,
    session: &ClosingSession,
    invocation: &FinalizerInvocation,
    blueprint: &Arc<Blueprint>,
    artifacts: &[submilli_build::Artifact],
) -> Result<(), FinalizerFailure> {
    let value = invocation.value.as_ref().map_err(|failure| *failure)?;
    let mut config = host.runtime_config().clone();
    let timeout = deadline(config.timeout);
    config.timeout = Some(timeout);
    let engine = config
        .engine_async()
        .map_err(|_| FinalizerFailure::Execution)?;
    let data = host
        .finalizer_store_data(&session.id, blueprint, &session.variables, &session.secrets)
        .await
        .map_err(|_| FinalizerFailure::AuthorityUnavailable)?;
    let mut store = config
        .store_async(&engine, data)
        .map_err(|_| FinalizerFailure::Execution)?;
    install_tenant_limits(&mut store);
    let _watchdog = config
        .arm_timeout(&engine)
        .map_err(|_| FinalizerFailure::Execution)?;
    let modules = artifacts
        .iter()
        .map(|artifact| wasmtime::Module::new(&engine, &artifact.wasm))
        .collect::<wasmtime::Result<Vec<_>>>()
        .map_err(|_| FinalizerFailure::Execution)?;
    let linked: Vec<_> = modules
        .iter()
        .zip(artifacts)
        .map(|(module, artifact)| LinkedPackageModule {
            module,
            declaration: &artifact.package_declaration,
            type_info: &artifact.type_info,
        })
        .collect();
    let call = FinalizerCall {
        package: &invocation.package,
        key: &invocation.key,
        value,
    };
    let execution = invoke_finalizer_async(&mut store, &linked, call);
    let outcome = match tokio::time::timeout(timeout, execution).await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(error)) => Err(failure(&error)),
        Err(_) => Err(FinalizerFailure::BudgetExhausted),
    };
    store
        .data_mut()
        .finish_blocking_work()
        .await
        .map_err(|_| FinalizerFailure::Execution)?;
    outcome
}

fn failure(error: &FinalizerError) -> FinalizerFailure {
    match error {
        FinalizerError::Unavailable(_) => FinalizerFailure::ArtifactUnavailable,
        FinalizerError::Denied(_) => FinalizerFailure::AuthorityUnavailable,
        FinalizerError::BudgetExhausted(_) => FinalizerFailure::BudgetExhausted,
        FinalizerError::MalformedArgument(_)
        | FinalizerError::IncompatibleArgument(_)
        | FinalizerError::Failed(_) => FinalizerFailure::Execution,
    }
}

#[cfg(test)]
mod tests;
