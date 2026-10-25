//! The host as the playground's own actions use it: a one-off program, a session
//! started, run in and ended, a test run, a cancel, and the blueprint file applied.
//!
//! Copied from `submilli-server`'s embedder entry points: `record/program.rs`
//! (`run_program`), `record/session.rs` (`start_session`, `run_in_session`,
//! `end_session`, `session_variables`), `handlers/execute.rs` (`one_shot`, the
//! checks a one-off program passes before it runs) and `app.rs` (`cancel_run`,
//! `apply_local_blueprint`, `evict_all_prepared_packages`).

use std::collections::BTreeMap;
use std::sync::Arc;

use anyhow::Result;
use submilli_blueprint::{
    Blueprint, HarnessSecretBindings, VarBindings, required_harness_secrets,
    resolve_harness_secrets, resolve_variables,
};

use super::super::log::warn;
use super::blueprints::{AppliedBlueprint, Blueprints, LocalApplied, LocalApplyError};
use super::record::test_run::{self, TestError, TestHost, TestOutcome, TestRun};
use super::run::{
    ErrorKind, ExecuteError, ExecuteResponse, RunEntry, RunHost, RunRequest, VfsSource,
};
use super::sessions::Sessions;
use super::volumes::Volumes;

/// A program to run in a session of its own, closed once it returns.
pub(crate) struct ProgramRun {
    /// Who the run is recorded as.
    pub(crate) label: String,
    pub(crate) blueprint: String,
    pub(crate) code: String,
    pub(crate) variables: BTreeMap<String, String>,
    /// Harness credentials for this run alone. Never persisted.
    pub(crate) secrets: HarnessSecretBindings,
}

/// A session to start.
pub(crate) struct SessionStart {
    pub(crate) blueprint: String,
    /// `${vars.NAME}` bindings, fixed for the whole session.
    pub(crate) variables: BTreeMap<String, String>,
    /// Harness credentials, held in memory while the session is open.
    pub(crate) secrets: HarnessSecretBindings,
}

/// Why a session was not started.
#[derive(Debug)]
pub(crate) enum SessionStartError {
    /// The playground serves no blueprint by that name.
    UnknownBlueprint(String),
    /// The variables do not satisfy the blueprint's declarations.
    InvalidVariables(String),
    /// The blueprint's files or git checkout cannot be resolved with these variables.
    InvalidFilesystem(String),
    /// The harness secrets do not satisfy the blueprint's declarations.
    InvalidSecrets(String),
    /// The session could not be stored.
    Session(String),
}

impl std::fmt::Display for SessionStartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownBlueprint(name) => write!(f, "unknown blueprint: {name}"),
            Self::InvalidVariables(message) => write!(f, "invalid variables: {message}"),
            Self::InvalidFilesystem(message) | Self::Session(message) => f.write_str(message),
            Self::InvalidSecrets(message) => write!(f, "invalid secrets: {message}"),
        }
    }
}

impl std::error::Error for SessionStartError {}

/// Code to run in a started session.
pub(crate) struct SessionProgram {
    /// Who the run is recorded as.
    pub(crate) label: String,
    pub(crate) session_id: String,
    pub(crate) code: String,
}

/// Why a session could not run a program. A refused run never started, so no
/// recorder saw it.
#[derive(Debug)]
pub(crate) enum SessionRunError {
    /// No session has that id, or it has ended or expired.
    UnknownSession,
    /// The playground no longer serves the session's blueprint.
    BlueprintMissing(String),
    /// The blueprint requires harness secrets the session no longer holds, as after a
    /// restart; rebind them to run again.
    SecretsRequired { required: Vec<String> },
    /// The session's state could not be read.
    Session(String),
}

impl std::fmt::Display for SessionRunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownSession => f.write_str("unknown, ended, or expired session"),
            Self::BlueprintMissing(name) => {
                write!(f, "the session's blueprint no longer exists: {name}")
            }
            Self::SecretsRequired { required } => write!(
                f,
                "the session requires harness secrets it does not hold: {}",
                required.join(", ")
            ),
            Self::Session(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for SessionRunError {}

/// What the playground's actions run on.
pub(crate) struct App {
    pub(crate) run: Arc<RunHost>,
    pub(crate) sessions: Arc<Sessions>,
    pub(crate) blueprints: Arc<Blueprints>,
    pub(crate) volumes: Arc<Volumes>,
}

impl App {
    /// A host over the project at `project`, serving `name`, its packages in
    /// `config`'s store; no blueprint is applied yet.
    #[cfg(test)]
    pub(crate) async fn for_tests(
        project: &std::path::Path,
        name: &str,
        config: super::run::RunHostConfig,
    ) -> Self {
        let state = super::super::state::StateDir::for_project(project);
        let sessions = Arc::new(Sessions::boot(&state).await.unwrap());
        let volumes = Arc::new(Volumes::boot(&state, super::volumes::VolumeTable::new()));
        let blueprints = Arc::new(Blueprints::new(
            name.to_owned(),
            Arc::clone(&volumes) as Arc<dyn super::blueprints::LocalVolumes>,
            None,
            config.package_store.clone(),
        ));
        let run =
            Arc::new(RunHost::new(config, Arc::clone(&sessions), Arc::clone(&volumes)).unwrap());
        Self {
            run,
            sessions,
            blueprints,
            volumes,
        }
    }

    /// The blueprint in force, when it is the one named `name`.
    pub(crate) async fn applied(&self, name: &str) -> Option<AppliedBlueprint> {
        self.blueprints.named(name).await
    }

    /// Admit a name from a trusted discovered project file before applying it.
    pub(crate) async fn register_blueprint(&self, name: &str) {
        self.blueprints.register(name).await;
    }

    /// [`Self::applied`], the blueprint alone.
    pub(crate) async fn blueprint(&self, name: &str) -> Option<Blueprint> {
        self.applied(name)
            .await
            .map(|applied| Blueprint::clone(&applied.blueprint))
    }

    /// Runs `program` in a fresh session, closed once it returns. A refusal before
    /// it runs comes back as the response's error, unrecorded.
    pub(crate) async fn run_program(&self, program: ProgramRun) -> ExecuteResponse {
        let session_id = uuid::Uuid::new_v4().to_string();
        let failed = |kind: ErrorKind, message: String| refused(&session_id, kind, message);
        let Some(applied) = self.applied(&program.blueprint).await else {
            return failed(
                ErrorKind::BlueprintNotFound,
                format!("blueprint '{}' is not registered", program.blueprint),
            );
        };
        let blueprint = Arc::clone(&applied.blueprint);
        let variables = match resolve_variables(&blueprint.variables, &program.variables) {
            Ok(resolved) => resolved,
            Err(error) => {
                return failed(
                    ErrorKind::InvalidRequest,
                    format!("invalid variables: {error}"),
                );
            }
        };
        if let Err(message) = filesystem(&blueprint, &variables) {
            return failed(ErrorKind::InvalidRequest, message);
        }
        let secrets = match resolve_harness_secrets(&blueprint.secrets, &program.secrets) {
            Ok(resolved) => Arc::new(resolved),
            Err(error) => {
                return failed(
                    ErrorKind::InvalidRequest,
                    format!("invalid secrets: {error}"),
                );
            }
        };
        let session = match self
            .sessions
            .open_as(session_id.clone(), &blueprint, &program.variables)
            .await
        {
            Ok(session) => session,
            Err(error) => return failed(ErrorKind::RuntimeError, error.to_string()),
        };
        let output = self
            .run
            .run(RunRequest {
                session_id: session.id.clone(),
                code: program.code,
                label: program.label,
                entry: RunEntry::Program,
                blueprint: applied,
                variables: Arc::new(session.variables),
                harness_secrets: secrets,
                client: None,
                tool_call_id: None,
                test_of: None,
                vfs: VfsSource::Session,
                finalizers: None,
                world: None,
            })
            .await;
        // A one-off session must not outlive its run.
        if let Err(error) = self.sessions.close(&session.id).await {
            warn(&format!(
                "closing a one-off run's session failed: {error:#}"
            ));
        }
        let mut response = output.response;
        // Its session's last run ended with it, so the response carries the console.
        if let Some(last_run) = output.last_run {
            response.console = last_run.console;
        }
        response
    }

    /// Starts a session bound to `start`'s blueprint, variables and harness secrets,
    /// and returns its id.
    pub(crate) async fn start_session(
        &self,
        start: SessionStart,
    ) -> Result<String, SessionStartError> {
        let Some(applied) = self.applied(&start.blueprint).await else {
            return Err(SessionStartError::UnknownBlueprint(start.blueprint));
        };
        let blueprint = &applied.blueprint;
        let variables = resolve_variables(&blueprint.variables, &start.variables)
            .map_err(|error| SessionStartError::InvalidVariables(error.to_string()))?;
        filesystem(blueprint, &variables).map_err(SessionStartError::InvalidFilesystem)?;
        let secrets = resolve_harness_secrets(&blueprint.secrets, &start.secrets)
            .map_err(|error| SessionStartError::InvalidSecrets(error.to_string()))?;
        let session = self
            .sessions
            .open(blueprint, &start.variables)
            .await
            .map_err(|error| SessionStartError::Session(error.to_string()))?;
        self.sessions.bind_secrets(&session.id, Arc::new(secrets));
        Ok(session.id)
    }

    /// Runs `program` in its session, under the variables and harness secrets the
    /// session was started with.
    pub(crate) async fn run_in_session(
        &self,
        program: SessionProgram,
    ) -> Result<ExecuteResponse, SessionRunError> {
        let session = self
            .sessions
            .get(&program.session_id)
            .await
            .map_err(|error| SessionRunError::Session(format!("{error:#}")))?
            .ok_or(SessionRunError::UnknownSession)?;
        let Some(applied) = self.applied(&session.blueprint_name).await else {
            return Err(SessionRunError::BlueprintMissing(session.blueprint_name));
        };
        let harness_secrets = if let Some(secrets) = self.sessions.secrets(&session.id) {
            secrets
        } else {
            let required = required_harness_secrets(&applied.blueprint.secrets);
            if !required.is_empty() {
                return Err(SessionRunError::SecretsRequired { required });
            }
            Arc::new(HarnessSecretBindings::new())
        };
        let output = self
            .run
            .run(RunRequest {
                session_id: session.id,
                code: program.code,
                label: program.label,
                entry: RunEntry::Session,
                blueprint: applied,
                variables: Arc::new(session.variables),
                harness_secrets,
                client: None,
                tool_call_id: None,
                test_of: None,
                vfs: VfsSource::Session,
                finalizers: None,
                world: None,
            })
            .await;
        Ok(output.response)
    }

    /// Ends a session, its finalizers run and its files and values gone. `false`
    /// when no open session has that id.
    pub(crate) async fn end_session(&self, session_id: &str) -> Result<bool> {
        self.sessions.close(session_id).await
    }

    /// The bindings an open session was started with, defaults filled; `None` when
    /// no open session has that id.
    pub(crate) async fn session_variables(
        &self,
        session_id: &str,
    ) -> Result<Option<BTreeMap<String, String>>> {
        Ok(self
            .sessions
            .get(session_id)
            .await?
            .map(|session| session.variables))
    }

    /// The blueprint fixed when an open session was started, including recovered sessions.
    pub(crate) async fn session_blueprint_name(&self, session_id: &str) -> Result<Option<String>> {
        Ok(self
            .sessions
            .get(session_id)
            .await?
            .map(|session| session.blueprint_name))
    }

    /// Runs `test`'s recorded program again under the blueprint in force.
    pub(crate) async fn test_program(&self, test: TestRun) -> Result<TestOutcome, TestError> {
        test_run::test_program(
            TestHost {
                runs: &self.run,
                sessions: &self.sessions,
                volumes: &self.volumes,
                blueprint: self.applied(&test.recorded.blueprint_name).await,
            },
            test,
        )
        .await
    }

    /// Cancels the run `execution_id`; whether it was in flight and took the cancel.
    pub(crate) fn cancel_run(&self, execution_id: &str) -> bool {
        self.run.cancel(execution_id)
    }

    /// What applying `yaml` would refuse, without changing anything.
    pub(crate) async fn check_local_blueprint(
        &self,
        yaml: &str,
    ) -> Result<Blueprint, LocalApplyError> {
        self.blueprints.check(yaml).await
    }

    /// Applies `yaml` as the blueprint in force, tagged `version_tag`. The next run
    /// discovers its MCP servers again.
    pub(crate) async fn apply_local_blueprint(
        &self,
        yaml: &str,
        version_tag: &str,
    ) -> Result<LocalApplied, LocalApplyError> {
        let applied = self.blueprints.apply(yaml, version_tag).await?;
        self.run.clear_mcp_catalogs();
        Ok(applied)
    }

    /// Drops every prepared package set, so the next run links what the store holds.
    pub(crate) fn evict_all_prepared_packages(&self) {
        self.run.clear_prepared_packages();
    }
}

/// Whether `blueprint`'s files and git checkout resolve under `variables`.
fn filesystem(blueprint: &Blueprint, variables: &VarBindings) -> Result<(), String> {
    blueprint
        .vfs
        .resolve(variables)
        .map(|_| ())
        .and_then(|()| submilli_shared::resolve_git(blueprint, variables).map(|_| ()))
        .map_err(|error| error.to_string())
}

/// The response to a run refused before it started.
fn refused(session_id: &str, kind: ErrorKind, message: String) -> ExecuteResponse {
    ExecuteResponse {
        execution_id: uuid::Uuid::new_v4().to_string(),
        session_id: session_id.to_owned(),
        result: None,
        console: Vec::new(),
        error: Some(ExecuteError::new(kind, message)),
        discovery_warnings: Vec::new(),
    }
}
