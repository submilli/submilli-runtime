//! The blueprint watcher (KTD18): every save of the playground's blueprint, from any
//! editor or tool, is checked, logged as a version, and applied, with no restart and
//! no approval step (R9, R37).
//!
//! A save is read whole. The project packages it newly names are built and installed
//! first, through the same serialized check runs get, so a package created in the
//! same edit as the line that lists it is in the store when the save is validated; a
//! package that does not build refuses the save with its diagnostic. The save is then
//! validated as registration would validate it (parse, the filter-field check,
//! volumes, secrets, packages); a refused save is reported, with
//! its line when the parser gave one, and the last good version stays in force. A save
//! that passes is classified against the version in force (KTD10), logged as a new
//! version, and only then applied, so no run can record a version the log lacks. If
//! the apply itself fails, an apply-failed entry voids the version. A save that only
//! changes comments or whitespace (same normalized hash) is not a new version: it
//! appends a bytes-updated entry for the current one.
//!
//! The watcher watches the file's parent directory, not the file: editors that save
//! by writing a new file and renaming it over the old one replace the inode a
//! file watch would follow.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use anyhow::{Context, Result};
use notify_debouncer_mini::notify::RecursiveMode;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use submilli_blueprint::Blueprint;
use submilli_blueprint::diff::{self, BlueprintDiff};
use submilli_server::{AppState, LocalApplyError};

use super::log::{note, warn};
use super::packages::{Freshness, ResolutionFailure};
use super::store::Store;
use super::store::changes::{NewVersion, Version};

/// How long the directory must be quiet before a save is read. Long enough to
/// coalesce an editor's write-then-rename or a format-on-save rewrite into one
/// version, short enough that a run started right after saving sees the edit.
pub(crate) const DEBOUNCE: Duration = Duration::from_millis(250);

/// What the control listener reports about the blueprint: the version in force and
/// the last save that was refused, until a later save applies.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct BlueprintStatus {
    pub(crate) version: Option<u64>,
    pub(crate) refused: Option<Refusal>,
}

/// A save the playground did not apply. Nothing was logged for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Refusal {
    /// Registration's error code (`parse_error`, `invalid_filter`, ...), or
    /// `name_changed` or `unreadable`.
    pub(crate) code: String,
    pub(crate) message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) line: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) column: Option<usize>,
}

impl Refusal {
    fn new(code: &str, message: String) -> Self {
        Self {
            code: code.to_owned(),
            message,
            line: None,
            column: None,
        }
    }

    fn of(error: LocalApplyError) -> Self {
        let first = error.diagnostics.first();
        Self {
            code: error.code.to_owned(),
            line: first.and_then(|d| d.line),
            column: first.and_then(|d| d.col),
            message: error.message,
        }
    }
}

/// What one look at the file did.
#[derive(Debug)]
pub(crate) enum Outcome {
    /// The file holds the text the current version already has.
    Unchanged,
    /// Only comments or whitespace changed: the current version's bytes were replaced.
    BytesUpdated { version: u64 },
    /// A new version was logged and applied. `diff` is `None` for the first version.
    Applied {
        version: u64,
        diff: Option<Box<BlueprintDiff>>,
    },
    /// Refused before anything was logged; the last good version stays.
    Refused(Refusal),
    /// Logged, then the apply failed: the version was voided.
    ApplyFailed { version: u64, reason: String },
}

/// When the file is read: at start, a file that holds the version in force is
/// registered again under its tag; on a later save it is already registered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Moment {
    Start,
    Save,
}

/// The version a save replaces, as the change log holds it.
enum Previous {
    /// The log holds no version: this save is the first.
    None,
    /// The version in force no longer parses, so the change cannot be classified.
    Unreadable,
    Parsed(Box<Blueprint>),
}

impl Previous {
    fn of(current: Option<&Version>) -> Self {
        match current.map(|current| submilli_blueprint::parse(&current.bytes)) {
            None => Self::None,
            Some(Ok(blueprint)) => Self::Parsed(Box::new(blueprint)),
            Some(Err(_)) => Self::Unreadable,
        }
    }

    fn blueprint(&self) -> Option<&Blueprint> {
        match self {
            Self::Parsed(blueprint) => Some(blueprint),
            Self::None | Self::Unreadable => None,
        }
    }
}

/// Applies the playground's blueprint file through the server's trusted local path.
pub(crate) struct Applier {
    state: AppState,
    store: Arc<Store>,
    path: PathBuf,
    /// The name the playground serves; a save that changes it is refused.
    name: String,
    /// Builds the project packages a save names before it is validated; `None` when
    /// nothing builds them.
    packages: Option<Arc<Freshness>>,
    // Poison means a panic interrupted a status update. It is read through anyway, as
    // the store's locks are: the status is only a report, and each update sets whole
    // fields.
    status: Arc<Mutex<BlueprintStatus>>,
    /// One apply at a time: the watcher can fire while `start` is still applying, and
    /// two applies reading the same change log would log the same version twice.
    turn: tokio::sync::Mutex<()>,
}

impl Applier {
    pub(crate) fn new(state: AppState, store: Arc<Store>, path: PathBuf, name: String) -> Self {
        Self {
            state,
            store,
            path,
            name,
            packages: None,
            status: Arc::default(),
            turn: tokio::sync::Mutex::new(()),
        }
    }

    /// Build the project packages each save names, through `packages`, before it is
    /// validated.
    pub(crate) fn with_packages(mut self, packages: Arc<Freshness>) -> Self {
        self.packages = Some(packages);
        self
    }

    pub(crate) fn status(&self) -> Arc<Mutex<BlueprintStatus>> {
        Arc::clone(&self.status)
    }

    fn set_status(&self, update: impl FnOnce(&mut BlueprintStatus)) {
        update(&mut self.status.lock().unwrap_or_else(PoisonError::into_inner));
    }

    /// At start: apply the file as it is now, as a new version when it differs from
    /// the version in force, or under that version's tag when it does not. A refused
    /// file fails the start.
    pub(crate) async fn start(&self) -> Result<()> {
        match self.apply_file_at(Moment::Start).await {
            Outcome::Refused(refusal) => Err(anyhow::anyhow!(
                "{}: {}",
                self.path.display(),
                describe_refusal(&refusal)
            )),
            Outcome::ApplyFailed { reason, .. } => Err(anyhow::anyhow!(
                "the server refused blueprint `{}`: {reason}",
                self.name
            )),
            _ => Ok(()),
        }
    }

    /// Reads the file and acts on what changed; see the module docs.
    pub(crate) async fn apply_file(&self) -> Outcome {
        self.apply_file_at(Moment::Save).await
    }

    async fn apply_file_at(&self, moment: Moment) -> Outcome {
        let _turn = self.turn.lock().await;
        let outcome = self.check_and_apply(moment).await;
        self.report(&outcome);
        outcome
    }

    /// Reads the file, refuses what registration would refuse or what renames the
    /// blueprint, and otherwise logs and applies it as the version it is.
    async fn check_and_apply(&self, moment: Moment) -> Outcome {
        let yaml = match std::fs::read_to_string(&self.path) {
            Ok(yaml) => yaml,
            Err(error) => {
                return self.refuse(Refusal::new(
                    "unreadable",
                    format!("reading {}: {error}", self.path.display()),
                ));
            }
        };
        let changes = match self.store.changes() {
            Ok(changes) => changes,
            Err(error) => {
                return self.refuse(Refusal::new(
                    "change_log_unreadable",
                    format!("reading the change log: {error}"),
                ));
            }
        };
        // No other apply can log a version while this one holds the turn, so the
        // version read here stays the one in force until this save is logged.
        let current = changes.current();
        let previous = Previous::of(current);
        if let Err(failure) = self.prepare_packages(&yaml, previous.blueprint()).await {
            return self.refuse(package_refusal(&failure));
        }
        let blueprint = match self.state.check_local_blueprint(&yaml).await {
            Ok(blueprint) => blueprint,
            Err(error) => return self.refuse(Refusal::of(error)),
        };
        if blueprint.name != self.name {
            return self.refuse(Refusal::new(
                "name_changed",
                format!(
                    "the blueprint's `name:` changed from `{}` to `{}`; the playground serves \
                     `{}` until it restarts, so put the name back or restart it",
                    self.name, blueprint.name, self.name
                ),
            ));
        }
        let hash = normalized_hash(&blueprint);
        if let Some(current) = current.filter(|current| current.hash == hash) {
            return self.same_version(current, yaml, moment).await;
        }
        self.log_and_apply(previous, &blueprint, hash, yaml).await
    }

    /// The file holds the current version, maybe with other comments or layout:
    /// records the new bytes and, at start, registers it again under its tag.
    async fn same_version(&self, current: &Version, yaml: String, moment: Moment) -> Outcome {
        let version = current.version;
        let same_bytes = current.bytes == yaml;
        if !same_bytes && let Err(error) = self.store.append_bytes_updated(version, yaml.clone()) {
            warn(&format!("logging the comment-only edit failed: {error}"));
        }
        if moment == Moment::Start {
            // Runs record the version's tag, which lives with the registration, not
            // on disk: register again under it.
            if let Err(error) = self.state.apply_local_blueprint(&yaml, &tag(version)).await {
                return Outcome::ApplyFailed {
                    version,
                    reason: error.message,
                };
            }
            self.set_status(|status| status.version = Some(version));
        }
        // The file is back to good text, so an earlier refusal no longer stands.
        self.set_status(|status| status.refused = None);
        if same_bytes {
            Outcome::Unchanged
        } else {
            Outcome::BytesUpdated { version }
        }
    }

    /// Classifies `blueprint` against the version in force, logs it as a new version,
    /// and applies it; a failed apply voids the version.
    async fn log_and_apply(
        &self,
        previous: Previous,
        blueprint: &Blueprint,
        hash: String,
        yaml: String,
    ) -> Outcome {
        let (diff, classification, summary) = match previous {
            Previous::Parsed(previous) => {
                let diff = Box::new(diff::diff(&previous, blueprint));
                let classification = serde_json::to_value(diff.as_ref()).unwrap_or(Value::Null);
                let summary = diff.summary();
                (Some(diff), classification, summary)
            }
            Previous::Unreadable => (
                None,
                json!({ "classification": "unknown", "changes": [] }),
                "The version this replaces could not be read back, so the change is not \
                 classified."
                    .to_owned(),
            ),
            Previous::None => (
                None,
                json!({ "classification": "initial", "changes": [] }),
                "The first version the playground served.".to_owned(),
            ),
        };
        let version = match self.store.append_version(NewVersion {
            hash,
            bytes: yaml.clone(),
            classification,
            summary,
        }) {
            Ok(version) => version,
            Err(error) => {
                return self.refuse(Refusal::new(
                    "change_log_unwritable",
                    format!("logging the version failed, so it was not applied: {error}"),
                ));
            }
        };
        match self.state.apply_local_blueprint(&yaml, &tag(version)).await {
            Ok(_) => {
                self.set_status(|status| {
                    status.version = Some(version);
                    status.refused = None;
                });
                Outcome::Applied { version, diff }
            }
            Err(error) => {
                if let Err(log_error) = self
                    .store
                    .append_apply_failed(version, error.message.clone())
                {
                    warn(&format!(
                        "voiding version {version} in the change log failed: {log_error}"
                    ));
                }
                Outcome::ApplyFailed {
                    version,
                    reason: error.message,
                }
            }
        }
    }

    /// Build and install the project packages `yaml` names that `in_force` does not.
    /// Text that does not parse is left for validation to report.
    async fn prepare_packages(
        &self,
        yaml: &str,
        in_force: Option<&Blueprint>,
    ) -> Result<(), ResolutionFailure> {
        let Some(packages) = &self.packages else {
            return Ok(());
        };
        let Ok(blueprint) = submilli_blueprint::parse(yaml) else {
            return Ok(());
        };
        packages
            .prepare_edit(&self.state, blueprint, in_force.cloned())
            .await
    }

    fn refuse(&self, refusal: Refusal) -> Outcome {
        self.set_status(|status| status.refused = Some(refusal.clone()));
        Outcome::Refused(refusal)
    }

    /// The playground's log is its stderr.
    fn report(&self, outcome: &Outcome) {
        let in_force = self
            .status
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .version;
        let stays = in_force.map_or_else(String::new, |v| format!("; version {v} stays in force"));
        match outcome {
            Outcome::Unchanged => {}
            Outcome::BytesUpdated { version } => {
                note(&format!(
                    "blueprint: comments or whitespace changed; still version {version}"
                ));
            }
            Outcome::Applied {
                version,
                diff: None,
            } => {
                note(&format!("blueprint: version {version} applied"));
            }
            Outcome::Applied {
                version,
                diff: Some(diff),
            } => {
                for change in diff.pin_removals() {
                    warn(&format!(
                        "PIN REMOVED in version {version}: {}",
                        change.summary
                    ));
                }
                note(&format!(
                    "blueprint: version {version} applied ({})\n{}",
                    diff.classification,
                    indent(&diff.summary())
                ));
            }
            Outcome::Refused(refusal) => {
                warn(&format!(
                    "blueprint edit refused: {}{stays}",
                    describe_refusal(refusal)
                ));
            }
            Outcome::ApplyFailed { version, reason } => {
                warn(&format!(
                    "blueprint version {version} could not be applied and is void: {reason}{stays}"
                ));
            }
        }
    }
}

/// The opaque tag a version's runs record.
pub(crate) fn tag(version: u64) -> String {
    super::store::changes::version_tag(version)
}

/// The hash that decides whether an edit is a new version: of the blueprint as
/// parsed and written back, so comments and layout do not count.
pub(crate) fn normalized_hash(blueprint: &Blueprint) -> String {
    let digest = Sha256::digest(submilli_blueprint::to_yaml(blueprint).as_bytes());
    super::state::hex(&digest)
}

/// A save refused because a project package it names could not be built or installed.
fn package_refusal(failure: &ResolutionFailure) -> Refusal {
    let (code, message) = match failure {
        ResolutionFailure::Build {
            package,
            diagnostic,
            manifest_dir,
        } => (
            "package_build_failed",
            format!(
                "package `{package}` does not build, so the edit was not applied. Fix the \
                 error and save the blueprint again; `submilli build check` in {} shows it \
                 too.\n{}",
                manifest_dir.display(),
                diagnostic.trim_end()
            ),
        ),
        other => ("package_resolution", other.to_string()),
    };
    Refusal::new(code, message)
}

pub(crate) fn describe_refusal(refusal: &Refusal) -> String {
    match (refusal.line, refusal.column) {
        (Some(line), Some(column)) => format!("line {line}, column {column}: {}", refusal.message),
        (Some(line), None) => format!("line {line}: {}", refusal.message),
        _ => refusal.message.clone(),
    }
}

fn indent(text: &str) -> String {
    text.lines()
        .map(|line| format!("  {line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Watches the blueprint's directory and applies each settled save until dropped.
pub(crate) struct Watch {
    _debouncer: notify_debouncer_mini::Debouncer<notify_debouncer_mini::notify::RecommendedWatcher>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Watch {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// Starts watching `applier`'s file. Must run inside a Tokio runtime.
pub(crate) fn watch(applier: Arc<Applier>, debounce: Duration) -> Result<Watch> {
    let dir = applier
        .path
        .parent()
        .map(Path::to_path_buf)
        .context("the blueprint file has no parent directory")?;
    let file_name = applier
        .path
        .file_name()
        .map(ToOwned::to_owned)
        .context("the blueprint path names no file")?;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<()>();
    let mut debouncer = notify_debouncer_mini::new_debouncer(
        debounce,
        move |events: notify_debouncer_mini::DebounceEventResult| {
            let touched = match events {
                Ok(events) => events
                    .iter()
                    .any(|event| event.path.file_name() == Some(file_name.as_os_str())),
                // A backend error may have dropped events; look at the file again.
                Err(_) => true,
            };
            if touched {
                let _ = tx.send(());
            }
        },
    )
    .context("starting the blueprint watcher")?;
    debouncer
        .watcher()
        .watch(&dir, RecursiveMode::NonRecursive)
        .with_context(|| format!("watching {}", dir.display()))?;
    let task = tokio::spawn(async move {
        while rx.recv().await.is_some() {
            // Saves that settled while this one was applied are one look, not many.
            while rx.try_recv().is_ok() {}
            applier.apply_file().await;
        }
    });
    Ok(Watch {
        _debouncer: debouncer,
        task,
    })
}

#[cfg(test)]
mod tests;
