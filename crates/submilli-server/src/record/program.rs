//! Running a program from inside the process, for an embedder that starts runs itself.

use std::collections::BTreeMap;

use submilli_blueprint::HarnessSecretBindings;

use crate::app::AppState;
use crate::handlers::execute::{ExecuteRequest, ExecuteResponse, one_shot};

/// A program to run as `POST /v1/execute` would, under an explicit label.
pub struct ProgramRun {
    /// Who the run is recorded as, in place of an API token's name.
    pub label: String,
    pub blueprint: String,
    pub code: String,
    pub variables: BTreeMap<String, String>,
    /// Harness credentials for this run alone. Never persisted.
    pub secrets: HarnessSecretBindings,
}

/// Runs `program` in a fresh session, torn down once it returns, and returns what
/// `POST /v1/execute` would have. The run is audited and, with a run recorder
/// configured, recorded under the program's label.
pub async fn run_program(state: &AppState, program: ProgramRun) -> ExecuteResponse {
    let audit =
        crate::audit::ExecutionAudit::new(state.audit().clone(), &program.label, "program", None);
    let request = ExecuteRequest {
        code: program.code,
        blueprint: program.blueprint,
        variables: Some(program.variables),
        secrets: Some(program.secrets),
    };
    let (_session, response) = one_shot(
        state,
        request,
        Some(audit.clone()),
        crate::record::RunEntry::Program,
    )
    .await;
    audit.finish(response.error.is_none());
    response
}
