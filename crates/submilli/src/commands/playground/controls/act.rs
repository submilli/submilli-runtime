//! The action controls: `exec`, `bind`, `session start|end`, `recheck`, `test`,
//! `rerun`, `draft-rule`, `clear`, `cancel`, and the client side of `watch`.
//!
//! The running playground does what each action asks through [`Actions`], behind the
//! control API (`api.rs`); the CLI sends the request with the admin token and prints
//! the [`Answer`] that comes back: the result as JSON, its text form, and the exit
//! status. `draft-rule` is the exception: it reads the store and edits the blueprint
//! file, so it works with the playground stopped too, and the page reaches the same
//! function through the API.
//!
//! Every run an action starts goes through the server's embedder entry points
//! (`run_program`, `run_in_session`, `test_program`) under the playground's own label,
//! so the recorder stores it like any other run, and its result is what `show` says of
//! the stored run.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::{Arc, Mutex, PoisonError};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use submilli_blueprint::{
    Blueprint, DraftCall, DraftError, ResolutionCause, draft_allow, required_harness_secrets,
    resolve_variables,
};
use submilli_server::AppState;
use submilli_server::error::ErrorKind;
use submilli_server::handlers::execute::ExecuteResponse;
use submilli_server::record::recheck::Verdict;
use submilli_server::record::{
    ProgramRun, SessionProgram, SessionRunError, SessionStart, SessionStartError, TestError,
    TestMode, TestRun, end_session, recheck, run_in_session, run_program, session_variables,
    start_session, test_program,
};

use super::super::Output;
use super::super::client::{self, Probe, Running};
use super::super::labels;
use super::super::project;
use super::super::state::{self, RememberedBinding, StateDir};
use super::super::store::run::{DecisionRef, StoredRun, StoredTestReport};
use super::super::store::sessions::SessionEntry;
use super::super::store::{KnownSecrets, Recorder, Store, StoreError};
use super::read::{self, ShowResult};
use super::render::{
    self, EXIT_AWAITING_LIVE, EXIT_DENIED, EXIT_FAILURE, EXIT_NOT_RUNNING, EXIT_PACKAGE_RESOLUTION,
    EXIT_SUCCESS, EXIT_USAGE, Fence, Next, Page, RunRef, clean, next,
};
use super::{ReadError, Reader};

// ---- requests ------------------------------------------------------------------------------

/// `POST /api/exec`: program text only. A body naming anything else (a file path, a
/// label) is refused, so the API never reads a file it is pointed at.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExecRequest {
    pub(crate) code: String,
    /// The scaffolded example, labeled `example` rather than `assistant`.
    #[serde(default)]
    pub(crate) example: bool,
    /// A session started with `session start`; without one the run gets a fresh session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) session: Option<String>,
    /// Values over the binding, for this run alone; for a started session they must be
    /// the session's own.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) variables: BTreeMap<String, String>,
}

/// `POST /api/bind`: changes to the binding. Secret values come from the CLI's
/// environment or the local secret store, never from its command line.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BindRequest {
    #[serde(default)]
    pub(crate) variables: BTreeMap<String, String>,
    #[serde(default)]
    pub(crate) secrets: BTreeMap<String, String>,
    /// Variables and secrets to remove, by name.
    #[serde(default)]
    pub(crate) unset: Vec<String>,
    /// Remove everything first.
    #[serde(default)]
    pub(crate) clear: bool,
}

/// `POST /api/sessions/start`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SessionStartRequest {
    #[serde(default)]
    pub(crate) variables: BTreeMap<String, String>,
}

/// `POST /api/sessions/end`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SessionEndRequest {
    pub(crate) session: String,
}

/// `POST /api/recheck`, `/api/rerun`, `/api/cancel`: one run.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RunRequest {
    pub(crate) run: u64,
}

/// `POST /api/test`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TestRequest {
    pub(crate) run: u64,
    #[serde(default)]
    pub(crate) mode: Mode,
}

/// How far a test run may go live.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Mode {
    /// Stops at the first call with nothing recorded.
    #[default]
    Recorded,
    /// Lets unrecorded reads through.
    ReadsLive,
    /// Lets every unrecorded call through.
    Live,
}

impl Mode {
    fn server(self) -> TestMode {
        match self {
            Self::Recorded => TestMode::Recorded,
            Self::ReadsLive => TestMode::ReadsLive,
            Self::Live => TestMode::Live,
        }
    }
}

/// `POST /api/draft-rule`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DraftRequest {
    /// `<run>.<n>`.
    pub(crate) decision: String,
    #[serde(default)]
    pub(crate) write: bool,
    /// The name the drafted rule carries, written as its first key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) name: Option<String>,
}

// ---- answers -------------------------------------------------------------------------------

/// What an action answers: its result (what `--json` prints), the result's text form,
/// our own messages for standard error, and the exit status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Answer {
    pub(crate) exit: u8,
    pub(crate) result: Value,
    #[serde(default)]
    pub(crate) text: String,
    /// Warnings, and why an action could not answer: the playground's own words.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) notes: Vec<String>,
}

impl Answer {
    fn of<T: Serialize>(exit: u8, result: &T, text: fn(&T) -> String) -> Self {
        match serde_json::to_value(result) {
            Ok(value) => Self {
                exit,
                result: value,
                text: text(result),
                notes: Vec::new(),
            },
            Err(error) => ActError::failure(format!("encoding the result failed: {error}")).into(),
        }
    }

    fn noted(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }

    /// Prints the answer as `output` asks and returns its exit status.
    pub(crate) fn print(&self, output: Output) -> ExitCode {
        match output {
            Output::Json => println!("{}", self.result),
            Output::Text => print!("{}", self.text),
        }
        for note in &self.notes {
            eprintln!("{note}");
        }
        ExitCode::from(self.exit)
    }
}

/// Why an action could not do what it was asked.
#[derive(Debug)]
pub(crate) struct ActError {
    pub(crate) exit: u8,
    pub(crate) kind: &'static str,
    pub(crate) message: String,
    pub(crate) next: Vec<String>,
}

impl ActError {
    fn new(exit: u8, kind: &'static str, message: impl Into<String>) -> Self {
        Self {
            exit,
            kind,
            message: message.into(),
            next: Vec::new(),
        }
    }

    fn usage(kind: &'static str, message: impl Into<String>) -> Self {
        Self::new(EXIT_USAGE, kind, message)
    }

    fn failure(message: impl Into<String>) -> Self {
        Self::new(EXIT_FAILURE, "failure", message)
    }

    fn next(mut self, items: impl IntoIterator<Item = Next>) -> Self {
        self.next = next(items);
        self
    }

    pub(crate) fn not_running() -> Self {
        Self::new(
            EXIT_NOT_RUNNING,
            "not-running",
            "the playground is not running; start it with `submilli playground`",
        )
    }

    fn store(error: &StoreError) -> Self {
        Self::failure(format!("the run store failed: {error}"))
    }
}

impl From<ReadError> for ActError {
    fn from(error: ReadError) -> Self {
        Self {
            exit: error.exit(),
            kind: error.kind(),
            message: error.to_string(),
            next: error.next(),
        }
    }
}

impl From<ActError> for Answer {
    fn from(error: ActError) -> Self {
        Self {
            exit: error.exit,
            result: json!({
                "error": { "kind": error.kind, "message": error.message },
                "next": error.next,
            }),
            text: String::new(),
            notes: vec![error.message],
        }
    }
}

fn answer(result: Result<Answer, ActError>) -> Answer {
    result.unwrap_or_else(Answer::from)
}

// ---- the binding ---------------------------------------------------------------------------

/// Names and their values: variables, or harness secrets.
type Values = BTreeMap<String, String>;

/// The variables and harness-secret development values new runs get. The variables are
/// saved for the next start (see [`RememberedBinding`]); secret values are held in the
/// running playground's memory only, never written to any file.
#[derive(Debug, Clone, Default)]
pub(crate) struct Binding {
    pub(crate) variables: BTreeMap<String, String>,
    pub(crate) secrets: BTreeMap<String, String>,
    /// The last value each variable had, kept after it is unset or cleared: what a
    /// refusal for a missing variable suggests binding again.
    pub(crate) last: BTreeMap<String, String>,
    /// Secrets bound before the playground started and not bound since: their values
    /// were not kept across the restart.
    pub(crate) forgotten_secrets: BTreeSet<String>,
}

impl Binding {
    /// The binding a start begins with: the saved variables, and the saved secret names
    /// as secrets to bind again.
    pub(crate) fn remembered(saved: RememberedBinding) -> Self {
        Self {
            variables: saved.variables,
            secrets: BTreeMap::new(),
            last: saved.last,
            forgotten_secrets: saved.secret_names,
        }
    }

    /// What the next start begins with: never a secret's value.
    fn to_remember(&self) -> RememberedBinding {
        RememberedBinding {
            variables: self.variables.clone(),
            last: self.last.clone(),
            secret_names: self
                .secrets
                .keys()
                .chain(&self.forgotten_secrets)
                .cloned()
                .collect(),
        }
    }

    /// Applies `request`: clear first, then unset, then set.
    fn apply(&mut self, request: BindRequest) {
        if request.clear {
            self.variables.clear();
            self.secrets.clear();
            self.forgotten_secrets.clear();
        }
        for name in &request.unset {
            self.variables.remove(name);
            self.secrets.remove(name);
            self.forgotten_secrets.remove(name);
        }
        for (name, value) in request.variables {
            self.last.insert(name.clone(), value.clone());
            self.variables.insert(name, value);
        }
        for (name, value) in request.secrets {
            self.forgotten_secrets.remove(&name);
            self.secrets.insert(name, value);
        }
    }
}

/// What `bind` answers: the variables, and only the names of the secrets.
#[derive(Debug, Serialize)]
pub(crate) struct BindingResult {
    pub(crate) kind: &'static str,
    pub(crate) variables: BTreeMap<String, String>,
    pub(crate) secrets: Vec<String>,
    /// Secrets bound before the playground started, whose values were not kept.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) to_bind_again: Vec<String>,
    pub(crate) warnings: Vec<String>,
    pub(crate) next: Vec<String>,
}

// ---- the running playground's side ---------------------------------------------------------

/// What the running playground acts with.
pub(crate) struct Actions {
    pub(crate) app: AppState,
    pub(crate) store: Arc<Store>,
    pub(crate) recorder: Recorder,
    /// Secret values the recorder cuts out of what it stores; `bind` adds to it.
    pub(crate) secrets: KnownSecrets,
    pub(crate) binding: Mutex<Binding>,
    /// Where `bind` saves the binding's variables for the next start.
    pub(crate) binding_file: PathBuf,
    pub(crate) blueprints: Arc<dyn submilli_server::blueprint::BlueprintStore>,
    pub(crate) blueprint_name: String,
    pub(crate) blueprint_path: PathBuf,
    pub(crate) project_root: PathBuf,
    pub(crate) page: Page,
}

impl Actions {
    fn binding(&self) -> Binding {
        // Poisoned only by a panic inside one of the short updates below, which cannot
        // leave the two maps half-changed in a way that matters to a run.
        self.binding
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// A reader over the store, as the read controls use, with this playground's page.
    pub(crate) fn reader(&self) -> Result<Reader, ActError> {
        Reader::for_host(self.store.root(), self.page.clone(), &self.project_root)
            .map_err(ActError::from)
    }

    /// The blueprint in force, as the server runs it.
    async fn blueprint(&self, name: &str) -> Result<Blueprint, ActError> {
        match self.blueprints.get(name).await {
            Ok(Some(blueprint)) => Ok(blueprint),
            Ok(None) => Err(ActError::usage(
                "blueprint-gone",
                format!(
                    "blueprint `{name}` is not registered: the playground serves `{}`, and a \
                     run of another blueprint cannot run here",
                    self.blueprint_name
                ),
            )),
            Err(error) => Err(ActError::failure(format!(
                "reading blueprint `{name}` failed: {error}"
            ))),
        }
    }

    /// The variables and secrets a new run gets: the binding's, for what `blueprint`
    /// declares, under `overrides`. Refuses, naming `bind`, when a required one is missing.
    fn for_new_run(
        &self,
        blueprint: &Blueprint,
        overrides: &BTreeMap<String, String>,
    ) -> Result<(Values, Values), ActError> {
        let binding = self.binding();
        let mut variables: BTreeMap<String, String> = binding
            .variables
            .into_iter()
            .filter(|(name, _)| blueprint.variables.contains_key(name))
            .collect();
        variables.extend(overrides.clone());
        if let Err(error) = resolve_variables(&blueprint.variables, &variables) {
            return Err(self.missing_variables(&error.to_string(), Some(blueprint), &variables));
        }
        let secrets: BTreeMap<String, String> = binding
            .secrets
            .into_iter()
            .filter(|(name, _)| harness_declared(blueprint, name))
            .collect();
        let missing: Vec<String> = required_harness_secrets(&blueprint.secrets)
            .into_iter()
            .filter(|name| !secrets.contains_key(name))
            .collect();
        if !missing.is_empty() {
            return Err(missing_secrets(&missing, &binding.forgotten_secrets));
        }
        Ok((variables, secrets))
    }

    /// A refusal for missing required variables that names them in the `bind` command
    /// it suggests, with the last value each had when there is one. The names come from
    /// `blueprint` when given, else from the server's `message`.
    fn missing_variables(
        &self,
        message: &str,
        blueprint: Option<&Blueprint>,
        supplied: &Values,
    ) -> ActError {
        let mut names: Vec<String> = blueprint
            .map(|blueprint| {
                blueprint
                    .variables
                    .iter()
                    .filter(|(name, decl)| {
                        decl.required && supplied.get(*name).is_none_or(String::is_empty)
                    })
                    .map(|(name, _)| name.clone())
                    .collect()
            })
            .unwrap_or_default();
        if names.is_empty() {
            names.extend(missing_variable_named(message));
        }
        missing_variables(message, &names, &self.binding().last)
    }

    // ---- exec ----

    pub(crate) async fn exec(&self, request: ExecRequest) -> Answer {
        answer(self.try_exec(request).await)
    }

    async fn try_exec(&self, request: ExecRequest) -> Result<Answer, ActError> {
        let label = if request.example {
            labels::EXAMPLE
        } else {
            labels::ASSISTANT
        };
        let response = if let Some(session) = request.session {
            self.check_session_values(&session, &request.variables)
                .await?;
            let program = SessionProgram {
                label: label.to_owned(),
                session_id: session.clone(),
                code: request.code,
            };
            run_in_session(&self.app, program)
                .await
                .map_err(|error| session_run_error(&error, &session))?
        } else {
            let blueprint = self.blueprint(&self.blueprint_name).await?;
            let (variables, secrets) = self.for_new_run(&blueprint, &request.variables)?;
            run_program(
                &self.app,
                ProgramRun {
                    label: label.to_owned(),
                    blueprint: self.blueprint_name.clone(),
                    code: request.code,
                    variables,
                    secrets,
                },
            )
            .await
        };
        self.ran(&response, "exec", None)
    }

    /// A request naming variables for a started session must name the session's own.
    async fn check_session_values(
        &self,
        session: &str,
        asked: &BTreeMap<String, String>,
    ) -> Result<(), ActError> {
        let fixed = match session_variables(&self.app, session).await {
            Ok(Some(fixed)) => fixed,
            Ok(None) => return Err(unknown_session(session)),
            Err(error) => {
                return Err(ActError::failure(format!(
                    "reading session {} failed: {error}",
                    clean(session)
                )));
            }
        };
        let differ: Vec<String> = asked
            .iter()
            .filter(|(name, value)| fixed.get(*name) != Some(*value))
            .map(|(name, _)| clean(name))
            .collect();
        if differ.is_empty() {
            return Ok(());
        }
        Err(ActError::usage(
            "session-values-fixed",
            format!(
                "session {} was started with other values for {}; a session's variables are \
                 fixed when it starts. Start a new session with `submilli playground session \
                 start --var NAME=VALUE`",
                clean(session),
                differ.join(", ")
            ),
        )
        .next([Next::Sessions]))
    }

    /// The answer for a run that returned: what `show` says of the stored run, or, when
    /// none was stored, why it did not run.
    fn ran(
        &self,
        response: &ExecuteResponse,
        kind: &'static str,
        rerun_of: Option<u64>,
    ) -> Result<Answer, ActError> {
        let id = match self.store.run_id_of(&response.execution_id) {
            Ok(Some(id)) => id,
            Ok(None) => return Err(self.not_recorded(response)),
            Err(error) => return Err(ActError::store(&error)),
        };
        if let Some(source) = rerun_of
            && let Err(error) = self.store.append_link(id, source)
        {
            return Err(ActError::store(&error));
        }
        self.show_run(id, kind)
    }

    /// Run `id` as `show` renders it, with `kind` naming the action, and the exit status
    /// its outcome calls for.
    fn show_run(&self, id: u64, kind: &'static str) -> Result<Answer, ActError> {
        let reader = self.reader()?;
        let mut result: ShowResult = read::show(&reader, id, false)?;
        result.kind = kind;
        let stored = self
            .store
            .load_run(id)
            .map_err(|error| ActError::store(&error))?
            .ok_or(ReadError::UnknownRun(id))?;
        let exit = exit_of(&stored);
        let mut answer = Answer::of(exit, &result, render::show_text);
        if let Some(error) = &stored.error {
            match error.kind {
                ErrorKind::InvalidRequest => {
                    answer = answer.noted(
                        "the run did not start: its variables or secrets do not fit the \
                         blueprint; set them with `submilli playground bind` (see run-data for \
                         the server's message)",
                    );
                }
                ErrorKind::PackageResolution => {
                    answer = answer.noted(
                        "a package the program imports could not be built or found; see the \
                         playground's log, fix the package, and run it again",
                    );
                }
                _ => {}
            }
        }
        Ok(answer)
    }

    // ---- bind ----

    /// The binding, after applying `request` when one is given.
    pub(crate) async fn bind(&self, request: Option<BindRequest>) -> Answer {
        let changed = request.is_some();
        let (binding, saved) = {
            let mut binding = self.binding.lock().unwrap_or_else(PoisonError::into_inner);
            let mut saved = Ok(());
            if let Some(request) = request {
                for value in request.secrets.values() {
                    self.secrets.add(value);
                }
                binding.apply(request);
                // Saved under the lock, so two binds write in the order they applied.
                saved = state::write_binding_at(&self.binding_file, &binding.to_remember());
            }
            (binding.clone(), saved)
        };
        let mut warnings = if changed {
            self.open_sessions_that_differ(&binding).await
        } else {
            Vec::new()
        };
        if let Err(error) = saved {
            warnings.push(format!(
                "the binding is in force, but saving it for the next start failed: {error:#}"
            ));
        }
        let result = BindingResult {
            kind: "binding",
            variables: binding.variables,
            secrets: binding.secrets.into_keys().collect(),
            to_bind_again: binding.forgotten_secrets.into_iter().collect(),
            warnings: warnings.clone(),
            next: Vec::new(),
        };
        warnings.into_iter().fold(
            Answer::of(EXIT_SUCCESS, &result, binding_text),
            |answer, warning| answer.noted(format!("warning: {warning}")),
        )
    }

    /// A warning for each open started session whose fixed values differ from `binding`.
    async fn open_sessions_that_differ(&self, binding: &Binding) -> Vec<String> {
        let Ok(log) = self.store.session_log() else {
            return Vec::new();
        };
        let mut open: Vec<String> = Vec::new();
        for line in log {
            match line.entry {
                SessionEntry::Started { session_id, .. } => open.push(session_id),
                SessionEntry::Ended { session_id } => open.retain(|id| *id != session_id),
            }
        }
        let mut warnings = Vec::new();
        for session in open {
            let Ok(Some(fixed)) = session_variables(&self.app, &session).await else {
                continue;
            };
            let differ: Vec<String> = binding
                .variables
                .iter()
                .filter_map(|(name, value)| {
                    let held = fixed.get(name)?;
                    (held != value).then(|| format!("{}={}", clean(name), clean(held)))
                })
                .collect();
            if !differ.is_empty() {
                warnings.push(format!(
                    "open session {} keeps {}; its runs do not change. End it with `submilli \
                     playground session end {}` and start a new one to use the new values",
                    clean(&session),
                    differ.join(", "),
                    clean(&session)
                ));
            }
        }
        warnings
    }

    /// The binding as `status` shows it: never a secret's value.
    pub(crate) fn binding_view(&self) -> super::super::host::BindingView {
        let binding = self.binding();
        super::super::host::BindingView {
            variables: binding.variables,
            secrets: binding.secrets.into_keys().collect(),
            to_bind_again: binding.forgotten_secrets.into_iter().collect(),
        }
    }

    /// The binding with secret values, for the bridge.
    pub(crate) fn bridge_binding(&self) -> Value {
        let binding = self.binding();
        json!({ "variables": binding.variables, "secrets": binding.secrets })
    }

    // ---- sessions ----

    pub(crate) async fn session_start(&self, request: SessionStartRequest) -> Answer {
        answer(self.try_session_start(request).await)
    }

    async fn try_session_start(&self, request: SessionStartRequest) -> Result<Answer, ActError> {
        let blueprint = self.blueprint(&self.blueprint_name).await?;
        let (variables, secrets) = self.for_new_run(&blueprint, &request.variables)?;
        let session = start_session(
            &self.app,
            SessionStart {
                blueprint: self.blueprint_name.clone(),
                variables: variables.clone(),
                secrets,
            },
        )
        .await
        .map_err(|error| match &error {
            SessionStartError::InvalidVariables(message) => {
                self.missing_variables(message, Some(&blueprint), &variables)
            }
            _ => session_start_error(&error),
        })?;
        let variables = match session_variables(&self.app, &session).await {
            Ok(Some(fixed)) => fixed,
            _ => variables,
        };
        self.store
            .append_session(SessionEntry::Started {
                session_id: session.clone(),
                variables: variables.clone(),
                label: labels::ASSISTANT.to_owned(),
            })
            .map_err(|error| ActError::store(&error))?;
        let result = SessionResult {
            kind: "session",
            session: session.clone(),
            open: true,
            variables,
            next: next([Next::Watch(session.clone()), Next::RunsInSession(session)]),
        };
        Ok(Answer::of(EXIT_SUCCESS, &result, session_text))
    }

    pub(crate) async fn session_end(&self, request: SessionEndRequest) -> Answer {
        let session = request.session;
        let ended = match end_session(&self.app, &session).await {
            Ok(true) => true,
            Ok(false) => return unknown_session(&session).into(),
            Err(error) => {
                return ActError::failure(format!(
                    "ending session {} failed: {error}",
                    clean(&session)
                ))
                .into();
            }
        };
        if let Err(error) = self.store.append_session(SessionEntry::Ended {
            session_id: session.clone(),
        }) {
            return ActError::store(&error).into();
        }
        let result = SessionResult {
            kind: "session",
            session: session.clone(),
            open: !ended,
            variables: BTreeMap::new(),
            next: next([Next::RunsInSession(session), Next::Sessions]),
        };
        Answer::of(EXIT_SUCCESS, &result, session_text)
    }

    // ---- recheck ----

    pub(crate) async fn recheck(&self, request: RunRequest) -> Answer {
        answer(self.try_recheck(request.run).await)
    }

    async fn try_recheck(&self, id: u64) -> Result<Answer, ActError> {
        let stored = self.load(id)?;
        let blueprint = self.blueprint(&stored.recording.blueprint_name).await?;
        let binding = self.binding();
        let report = recheck(&blueprint, &binding.variables, &stored.recording);
        let in_force = self
            .store
            .changes()
            .ok()
            .and_then(|changes| changes.current().map(|version| version.version.to_string()));
        // The file as it is now, to cite the rules that decide now by their lines.
        let current_text = std::fs::read_to_string(&self.blueprint_path).ok();
        let mut changes = Vec::new();
        let mut contexts = BTreeMap::new();
        let mut cant_tell = Vec::new();
        for (position, check) in report.decisions.iter().enumerate() {
            let decision = DecisionRef {
                run: id,
                n: position.saturating_add(1),
            }
            .to_string();
            let change = match &check.verdict {
                Verdict::NewlyAllowed => "newly-allowed",
                Verdict::NewlyDenied => "newly-denied",
                Verdict::UnchangedDifferentRule { .. } => "different-rule",
                Verdict::CantTell { .. } => {
                    cant_tell.push(decision);
                    continue;
                }
                Verdict::Unchanged | Verdict::NotReached { .. } => continue,
            };
            if let Some(record) = stored.recording.decisions.get(position)
                && !record.context.is_null()
            {
                contexts.insert(decision.clone(), record.context.clone());
            }
            changes.push(RecheckChange {
                decision,
                change,
                caller: check.caller.clone(),
                capability: check.capability.clone(),
                now_by: check.now.as_ref().map_or_else(
                    || "unknown".to_owned(),
                    |now| cause_text(&now.cause, current_text.as_deref()),
                ),
            });
        }
        let mut suggestions: Vec<Next> = changes
            .iter()
            .filter_map(|change| change.decision.parse().ok())
            .take(3)
            .map(Next::Explain)
            .collect();
        if !changes.is_empty() {
            suggestions.push(Next::Test(id));
        }
        suggestions.push(Next::Show(id));
        let result = RecheckResult {
            kind: "recheck",
            header: RunRef {
                run: id,
                page: self.page.run(id),
                blueprint_version: stored.recording.blueprint_version.clone(),
                source: stored.label.clone(),
                decision_refs: changes
                    .iter()
                    .map(|change| change.decision.clone())
                    .collect(),
            },
            in_force,
            ran: false,
            newly_allowed: report.summary.newly_allowed,
            newly_denied: report.summary.newly_denied,
            different_rule: report.summary.different_rule,
            unchanged: report.summary.unchanged,
            cant_tell,
            changes,
            variables_filled: report.variables.filled.keys().cloned().collect(),
            variables_dropped: report.variables.dropped.clone(),
            recording_truncated: report.recording_truncated,
            next: next(suggestions),
            untrusted: RecheckUntrusted { contexts },
        };
        Ok(Answer::of(EXIT_SUCCESS, &result, recheck_text))
    }

    fn load(&self, id: u64) -> Result<StoredRun, ActError> {
        match self.store.load_run(id) {
            Ok(Some(run)) => Ok(run),
            Ok(None) => Err(ReadError::UnknownRun(id).into()),
            Err(error) => Err(ActError::store(&error)),
        }
    }

    // ---- test ----

    pub(crate) async fn test(&self, request: TestRequest) -> Answer {
        answer(self.try_test(request).await)
    }

    async fn try_test(&self, request: TestRequest) -> Result<Answer, ActError> {
        let source = request.run;
        let stored = self.load(source)?;
        let blueprint = self.blueprint(&stored.recording.blueprint_name).await?;
        let binding = self.binding();
        let secrets = binding
            .secrets
            .into_iter()
            .filter(|(name, _)| harness_declared(&blueprint, name))
            .collect();
        // What the test run will have: the recorded run's variables, then the binding's.
        let mut supplied = binding.variables.clone();
        supplied.extend(stored.recording.variables.clone());
        let forgotten = binding.forgotten_secrets;
        let outcome = test_program(
            &self.app,
            TestRun {
                label: labels::TEST.to_owned(),
                recorded: stored.recording,
                bindings: binding.variables,
                mode: request.mode.server(),
                secrets: Some(secrets),
            },
        )
        .await
        .map_err(|error| match &error {
            TestError::InvalidVariables(message) => {
                self.missing_variables(message, Some(&blueprint), &supplied)
            }
            _ => test_error(&error, source, &forgotten),
        })?;
        let id = match self.store.run_id_of(&outcome.response.execution_id) {
            Ok(Some(id)) => id,
            Ok(None) => return Err(self.not_recorded(&outcome.response)),
            Err(error) => return Err(ActError::store(&error)),
        };
        self.keep_report(id, &outcome.report)?;
        self.show_run(id, "test")
    }

    /// Keeps a test run's report with the run, redacted like the run itself.
    fn keep_report(
        &self,
        id: u64,
        report: &submilli_server::record::TestReport,
    ) -> Result<(), ActError> {
        let mut run = self.load(id)?;
        run.test_report = StoredTestReport::of(report);
        let redacted = self
            .secrets
            .redact_record(&run)
            .map_err(|error| ActError::failure(format!("the test report was not kept: {error}")))?;
        self.store
            .rewrite_run(&redacted.record)
            .map_err(|error| ActError::store(&error))
    }

    // ---- rerun ----

    pub(crate) async fn rerun(&self, request: RunRequest) -> Answer {
        answer(self.try_rerun(request.run).await)
    }

    async fn try_rerun(&self, source: u64) -> Result<Answer, ActError> {
        let stored = self.load(source)?;
        let Some(code) = stored.recording.code.clone() else {
            return Err(no_program(&stored, "rerun"));
        };
        let blueprint = self.blueprint(&stored.recording.blueprint_name).await?;
        let (variables, secrets) = self.for_new_run(&blueprint, &BTreeMap::new())?;
        let response = run_program(
            &self.app,
            ProgramRun {
                label: labels::RERUN.to_owned(),
                blueprint: stored.recording.blueprint_name.clone(),
                code,
                variables,
                secrets,
            },
        )
        .await;
        self.ran(&response, "rerun", Some(source))
    }

    // ---- draft-rule, clear, cancel ----

    pub(crate) fn draft(&self, request: &DraftRequest) -> Answer {
        let decision = match request.decision.parse::<DecisionRef>() {
            Ok(decision) => decision,
            Err(message) => return ActError::usage("invalid-decision", message).into(),
        };
        answer(
            draft_rule(
                &self.store,
                &self.blueprint_path,
                &self.project_root,
                decision,
                request.write,
                request.name.as_deref(),
                &self.page,
            )
            .map(|result| Answer::of(EXIT_SUCCESS, &result, draft_text)),
        )
    }

    pub(crate) fn clear(&self) -> Answer {
        match self.store.clear() {
            Ok(removed) => {
                let result = ClearResult {
                    kind: "clear",
                    removed,
                    next: next([Next::Runs]),
                };
                Answer::of(EXIT_SUCCESS, &result, |result| {
                    format!(
                        "Cleared {} run{}; run ids keep counting, and a new audit window starts \
                         now.\n",
                        result.removed,
                        if result.removed == 1 { "" } else { "s" }
                    )
                })
            }
            Err(error) => ActError::store(&error).into(),
        }
    }

    pub(crate) async fn cancel(&self, request: &RunRequest) -> Answer {
        let run = request.run;
        let Some(execution_id) = self.recorder.in_flight(run) else {
            let stored = matches!(self.store.load_run(run), Ok(Some(_)));
            let message = if stored {
                format!("run {run} already finished; there is nothing to cancel")
            } else {
                format!("no run {run} is in flight; list the runs with `submilli playground runs`")
            };
            return ActError::usage("not-in-flight", message)
                .next([Next::Runs])
                .into();
        };
        // The playground numbers a run as it starts, a moment before the server can
        // cancel it, so a cancel that lands in between waits for it.
        let deadline = tokio::time::Instant::now() + CANCEL_REGISTRATION_WAIT;
        let mut cancelled = self.app.cancel_run(&execution_id);
        while !cancelled
            && self.recorder.in_flight(run).is_some()
            && tokio::time::Instant::now() < deadline
        {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            cancelled = self.app.cancel_run(&execution_id);
        }
        let result = CancelResult {
            kind: "cancel",
            run,
            cancelled,
            next: next([Next::Show(run)]),
        };
        let exit = if cancelled { EXIT_SUCCESS } else { EXIT_USAGE };
        Answer::of(exit, &result, |result| {
            if result.cancelled {
                format!(
                    "Cancelling run {}; it ends with a cancelled outcome once its calls drain.\n",
                    result.run
                )
            } else {
                format!(
                    "Run {} finished before it could be cancelled.\n",
                    result.run
                )
            }
        })
    }
}

/// How long `cancel` waits for a run it found in flight to become cancellable.
const CANCEL_REGISTRATION_WAIT: std::time::Duration = std::time::Duration::from_secs(2);

/// Whether `blueprint` declares `name` as a secret its harness supplies.
fn harness_declared(blueprint: &Blueprint, name: &str) -> bool {
    matches!(
        blueprint.secrets.get(name),
        Some(submilli_blueprint::SecretSource::Harness(_))
    )
}

/// The refusal for missing required variables `names`, suggesting the `bind` command
/// that sets them: each with its value in `last` when it had one, else `VALUE`.
fn missing_variables(message: &str, names: &[String], last: &Values) -> ActError {
    let pairs: Vec<String> = names
        .iter()
        .map(|name| {
            let value = last
                .get(name)
                .filter(|value| !value.is_empty())
                .map_or_else(|| "VALUE".to_owned(), |value| shell_word(value));
            format!("{}={value}", shell_word(name))
        })
        .collect();
    let (values, pairs) = match pairs.as_slice() {
        [] => ("the value", "NAME=VALUE".to_owned()),
        [_] => ("the value", pairs.join(" ")),
        _ => ("the values", pairs.join(" ")),
    };
    ActError::usage(
        "missing-variables",
        format!("{message}; set {values} new runs use with `submilli playground bind {pairs}`"),
    )
}

/// The variable a resolver message names: `required variable 'NAME' was not supplied`.
fn missing_variable_named(message: &str) -> Option<String> {
    let rest = message.split("required variable '").nth(1)?;
    let (name, _) = rest.split_once("' was not supplied")?;
    (!name.is_empty()).then(|| name.to_owned())
}

/// `text` as one shell word on one line of text.
fn shell_word(text: &str) -> String {
    clean(&render::shell_word(text))
}

/// What a missing-secret refusal adds when one of `names` was bound before the
/// playground restarted.
fn restart_note(names: &[String], forgotten: &BTreeSet<String>) -> &'static str {
    if names.iter().any(|name| forgotten.contains(name)) {
        " (it was bound before the playground restarted; secret values are not kept across \
         restarts, so bind it again)"
    } else {
        ""
    }
}

fn missing_secrets(names: &[String], forgotten: &BTreeSet<String>) -> ActError {
    let note = restart_note(names, forgotten);
    let names: Vec<String> = names.iter().map(|name| clean(name)).collect();
    ActError::usage(
        "missing-secrets",
        format!(
            "the blueprint requires harness secret{} {} with no development value{note}; set it \
             from your environment or the local secret store with `submilli playground bind \
             --secret {}`",
            if names.len() == 1 { "" } else { "s" },
            names.join(", "),
            names.first().map_or("NAME", String::as_str)
        ),
    )
}

fn unknown_session(session: &str) -> ActError {
    ActError::usage(
        "unknown-session",
        format!(
            "no open session {}; it ended, expired, or was never started. List sessions with \
             `submilli playground sessions`",
            clean(session)
        ),
    )
    .next([Next::Sessions])
}

fn session_run_error(error: &SessionRunError, session: &str) -> ActError {
    match error {
        SessionRunError::UnknownSession { .. } => unknown_session(session),
        SessionRunError::BlueprintMissing { .. } => {
            ActError::usage("blueprint-gone", error.to_string())
        }
        SessionRunError::SecretsRequired { required, .. } => ActError::usage(
            "missing-secrets",
            format!(
                "session {} no longer holds harness secret{} {} (secret values are not kept \
                 across restarts); set {} with `submilli playground bind --secret NAME` and \
                 start a new session",
                clean(session),
                if required.len() == 1 { "" } else { "s" },
                clean(&required.join(", ")),
                if required.len() == 1 { "it" } else { "them" }
            ),
        ),
        SessionRunError::Store(_) | SessionRunError::Session(_) => {
            ActError::failure(error.to_string())
        }
    }
}

fn session_start_error(error: &SessionStartError) -> ActError {
    match error {
        SessionStartError::InvalidVariables(message) => {
            missing_variables(message, &[], &BTreeMap::new())
        }
        SessionStartError::InvalidSecrets(message) => ActError::usage(
            "missing-secrets",
            format!(
                "{message}; set development values with `submilli playground bind --secret NAME`"
            ),
        ),
        SessionStartError::UnknownBlueprint { .. } | SessionStartError::InvalidFilesystem(_) => {
            ActError::usage("blueprint-gone", error.to_string())
        }
        SessionStartError::Store(_) | SessionStartError::Session(_) => {
            ActError::failure(error.to_string())
        }
    }
}

fn test_error(error: &TestError, source: u64, forgotten: &BTreeSet<String>) -> ActError {
    match error {
        TestError::NoProgram { .. } => ActError::usage(
            "no-program",
            format!(
                "run {source} recorded no program (a file tool, or a retry answered from an \
                 earlier run), so there is nothing to test; test the run that executed the \
                 program instead"
            ),
        )
        .next([Next::Show(source), Next::Runs]),
        TestError::BlueprintNotFound(_) => ActError::usage("blueprint-gone", error.to_string()),
        TestError::InvalidVariables(message) => missing_variables(message, &[], &BTreeMap::new()),
        TestError::InvalidSecrets(message) => {
            let note = if forgotten.is_empty() {
                ""
            } else {
                " (secret values are not kept across restarts, so a secret bound before the \
                 playground restarted has to be bound again)"
            };
            ActError::usage(
                "missing-secrets",
                format!(
                    "{message}{note}; set development values with `submilli playground bind \
                     --secret NAME`"
                ),
            )
        }
        TestError::Store(_) | TestError::NoRecorder | TestError::LocalState(_) => {
            ActError::failure(error.to_string())
        }
    }
}

fn no_program(run: &StoredRun, action: &str) -> ActError {
    let source = run
        .test_of
        .as_ref()
        .and_then(|link| link.run)
        .map_or_else(String::new, |of| format!(" (run {of})"));
    ActError::usage(
        "no-program",
        format!(
            "run {} recorded no program (a file tool, or a retry answered from an earlier run), \
             so there is nothing to {action}; {action} the run that executed the program{source}",
            run.id
        ),
    )
    .next([Next::Show(run.id), Next::Runs])
}

impl Actions {
    /// A run that returned without being stored: why it did not run, when it says.
    fn not_recorded(&self, response: &ExecuteResponse) -> ActError {
        match response.error.as_ref() {
            Some(error) if error.kind == ErrorKind::InvalidRequest => {
                self.missing_variables(&clean(&error.message), None, &BTreeMap::new())
            }
            _ => not_recorded(response),
        }
    }
}

/// A run that returned without being stored: why it did not run, when it says.
fn not_recorded(response: &ExecuteResponse) -> ActError {
    let Some(error) = &response.error else {
        return ActError::failure(
            "the run finished but was not stored; the playground's log says why",
        );
    };
    let message = clean(&error.message);
    match error.kind {
        ErrorKind::InvalidRequest => missing_variables(&message, &[], &BTreeMap::new()),
        ErrorKind::PackageResolution => {
            ActError::new(EXIT_PACKAGE_RESOLUTION, "package-resolution", message)
        }
        ErrorKind::BlueprintNotFound => ActError::usage("blueprint-gone", message),
        _ => ActError::failure(format!(
            "the run did not start and was not stored: {message}"
        )),
    }
}

/// The exit status a stored run's outcome calls for.
fn exit_of(run: &StoredRun) -> u8 {
    if run
        .test_report
        .as_ref()
        .is_some_and(|report| report.stopped.is_some())
    {
        return EXIT_AWAITING_LIVE;
    }
    match run.error.as_ref().map(|error| error.kind) {
        None => EXIT_SUCCESS,
        Some(ErrorKind::PermissionDenied) => EXIT_DENIED,
        Some(ErrorKind::PackageResolution) => EXIT_PACKAGE_RESOLUTION,
        Some(ErrorKind::InvalidRequest | ErrorKind::BlueprintNotFound) => EXIT_USAGE,
        Some(_) => EXIT_FAILURE,
    }
}

fn cause_text(cause: &ResolutionCause, text: Option<&str>) -> String {
    match cause {
        ResolutionCause::Rule(rule) => render::rule_label(
            &rule.caller,
            rule.index,
            rule.name.as_deref(),
            text.and_then(|text| line_of(text, &rule.caller, rule.index)),
        ),
        ResolutionCause::Default { .. } => "the default".to_owned(),
    }
}

/// The line rule `index` of `caller` starts on in `text`, when the text locates it.
fn line_of(text: &str, caller: &str, index: usize) -> Option<usize> {
    match submilli_blueprint::locate_rule(text, caller, index) {
        submilli_blueprint::Citation::Line(location) => Some(location.line),
        submilli_blueprint::Citation::Index { .. } => None,
    }
}

// ---- results -------------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub(crate) struct SessionResult {
    pub(crate) kind: &'static str,
    pub(crate) session: String,
    pub(crate) open: bool,
    pub(crate) variables: BTreeMap<String, String>,
    pub(crate) next: Vec<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct ClearResult {
    pub(crate) kind: &'static str,
    pub(crate) removed: usize,
    pub(crate) next: Vec<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct CancelResult {
    pub(crate) kind: &'static str,
    pub(crate) run: u64,
    pub(crate) cancelled: bool,
    pub(crate) next: Vec<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct RecheckResult {
    pub(crate) kind: &'static str,
    /// `decision_refs` are the decisions that changed.
    #[serde(flatten)]
    pub(crate) header: RunRef,
    /// The blueprint version the decisions were resolved under.
    pub(crate) in_force: Option<String>,
    /// Always false: a re-check runs nothing and records nothing.
    pub(crate) ran: bool,
    pub(crate) newly_allowed: usize,
    pub(crate) newly_denied: usize,
    pub(crate) different_rule: usize,
    pub(crate) unchanged: usize,
    /// Decisions whose recording is too cut to resolve again.
    pub(crate) cant_tell: Vec<String>,
    pub(crate) changes: Vec<RecheckChange>,
    pub(crate) variables_filled: Vec<String>,
    pub(crate) variables_dropped: Vec<String>,
    pub(crate) recording_truncated: bool,
    pub(crate) next: Vec<String>,
    pub(crate) untrusted: RecheckUntrusted,
}

#[derive(Debug, Serialize)]
pub(crate) struct RecheckChange {
    pub(crate) decision: String,
    /// `newly-allowed`, `newly-denied`, or `different-rule`.
    pub(crate) change: &'static str,
    pub(crate) caller: String,
    pub(crate) capability: String,
    /// What decides it now.
    pub(crate) now_by: String,
}

#[derive(Debug, Default, Serialize)]
pub(crate) struct RecheckUntrusted {
    /// Each changed decision's recorded context, by its ref.
    pub(crate) contexts: BTreeMap<String, Value>,
}

#[derive(Debug, Serialize)]
pub(crate) struct DraftResult {
    pub(crate) kind: &'static str,
    /// `page` links the decision.
    #[serde(flatten)]
    pub(crate) header: RunRef,
    pub(crate) decision: String,
    pub(crate) caller: String,
    pub(crate) capability: String,
    /// The blueprint file the rule goes in.
    pub(crate) file: PathBuf,
    /// The same file, relative to the project root when it is inside it.
    pub(crate) file_in_project: PathBuf,
    /// The rule's name, when it was drafted with one.
    pub(crate) name: Option<String>,
    /// The rule's first and last line in the file once inserted, from 1.
    pub(crate) lines: [usize; 2],
    /// The deny rule the draft goes above and overrides for this call; `None` when the
    /// default decided.
    pub(crate) overrides: Option<OverriddenRule>,
    pub(crate) written: bool,
    pub(crate) next: Vec<String>,
    pub(crate) untrusted: DraftUntrusted,
}

#[derive(Debug, Serialize)]
pub(crate) struct OverriddenRule {
    pub(crate) caller: String,
    /// Zero-based, as the blueprint's rule list counts, before the draft goes in.
    pub(crate) index: usize,
    /// One-based, as the text cites it, before the draft goes in.
    pub(crate) position: usize,
    pub(crate) name: Option<String>,
    /// Its line in the file before the draft goes in, when located.
    pub(crate) line: Option<usize>,
}

/// The rule's text embeds values from the run's call, so all of it is the run's data.
#[derive(Debug, Serialize)]
pub(crate) struct DraftUntrusted {
    pub(crate) rule: String,
}

// ---- draft-rule ----------------------------------------------------------------------------

/// Drafts a rule allowing exactly decision `decision`'s call into the blueprint file,
/// named `name` when one is given, and with `write`, writes it there: only when the file
/// is still what the draft was made from, in one rename that keeps the file's mode. The
/// watcher then applies it as any save.
pub(crate) fn draft_rule(
    store: &Store,
    blueprint_path: &Path,
    project_root: &Path,
    decision: DecisionRef,
    write: bool,
    name: Option<&str>,
    page: &Page,
) -> Result<DraftResult, ActError> {
    let run = match store.load_run(decision.run) {
        Ok(Some(run)) => run,
        Ok(None) => return Err(ReadError::UnknownRun(decision.run).into()),
        Err(error) => return Err(ActError::store(&error)),
    };
    let record = run.decision(decision.n).ok_or(ReadError::UnknownDecision {
        decision,
        count: run.recording.decisions.len(),
    })?;
    if record.allowed {
        return Err(ActError::usage(
            "not-a-denial",
            format!("decision {decision} was allowed; there is no refusal to draft a rule from"),
        )
        .next([Next::Explain(decision)]));
    }
    if record.source != "policy" {
        return Err(ActError::usage(
            "not-a-policy-decision",
            format!(
                "decision {decision} was refused ahead of the policy, not by a rule or the \
                 default, so no rule can allow it"
            ),
        )
        .next([Next::Explain(decision)]));
    }
    if record.context_truncated || record.payload_dropped {
        return Err(ActError::failure(format!(
            "decision {decision}'s recorded context was cut to fit the recorder, so a rule \
             drafted from it could match the wrong call; add the rule by hand"
        ))
        .next([Next::Explain(decision)]));
    }
    let bytes = std::fs::read(blueprint_path).map_err(|error| {
        ActError::usage(
            "blueprint-gone",
            format!("reading {}: {error}", blueprint_path.display()),
        )
    })?;
    let text = String::from_utf8(bytes.clone()).map_err(|_| {
        ActError::failure(format!("{} is not UTF-8 text", blueprint_path.display()))
    })?;
    // The version the run was decided under: the draft is refused when the file no
    // longer decides the call through the same rule, or the default.
    let under = store.changes().ok().and_then(|changes| {
        let tag = run.recording.blueprint_version.as_deref()?;
        changes
            .versions
            .iter()
            .find(|version| version.version.to_string() == tag)
            .and_then(|version| submilli_blueprint::parse(&version.bytes).ok())
    });
    let under = match under {
        Some(blueprint) => blueprint,
        None => submilli_blueprint::parse(&text).map_err(|error| {
            ActError::failure(format!(
                "{} does not parse: {error}",
                blueprint_path.display()
            ))
        })?,
    };
    let draft = draft_allow(
        &text,
        &DraftCall {
            caller: &record.caller,
            capability: &record.capability,
            context: &record.context,
            vars: &run.recording.variables,
            name,
            decided_under: &under,
        },
    )
    .map_err(|error| draft_error(&error, decision))?;
    if write {
        write_blueprint(blueprint_path, &bytes, draft.text.as_bytes())?;
    }
    let mut suggestions = vec![Next::Explain(decision), Next::Show(run.id)];
    if write {
        suggestions = vec![Next::Recheck(run.id), Next::Test(run.id), Next::Changes];
    }
    Ok(DraftResult {
        kind: "draft",
        header: RunRef {
            run: run.id,
            page: page.decision(decision),
            blueprint_version: run.recording.blueprint_version.clone(),
            source: run.label.clone(),
            decision_refs: vec![decision.to_string()],
        },
        decision: decision.to_string(),
        caller: record.caller.clone(),
        capability: record.capability.clone(),
        file: blueprint_path.to_path_buf(),
        file_in_project: blueprint_path
            .strip_prefix(project_root)
            .unwrap_or(blueprint_path)
            .to_path_buf(),
        name: name.map(str::to_owned),
        lines: [*draft.lines.start(), *draft.lines.end()],
        overrides: draft.overrides.map(|rule| OverriddenRule {
            line: line_of(&text, &rule.caller, rule.index),
            position: rule.index.saturating_add(1),
            caller: rule.caller,
            index: rule.index,
            name: rule.name,
        }),
        written: write,
        next: next(suggestions),
        untrusted: DraftUntrusted { rule: draft.rule },
    })
}

fn draft_error(error: &DraftError, decision: DecisionRef) -> ActError {
    let name_refused = match error {
        DraftError::EmptyName | DraftError::DuplicateName { .. } => true,
        DraftError::UnsafeValue { field, .. } => field == "name",
        _ => false,
    };
    if name_refused {
        return ActError::usage(
            "invalid-name",
            format!("no rule drafted for {decision}: {error}; pick another `--name`"),
        )
        .next([Next::Explain(decision)]);
    }
    let error_kind = match error {
        DraftError::AlreadyAllowed => "already-allowed",
        DraftError::DecisionChanged { .. } => "decision-changed",
        DraftError::CurrentInvalid(_) => "blueprint-invalid",
        _ => "draft-refused",
    };
    let message = match error {
        DraftError::AlreadyAllowed | DraftError::DecisionChanged { .. } => format!(
            "{error}; re-check run {} against the blueprint in force",
            decision.run
        ),
        _ => format!("no rule drafted for {decision}: {error}"),
    };
    ActError::new(EXIT_FAILURE, error_kind, message).next([
        Next::Recheck(decision.run),
        Next::Explain(decision),
        Next::Changes,
    ])
}

/// Replaces the blueprint file with `new`, only when it still holds `drafted_from`, in one
/// rename in its own directory that keeps its permissions.
fn write_blueprint(path: &Path, drafted_from: &[u8], new: &[u8]) -> Result<(), ActError> {
    let failed =
        |error: std::io::Error| ActError::failure(format!("writing {}: {error}", path.display()));
    let now = std::fs::read(path).map_err(failed)?;
    if now != drafted_from {
        return Err(ActError::failure(format!(
            "{} changed while the rule was drafted; nothing was written. Draft it again",
            path.display()
        )));
    }
    let permissions = std::fs::metadata(path).map_err(failed)?.permissions();
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let mut staged = tempfile::Builder::new()
        .prefix(".submilli-draft-")
        .tempfile_in(dir)
        .map_err(failed)?;
    staged.write_all(new).map_err(failed)?;
    staged.as_file().sync_all().map_err(failed)?;
    std::fs::set_permissions(staged.path(), permissions).map_err(failed)?;
    staged
        .persist(path)
        .map(drop)
        .map_err(|error| failed(error.error))
}

// ---- text forms ----------------------------------------------------------------------------

fn next_lines(out: &mut String, next: &[String]) {
    use std::fmt::Write as _;
    if let Some((first, rest)) = next.split_first() {
        let _ = writeln!(out, "next: {first}");
        for command in rest {
            let _ = writeln!(out, "      {command}");
        }
    }
}

fn binding_text(result: &BindingResult) -> String {
    use std::fmt::Write as _;
    let mut out =
        String::from("binding for new runs, test runs' missing variables, and new sessions:\n");
    if result.variables.is_empty() {
        out.push_str("  variables: none\n");
    }
    for (name, value) in &result.variables {
        let _ = writeln!(out, "  {}={}", clean(name), clean(value));
    }
    if result.secrets.is_empty() {
        out.push_str("  secrets: none\n");
    } else {
        let _ = writeln!(
            out,
            "  secrets: {} (values held in the playground's memory only)",
            clean(&result.secrets.join(", "))
        );
    }
    if !result.to_bind_again.is_empty() {
        let _ = writeln!(
            out,
            "  to bind again: {} (secret values are not kept across restarts; `submilli \
             playground bind --secret NAME`)",
            clean(&result.to_bind_again.join(", "))
        );
    }
    out
}

fn session_text(result: &SessionResult) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    if result.open {
        let vars: Vec<String> = result
            .variables
            .iter()
            .map(|(name, value)| format!("{}={}", clean(name), clean(value)))
            .collect();
        let _ = writeln!(
            out,
            "session {} started with {} (fixed for every run in it)",
            clean(&result.session),
            if vars.is_empty() {
                "no variables".to_owned()
            } else {
                vars.join(", ")
            }
        );
        let _ = writeln!(
            out,
            "run in it with `submilli playground exec <file> --session {}`",
            clean(&result.session)
        );
    } else {
        let _ = writeln!(out, "session {} ended", clean(&result.session));
    }
    next_lines(&mut out, &result.next);
    out
}

fn recheck_text(result: &RecheckResult) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    let mut fence = Fence::default();
    let _ = writeln!(
        out,
        "recheck of run {} · {} · {} under {} (ran nothing, recorded nothing)",
        result.header.run,
        clean(&result.header.source),
        render::version_text(result.header.blueprint_version.as_deref()),
        render::version_text(result.in_force.as_deref())
    );
    let _ = writeln!(
        out,
        "page: {}",
        result.header.page.as_deref().unwrap_or(render::NO_PAGE)
    );
    let _ = writeln!(
        out,
        "newly allowed {}, newly denied {}, different rule {}, unchanged {}",
        result.newly_allowed, result.newly_denied, result.different_rule, result.unchanged
    );
    for change in &result.changes {
        let _ = writeln!(
            out,
            "  {:<8} {:<15} {} → {}  now by {}",
            change.decision,
            change.change.replace('-', " "),
            clean(&change.caller),
            clean(&change.capability),
            change.now_by
        );
        if let Some(context) = result.untrusted.contexts.get(&change.decision) {
            fence.value(&format!("{} context", change.decision), context);
        }
    }
    if !result.cant_tell.is_empty() {
        let _ = writeln!(
            out,
            "can't tell (recording cut): {}",
            result.cant_tell.join(" ")
        );
    }
    if !result.variables_filled.is_empty() || !result.variables_dropped.is_empty() {
        let _ = writeln!(
            out,
            "variables: filled from the binding {}; dropped {}",
            or_none(&result.variables_filled),
            or_none(&result.variables_dropped)
        );
    }
    if result.recording_truncated {
        let _ = writeln!(
            out,
            "the recording lost decisions to its caps, so this list may be incomplete"
        );
    }
    fence.render(&mut out);
    next_lines(&mut out, &result.next);
    out
}

fn or_none(items: &[String]) -> String {
    if items.is_empty() {
        "none".to_owned()
    } else {
        clean(&items.join(", "))
    }
}

fn draft_text(result: &DraftResult) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    let mut fence = Fence::default();
    let _ = writeln!(
        out,
        "draft for {}  {} → {}  (run {} · {} · {})",
        result.decision,
        clean(&result.caller),
        clean(&result.capability),
        result.header.run,
        clean(&result.header.source),
        render::version_text(result.header.blueprint_version.as_deref())
    );
    let _ = writeln!(
        out,
        "page: {}",
        result.header.page.as_deref().unwrap_or(render::NO_PAGE)
    );
    let _ = writeln!(
        out,
        "goes in: {}, lines {}-{}, in the `{}` block",
        result.file_in_project.display(),
        result.lines[0],
        result.lines[1],
        clean(&result.caller)
    );
    match &result.overrides {
        Some(rule) => {
            let _ = writeln!(
                out,
                "overrides: {} for this call only; it goes directly above it",
                render::rule_label(&rule.caller, rule.index, rule.name.as_deref(), rule.line)
            );
        }
        None => {
            let _ = writeln!(out, "overrides: nothing; the default refused the call");
        }
    }
    fence.text("rule", &result.untrusted.rule, 12);
    fence.render(&mut out);
    if result.name.is_none() {
        let _ = writeln!(
            out,
            "unnamed: decisions will cite this rule by its place in the block; draft it with \
             `--name <name>` to give it a name they cite instead"
        );
    }
    if result.written {
        let _ = writeln!(
            out,
            "written: the playground applies it and logs the new blueprint version"
        );
    } else {
        let _ = writeln!(
            out,
            "not written: the file is unchanged. Review the rule; writing it into the file takes \
             an explicit `--write`"
        );
    }
    next_lines(&mut out, &result.next);
    out
}

// ---- the CLI's side ------------------------------------------------------------------------

/// An action as the CLI sends it to the running playground.
pub(crate) enum ActionRequest {
    Exec(ExecRequest),
    /// `None` reads the binding without changing it.
    Bind(Option<BindRequest>),
    SessionStart(SessionStartRequest),
    SessionEnd(SessionEndRequest),
    Recheck(u64),
    Test(TestRequest),
    Rerun(u64),
    Clear,
    Cancel(u64),
}

impl ActionRequest {
    /// The control route and the body it takes.
    fn route(&self) -> (&'static str, &'static str, Option<Value>) {
        let body = |value: Result<Value, serde_json::Error>| value.ok();
        match self {
            Self::Exec(request) => ("POST", "/api/exec", body(serde_json::to_value(request))),
            Self::Bind(None) => ("GET", "/api/binding", None),
            Self::Bind(Some(request)) => ("POST", "/api/bind", body(serde_json::to_value(request))),
            Self::SessionStart(request) => (
                "POST",
                "/api/sessions/start",
                body(serde_json::to_value(request)),
            ),
            Self::SessionEnd(request) => (
                "POST",
                "/api/sessions/end",
                body(serde_json::to_value(request)),
            ),
            Self::Recheck(run) => ("POST", "/api/recheck", Some(json!({ "run": run }))),
            Self::Test(request) => ("POST", "/api/test", body(serde_json::to_value(request))),
            Self::Rerun(run) => ("POST", "/api/rerun", Some(json!({ "run": run }))),
            Self::Clear => ("POST", "/api/clear", Some(json!({}))),
            Self::Cancel(run) => ("POST", "/api/cancel", Some(json!({ "run": run }))),
        }
    }
}

/// The running playground for the project around the current directory, or the answer
/// that says it is not running.
pub(crate) fn connect() -> Result<Result<Running, Answer>> {
    let not_running = || Ok(Err(Answer::from(ActError::not_running())));
    let Ok(cwd) = std::env::current_dir() else {
        return not_running();
    };
    let Some(root) = project::find_project_root(&cwd) else {
        return not_running();
    };
    match client::probe(&StateDir::for_project(&root))? {
        Probe::Running(running) => Ok(Ok(running)),
        Probe::Busy(busy) => Ok(Err(Answer::from(ActError::new(
            EXIT_NOT_RUNNING,
            "not-running",
            busy.message(),
        )))),
        Probe::NotRunning | Probe::Stale(_) => not_running(),
    }
}

/// Sends `request` to the running playground and prints its answer.
pub(crate) fn execute_action(request: &ActionRequest, output: Output) -> Result<ExitCode> {
    let running = match connect()? {
        Ok(running) => running,
        Err(answer) => return Ok(answer.print(output)),
    };
    let (method, path, body) = request.route();
    let answer: Answer = match running.send(method, path, body.as_ref()) {
        Ok(answer) => answer,
        Err(error) => ActError::failure(format!("{error:#}")).into(),
    };
    Ok(answer.print(output))
}

/// `draft-rule`, against the store and the blueprint file directly: it works with the
/// playground stopped. A running playground applies a written rule as any save.
pub(crate) fn execute_draft(
    decision: DecisionRef,
    write: bool,
    name: Option<&str>,
    output: Output,
) -> ExitCode {
    let drafted = (|| {
        let cwd = std::env::current_dir().map_err(|_| ActError::from(ReadError::NoProject))?;
        let project = project::discover(&cwd, None)
            .map_err(|error| ActError::usage("no-project", error.to_string()))?;
        let state = StateDir::for_project(&project.root);
        let store = match Store::open_read_only(&state.store_dir()) {
            Ok(store) => store,
            Err(StoreError::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {
                return Err(ReadError::UnknownRun(decision.run).into());
            }
            Err(error) => return Err(ActError::store(&error)),
        };
        let page = super::running_page(&state);
        draft_rule(
            &store,
            &project.blueprint,
            &project.root,
            decision,
            write,
            name,
            &page,
        )
    })();
    answer(drafted.map(|result| Answer::of(EXIT_SUCCESS, &result, draft_text))).print(output)
}

/// `watch`: follows a session's event feed and prints each event as one JSON line, until
/// the session's last run finishes (with `follow`, until the session ends), the session
/// ends, or the playground stops. Without a session, follows the most recent one.
pub(crate) fn execute_watch(
    session: Option<String>,
    follow: bool,
    output: Output,
) -> Result<ExitCode> {
    let running = match connect()? {
        Ok(running) => running,
        Err(answer) => return Ok(answer.print(output)),
    };
    let session = match session {
        Some(session) => session,
        None => match most_recent_session() {
            Ok(Some(session)) => session,
            Ok(None) => {
                return Ok(Answer::from(
                    ActError::usage(
                        "no-sessions",
                        "no sessions yet; run a program first, or start one with `submilli \
                         playground session start`",
                    )
                    .next([Next::Sessions]),
                )
                .print(output));
            }
            Err(error) => return Ok(Answer::from(error).print(output)),
        },
    };
    let path = format!("/api/sessions/{}/events", url_segment(&session));
    let stdout = std::io::stdout();
    let mut idle = false;
    let mut ended = false;
    let mut decisions = std::collections::HashMap::new();
    let followed = running.follow(&path, None, |event| {
        let line = match event.name.as_str() {
            "run-idle" => {
                idle = true;
                json!({ "kind": "run-idle", "session": session })
            }
            "session-ended" => {
                ended = true;
                json!({ "kind": "session-ended", "session": session })
            }
            _ => watch_line(
                event.id.as_deref().and_then(|id| id.parse::<u64>().ok()),
                &serde_json::from_str::<Value>(&event.data).unwrap_or(Value::Null),
                &session,
                &mut decisions,
            ),
        };
        let mut out = stdout.lock();
        let _ = writeln!(out, "{line}");
        let _ = out.flush();
        !ended && (follow || !idle)
    });
    if let Err(error) = followed {
        return Ok(Answer::from(ActError::failure(format!("{error:#}"))).print(output));
    }
    if ended {
        return Ok(ExitCode::from(EXIT_SUCCESS));
    }
    if follow {
        eprintln!("the playground stopped; the session's feed ended with it");
        return Ok(ExitCode::from(EXIT_SUCCESS));
    }
    if idle {
        if matches!(output, Output::Text) {
            eprintln!(
                "the session is idle, so watch stopped; `submilli playground watch {} --follow` \
                 keeps following its later runs until it ends",
                render::shell_word(&session)
            );
        }
        return Ok(ExitCode::from(EXIT_SUCCESS));
    }
    eprintln!("the playground stopped before the session's runs finished");
    Ok(ExitCode::from(EXIT_NOT_RUNNING))
}

/// Fields of a session event that only say where it sits; the line carries them in its
/// own envelope.
const ENVELOPE_FIELDS: [&str; 6] = [
    "schema",
    "seq",
    "session_id",
    "run_id",
    "event_id",
    "at_micros",
];

/// One stored event as `watch` prints it: trusted fields (sequence, kind, session, run,
/// time, ids, caller and capability names, outcomes, sizes) at the top, and what came from
/// inside the run (a decision's context, its near misses' values and reason, an MCP
/// client's own name) under `untrusted`.
///
/// A decision gets its `<run>.<n>` reference by counting the run's decisions as they
/// arrive, which `watch` sees from the session's first event. A decision recovered after
/// its run lost events under load arrives out of order, so it gets none.
fn watch_line(
    seq: Option<u64>,
    stored: &Value,
    session: &str,
    decisions: &mut std::collections::HashMap<u64, usize>,
) -> Value {
    let mut line = serde_json::Map::new();
    let mut untrusted = serde_json::Map::new();
    line.insert("seq".into(), json!(seq));
    line.insert("session".into(), json!(session));
    line.insert("event_id".into(), stored["event_id"].clone());
    let run = stored["run"].as_u64();
    if let Some(run) = run {
        line.insert("run".into(), json!(run));
    }
    if let Some(gap) = stored["body"]["gap"].as_object() {
        line.insert("kind".into(), json!("gap"));
        if let Some(at) = stored["position"]["at_micros"].as_u64() {
            line.insert("at".into(), json!(render::rfc3339(at)));
        }
        line.extend(gap.clone());
        return Value::Object(line);
    }
    let Some(event) = stored["body"]["event"].as_object() else {
        line.insert("kind".into(), json!("unknown"));
        return Value::Object(line);
    };
    let kind = event.get("kind").cloned().unwrap_or(Value::Null);
    line.insert("kind".into(), kind.clone());
    if let Some(at) = event.get("at_micros").and_then(Value::as_u64) {
        line.insert("at".into(), json!(render::rfc3339(at)));
    }
    let mut fields = event.clone();
    fields.remove("kind");
    for field in ENVELOPE_FIELDS {
        fields.remove(field);
    }
    if let Some(client) = fields.remove("client") {
        untrusted.insert("client".into(), client);
    }
    if kind == "decision"
        && let Some(Value::Object(mut record)) = fields.remove("record")
    {
        for field in ["context", "reason", "near_misses"] {
            if let Some(value) = record.remove(field) {
                untrusted.insert(field.into(), value);
            }
        }
        // The decision's own place in its run, kept apart from the envelope's `seq`,
        // which is the session's sequence and the feed's resume cursor.
        if let Some(seq) = record.remove("seq") {
            record.insert("decision_seq".into(), seq);
        }
        record.remove("at_micros");
        if let Some(run) = run
            && stored["backfilled"].as_bool() != Some(true)
        {
            let n = decisions.entry(run).or_insert(0);
            *n += 1;
            line.insert("decision".into(), json!(format!("{run}.{n}")));
        }
        fields.extend(record);
    }
    // The envelope (sequence, session, run, kind, time) stays as set above: a field of
    // the stored event never replaces it.
    for (field, value) in fields {
        line.entry(field).or_insert(value);
    }
    if !untrusted.is_empty() {
        line.insert("untrusted".into(), Value::Object(untrusted));
    }
    Value::Object(line)
}

fn most_recent_session() -> Result<Option<String>, ActError> {
    let reader = Reader::for_cwd()?;
    Ok(read::sessions(&reader, 1)?
        .sessions
        .into_iter()
        .next()
        .map(|row| row.session))
}

/// `text` as one path segment.
pub(crate) fn url_segment(text: &str) -> String {
    percent_encoding::utf8_percent_encode(text, percent_encoding::NON_ALPHANUMERIC).to_string()
}

#[cfg(test)]
mod tests;
