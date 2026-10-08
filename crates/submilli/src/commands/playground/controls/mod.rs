//! The playground's controls: one function per control, taking typed inputs and
//! returning a typed result, behind the CLI, the control API, and the page.
//!
//! The reads ([`read`]) open the store read-only and never reach the running
//! playground, so they work with it stopped; a running one only adds page links,
//! found from its instance record without a request. The actions ([`act`]) run in the
//! running playground, which the CLI reaches through the control API. [`render`] holds
//! the output contract every control shares.

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Result;
use serde::Serialize;
use serde_json::json;

use super::Output;
use super::packages::{ClosureEntry, ProjectPackages};
use super::project;
use super::state::StateDir;
use super::store::run::DecisionRef;
use super::store::{Store, StoreError};

pub(crate) mod act;
pub(crate) mod read;
pub(crate) mod render;

#[cfg(test)]
pub(crate) mod fixtures;
#[cfg(test)]
mod tests;

use render::{EXIT_SUCCESS, EXIT_USAGE, Next, Page, next};

/// What the read controls read from.
pub(crate) struct Reader {
    /// `None` before the playground has stored anything.
    pub(crate) store: Option<Store>,
    pub(crate) page: Page,
    /// Where `audit` finds the package closure: the project around this directory,
    /// its blueprint, and the package store.
    project_dir: Option<PathBuf>,
    /// A closure given outright, in place of the project's.
    fixed_closure: Option<Vec<ClosureEntry>>,
}

/// Why a read control could not answer.
#[derive(Debug)]
pub(crate) enum ReadError {
    NoProject,
    UnknownRun(u64),
    UnknownDecision { decision: DecisionRef, count: usize },
    UnknownVersion { version: u64, voided: bool },
    Store(StoreError),
}

impl From<StoreError> for ReadError {
    fn from(error: StoreError) -> Self {
        Self::Store(error)
    }
}

impl std::fmt::Display for ReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoProject => {
                f.write_str("no Submilli project here; create one with `submilli playground init`")
            }
            Self::UnknownRun(run) => write!(
                f,
                "no run {run} is stored; list the runs with `submilli playground runs`"
            ),
            Self::UnknownDecision { decision, count } => write!(
                f,
                "run {} has {count} decision{}, so there is no {decision}; see them with \
                 `submilli playground show {}`",
                decision.run,
                if *count == 1 { "" } else { "s" },
                decision.run
            ),
            Self::UnknownVersion { version, voided } if *voided => write!(
                f,
                "blueprint version {version} is void: its apply failed and it was never in \
                 force; list the versions with `submilli playground changes`"
            ),
            Self::UnknownVersion { version, .. } => write!(
                f,
                "no blueprint version {version} is logged; list the versions with \
                 `submilli playground changes`"
            ),
            Self::Store(error) => write!(f, "reading the run store failed: {error}"),
        }
    }
}

impl ReadError {
    pub(crate) fn kind(&self) -> &'static str {
        match self {
            Self::NoProject => "no-project",
            Self::UnknownRun(_) => "unknown-run",
            Self::UnknownDecision { .. } => "unknown-decision",
            Self::UnknownVersion { .. } => "unknown-version",
            Self::Store(_) => "store",
        }
    }

    pub(crate) fn exit(&self) -> u8 {
        match self {
            Self::Store(_) => 1,
            _ => EXIT_USAGE,
        }
    }

    pub(crate) fn next(&self) -> Vec<String> {
        match self {
            Self::UnknownRun(_) => next([Next::Runs]),
            Self::UnknownDecision { decision, .. } => next([Next::Show(decision.run)]),
            Self::UnknownVersion { .. } => next([Next::Changes]),
            Self::NoProject | Self::Store(_) => Vec::new(),
        }
    }
}

impl Reader {
    /// The reader for the project around the current directory.
    fn for_cwd() -> Result<Self, ReadError> {
        let cwd = std::env::current_dir().map_err(|_| ReadError::NoProject)?;
        let root = project::find_project_root(&cwd).ok_or(ReadError::NoProject)?;
        let state = StateDir::for_project(&root);
        let store = match Store::open_read_only(&state.store_dir()) {
            Ok(store) => Some(store),
            Err(StoreError::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {
                None
            }
            Err(error) => return Err(error.into()),
        };
        Ok(Self {
            store,
            page: running_page(&state),
            project_dir: Some(cwd),
            fixed_closure: None,
        })
    }

    /// The reader the running playground answers the page's reads with: its own store,
    /// opened read-only like any reader's, and its page.
    pub(crate) fn for_host(
        store_root: &std::path::Path,
        page: Page,
        project_root: &std::path::Path,
    ) -> Result<Self, ReadError> {
        Ok(Self {
            store: Some(Store::open_read_only(store_root)?),
            page,
            project_dir: Some(project_root.to_path_buf()),
            fixed_closure: None,
        })
    }

    #[cfg(test)]
    pub(crate) fn for_tests(store: Store, page: Page, closure: Vec<ClosureEntry>) -> Self {
        Self {
            store: Some(store),
            page,
            project_dir: None,
            fixed_closure: Some(closure),
        }
    }

    /// The blueprint's package closure as the package store holds it now: from the
    /// version in force (the change log's text), or the project's blueprint file
    /// before any version is logged. Reads only; builds nothing.
    pub(crate) fn closure(&self) -> Result<Vec<ClosureEntry>, String> {
        if let Some(closure) = &self.fixed_closure {
            return Ok(closure.clone());
        }
        let cwd = self.project_dir.as_ref().ok_or("no project to read")?;
        let project = project::discover(cwd, None).map_err(|error| error.to_string())?;
        let current = self
            .store
            .as_ref()
            .and_then(|store| store.changes().ok())
            .and_then(|changes| changes.current().map(|version| version.bytes.clone()));
        let text = match current {
            Some(text) => text,
            None => std::fs::read_to_string(&project.blueprint)
                .map_err(|error| format!("{}: {error}", project.blueprint.display()))?,
        };
        let blueprint = submilli_blueprint::parse(&text)
            .map_err(|error| format!("the blueprint does not parse: {error}"))?;
        ProjectPackages::new(
            &project.package_dir,
            submilli_build::default_package_store_dir(),
        )
        .closure(&blueprint)
        .map_err(|failure| failure.to_string())
    }
}

/// The running playground's page, from its instance record and lock, without a
/// request: a record with no process holding the instance lock is a stopped one.
fn running_page(state: &StateDir) -> Page {
    let record = state.read_record().ok().flatten();
    let holder = state.instance_holder().ok().flatten();
    match (record, holder) {
        (Some(record), Some(holder)) if !holder.stopping => Page {
            base: Some(format!("http://127.0.0.1:{}/", record.control_port)),
        },
        _ => Page::default(),
    }
}

/// A read control, as the CLI asks for it.
pub(crate) enum ReadRequest {
    Runs(read::RunsQuery),
    Show { run: u64, include_payloads: bool },
    Explain(DecisionRef),
    Compare(u64, u64),
    Audit(read::AuditQuery),
    Changes(Option<u64>),
    Sessions { limit: usize },
}

/// Runs a read control against the project around the current directory and prints
/// its result.
pub(crate) fn execute_read(request: ReadRequest, output: Output) -> Result<ExitCode> {
    let reader = match Reader::for_cwd() {
        Ok(reader) => reader,
        Err(error) => return Ok(report_error(&error, output)),
    };
    let printed = match request {
        ReadRequest::Runs(query) => {
            read::runs(&reader, &query).map(|result| emit(&result, render::runs_text, output))
        }
        ReadRequest::Show {
            run,
            include_payloads,
        } => read::show(&reader, run, include_payloads)
            .map(|result| emit(&result, render::show_text, output)),
        ReadRequest::Explain(decision) => read::explain(&reader, decision)
            .map(|result| emit(&result, render::explain_text, output)),
        ReadRequest::Compare(a, b) => {
            read::compare(&reader, a, b).map(|result| emit(&result, render::compare_text, output))
        }
        ReadRequest::Audit(query) => {
            read::audit(&reader, query).map(|result| emit(&result, render::audit_text, output))
        }
        ReadRequest::Changes(version) => read::changes(&reader, version)
            .map(|result| emit(&result, render::changes_text, output)),
        ReadRequest::Sessions { limit } => read::sessions(&reader, limit)
            .map(|result| emit(&result, render::sessions_text, output)),
    };
    Ok(match printed {
        Ok(()) => ExitCode::from(EXIT_SUCCESS),
        Err(error) => report_error(&error, output),
    })
}

fn emit<T: Serialize>(result: &T, text: fn(&T) -> String, output: Output) {
    match output {
        Output::Json => match serde_json::to_string(result) {
            Ok(json) => println!("{json}"),
            Err(error) => eprintln!("encoding the result failed: {error}"),
        },
        Output::Text => print!("{}", text(result)),
    }
}

fn report_error(error: &ReadError, output: Output) -> ExitCode {
    if let Output::Json = output {
        println!(
            "{}",
            json!({
                "error": { "kind": error.kind(), "message": error.to_string() },
                "next": error.next(),
            })
        );
    }
    eprintln!("{error}");
    ExitCode::from(error.exit())
}
