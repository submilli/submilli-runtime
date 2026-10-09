//! Sessions from inside the process, for an embedder that starts and runs them itself.
//!
//! Each call does what its session endpoint does: [`start_session`] is `POST /v1/sessions`,
//! [`run_in_session`] is `POST /v1/sessions/{id}/execute` without an `Idempotency-Key`,
//! and [`end_session`] is `DELETE /v1/sessions/{id}`. A session's blueprint, variables
//! and harness secrets are fixed when it starts; every run in it uses them.

use std::collections::BTreeMap;

use submilli_blueprint::HarnessSecretBindings;

use crate::app::AppState;
use crate::blueprint::StoreError;
use crate::handlers::execute::ExecuteResponse;
use crate::handlers::sessions;
use crate::session_manager::SessionError;

/// A session to start as `POST /v1/sessions` would.
pub struct SessionStart {
    pub blueprint: String,
    /// `${vars.NAME}` bindings, fixed for the whole session.
    pub variables: BTreeMap<String, String>,
    /// Harness credentials, encrypted in durable session storage.
    pub secrets: HarnessSecretBindings,
}

/// Why a session was not started.
#[derive(Debug)]
pub enum SessionStartError {
    /// No runnable blueprint has that name; `message` says whether it was never
    /// registered or is registered in a form this binary can no longer parse.
    UnknownBlueprint { name: String, message: String },
    /// The variables do not satisfy the blueprint's declarations.
    InvalidVariables(String),
    /// The blueprint's files or git checkout cannot be resolved with these variables.
    InvalidFilesystem(String),
    /// The harness secrets do not satisfy the blueprint's declarations.
    InvalidSecrets(String),
    /// The blueprint could not be read.
    Store(StoreError),
    /// The session could not be created.
    Session(SessionError),
}

impl std::fmt::Display for SessionStartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownBlueprint { message, .. } => f.write_str(message),
            Self::InvalidVariables(message) => write!(f, "invalid variables: {message}"),
            Self::InvalidFilesystem(message) => f.write_str(message),
            Self::InvalidSecrets(message) => write!(f, "invalid secrets: {message}"),
            Self::Store(_) => f.write_str("blueprint store unavailable"),
            Self::Session(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for SessionStartError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Store(error) => Some(error),
            Self::Session(error) => Some(error),
            _ => None,
        }
    }
}

/// Code to run in a started session, under an explicit label.
pub struct SessionProgram {
    /// Who the run is recorded as, in place of an API token's name.
    pub label: String,
    pub session_id: String,
    pub code: String,
}

/// Why a session could not run a program. A refused run never started, so no recorder
/// saw it.
#[derive(Debug)]
pub enum SessionRunError {
    /// No session has that id, or it has ended or expired.
    UnknownSession { session_id: String },
    /// The session's blueprint is no longer runnable; `message` says whether it was
    /// removed or is held in a form this binary can no longer parse.
    BlueprintMissing { name: String, message: String },
    /// The blueprint requires harness secrets the session no longer holds, as after a
    /// restart that could not recover them; rebind them to run again.
    SecretsRequired {
        session_id: String,
        required: Vec<String>,
    },
    /// The blueprint could not be read.
    Store(StoreError),
    /// The session's state could not be read.
    Session(SessionError),
}

impl std::fmt::Display for SessionRunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownSession { session_id } => {
                write!(f, "unknown, ended, or expired session: {session_id}")
            }
            Self::BlueprintMissing { message, .. } => {
                write!(f, "the session's blueprint no longer exists: {message}")
            }
            Self::SecretsRequired {
                session_id,
                required,
            } => write!(
                f,
                "session {session_id} requires harness secrets it does not hold: {}",
                required.join(", ")
            ),
            Self::Store(_) => f.write_str("blueprint store unavailable"),
            Self::Session(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for SessionRunError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Store(error) => Some(error),
            Self::Session(error) => Some(error),
            _ => None,
        }
    }
}

/// Starts a session bound to `start`'s blueprint, variables, and harness secrets, all
/// validated as `POST /v1/sessions` validates them, and returns its id.
pub async fn start_session(
    state: &AppState,
    start: SessionStart,
) -> Result<String, SessionStartError> {
    sessions::start(state, &start.blueprint, &start.variables, &start.secrets).await
}

/// Runs `program` in its session under the blueprint, variables, and harness secrets the
/// session was started with, and returns what `POST /v1/sessions/{id}/execute` would
/// have. The run is audited and, with a run recorder configured, recorded as
/// [`RunEntry::Session`](crate::record::RunEntry::Session) under the program's label;
/// its recorder's [`RunStart::execution_id`](crate::record::RunStart::execution_id) is
/// the id [`AppState::cancel_run`] stops it by.
pub async fn run_in_session(
    state: &AppState,
    program: SessionProgram,
) -> Result<ExecuteResponse, SessionRunError> {
    let SessionProgram {
        label,
        session_id,
        code,
    } = program;
    let audit = crate::audit::ExecutionAudit::new(
        state.audit().clone(),
        &label,
        "session",
        Some(&session_id),
    );
    let bound = match sessions::bound_for_run(state, &session_id, &code, Some(&audit)).await {
        Ok(bound) => bound,
        Err(error) => {
            audit.finish(false);
            return Err(unknown_when_gone(error, &session_id));
        }
    };
    let outcome = sessions::run(state, &session_id, bound, &code, None, Some(audit.clone())).await;
    audit.finish(outcome.response.error.is_none());
    Ok(outcome.response)
}

/// A session that ends between the two reads of [`sessions::bound_for_run`] surfaces as a
/// session-state error; to an embedder it is the same unknown session either way.
fn unknown_when_gone(error: SessionRunError, session_id: &str) -> SessionRunError {
    match error {
        SessionRunError::Session(SessionError::UnknownSession) => SessionRunError::UnknownSession {
            session_id: session_id.to_owned(),
        },
        other => other,
    }
}

/// Ends a session as `DELETE /v1/sessions/{id}` does, wiping its `per_session` files
/// immediately. `false` when no live session has that id.
pub async fn end_session(state: &AppState, session_id: &str) -> Result<bool, SessionError> {
    state.session_manager().wipe_now(session_id).await
}

/// The `${vars.NAME}` bindings a live session was started with, defaults filled; `None`
/// when no live session has that id.
pub async fn session_variables(
    state: &AppState,
    session_id: &str,
) -> Result<Option<BTreeMap<String, String>>, SessionError> {
    match state.session_manager().get(session_id).await {
        Ok(session) => Ok(Some(session.binding().variables().clone())),
        Err(SessionError::UnknownSession) => Ok(None),
        Err(error) => Err(error),
    }
}
