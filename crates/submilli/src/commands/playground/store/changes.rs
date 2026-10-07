//! The change log: an append-only record of blueprint versions and clears.
//!
//! The blueprint watcher logs each change as a version before applying it, appends a
//! bytes-updated entry for an edit that changes only comments or whitespace, and voids
//! a version whose apply failed. `clear` appends a clear marker. Readers take the
//! latest bytes per version and skip voided versions. The audit window is the runs
//! decided under the latest version that still stands, or, when a clear came after it,
//! the runs after that clear.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::run::RunSummary;
use super::{FORMAT, Result, Store, append_line, json_line, now_micros, read_lines};

/// One line of the change log.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct ChangeLine {
    pub(crate) format: u32,
    pub(crate) at_micros: u64,
    #[serde(flatten)]
    pub(crate) entry: ChangeEntry,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "kind")]
pub(crate) enum ChangeEntry {
    /// A blueprint version, logged before it is applied.
    Version {
        /// Numbered from 1 in order, never reused; returning to earlier text is a new
        /// version.
        version: u64,
        /// The last run id handed out when the version was logged: runs after it ran
        /// under this version or a later one.
        after_run: u64,
        /// The normalized hash that decides whether an edit is a new version.
        hash: String,
        /// The blueprint file's text.
        bytes: String,
        /// How the change widened or narrowed access, as the watcher classified it.
        classification: Value,
        /// The change in plain language.
        summary: String,
    },
    /// The version's file changed in comments or whitespace only; these bytes replace
    /// its earlier ones for readers.
    BytesUpdated { version: u64, bytes: String },
    /// Applying the version failed after it was logged: readers ignore it.
    ApplyFailed { version: u64, reason: String },
    /// `clear` removed every run up to `high_water`, the last id handed out then.
    Clear { high_water: u64 },
}

/// A version as readers see it: its latest bytes.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Version {
    pub(crate) version: u64,
    pub(crate) after_run: u64,
    pub(crate) at_micros: u64,
    pub(crate) hash: String,
    pub(crate) bytes: String,
    pub(crate) classification: Value,
    pub(crate) summary: String,
}

/// Where the audit window starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WindowStart {
    /// No version or clear yet: every run.
    Beginning,
    Version(u64),
    Clear,
}

/// The change log as readers see it.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Changes {
    /// Versions that stand, in order, each with its latest bytes.
    pub(crate) versions: Vec<Version>,
    /// Versions whose apply failed.
    pub(crate) voided: Vec<u64>,
    /// Every line, in order, for a reader that wants the history itself.
    pub(crate) lines: Vec<ChangeLine>,
    /// Lines that did not parse (a write cut short), left out of `lines`.
    pub(crate) skipped: usize,
}

impl Changes {
    fn from_lines(lines: Vec<ChangeLine>, skipped: usize) -> Self {
        let voided: Vec<u64> = lines
            .iter()
            .filter_map(|line| match &line.entry {
                ChangeEntry::ApplyFailed { version, .. } => Some(*version),
                _ => None,
            })
            .collect();
        let mut versions: Vec<Version> = Vec::new();
        for line in &lines {
            match &line.entry {
                ChangeEntry::Version {
                    version,
                    after_run,
                    hash,
                    bytes,
                    classification,
                    summary,
                } if !voided.contains(version) => versions.push(Version {
                    version: *version,
                    after_run: *after_run,
                    at_micros: line.at_micros,
                    hash: hash.clone(),
                    bytes: bytes.clone(),
                    classification: classification.clone(),
                    summary: summary.clone(),
                }),
                ChangeEntry::BytesUpdated { version, bytes } => {
                    if let Some(entry) = versions.iter_mut().find(|v| v.version == *version) {
                        entry.bytes.clone_from(bytes);
                    }
                }
                _ => {}
            }
        }
        Self {
            versions,
            voided,
            lines,
            skipped,
        }
    }

    /// The version that stands now, if any.
    pub(crate) fn current(&self) -> Option<&Version> {
        self.versions.last()
    }

    /// The highest version number logged, voided ones included, so none is reused.
    pub(crate) fn last_version_number(&self) -> u64 {
        self.lines
            .iter()
            .filter_map(|line| match &line.entry {
                ChangeEntry::Version { version, .. } => Some(*version),
                _ => None,
            })
            .max()
            .unwrap_or(0)
    }

    /// The latest clear's high-water mark: run ids up to it are never handed out again.
    pub(crate) fn high_water(&self) -> u64 {
        self.lines
            .iter()
            .filter_map(|line| match &line.entry {
                ChangeEntry::Clear { high_water } => Some(*high_water),
                _ => None,
            })
            .max()
            .unwrap_or(0)
    }

    /// What started the audit window, and an id every run in it is above. The latest
    /// standing version or clear starts it, so a comment-only edit (a bytes-updated
    /// entry) and a failed apply leave it where it was. For a clear the id is its
    /// high-water mark, and the window is every run above it. For a version it is the
    /// last id handed out when the version was logged, a bound only: which runs are in
    /// the window is [`Changes::in_audit_window`]'s answer.
    pub(crate) fn audit_window(&self) -> (u64, WindowStart) {
        let mut window = (0, WindowStart::Beginning);
        for line in &self.lines {
            match &line.entry {
                ChangeEntry::Version {
                    version, after_run, ..
                } if !self.voided.contains(version) => {
                    window = (*after_run, WindowStart::Version(*version));
                }
                ChangeEntry::Clear { high_water } => window = (*high_water, WindowStart::Clear),
                _ => {}
            }
        }
        window
    }

    /// Whether `run` is in the audit window. When a version starts the window, that is a
    /// run decided under it, as the run itself records: a run that started after the
    /// version was logged but before it was applied ran under the earlier one and is not
    /// in it. Otherwise it is every run after the clear that started the window, or every
    /// run when nothing has.
    pub(crate) fn in_audit_window(&self, run: &RunSummary) -> bool {
        match self.audit_window() {
            (_, WindowStart::Beginning) => true,
            (high_water, WindowStart::Clear) => run.id > high_water,
            (_, WindowStart::Version(version)) => {
                run.blueprint_version.as_deref() == Some(version_tag(version).as_str())
            }
        }
    }
}

/// The tag a run records for the version it was decided under
/// (`RunStart::blueprint_version`).
pub(crate) fn version_tag(version: u64) -> String {
    version.to_string()
}

/// What the watcher logs for a new version; the store numbers it.
pub(crate) struct NewVersion {
    pub(crate) hash: String,
    pub(crate) bytes: String,
    pub(crate) classification: Value,
    pub(crate) summary: String,
}

impl Store {
    /// The change log as readers see it. A line that does not parse (a write cut short)
    /// is skipped and counted, so it never blocks the watcher; a line in a newer format
    /// is refused.
    pub(crate) fn changes(&self) -> Result<Changes> {
        read_lines(&self.changes_path())
            .map(|lines| Changes::from_lines(lines.records, lines.skipped))
    }

    /// The stored runs in the audit window ([`Changes::in_audit_window`]), by id.
    pub(crate) fn audit_window_runs(&self) -> Result<Vec<RunSummary>> {
        let changes = self.changes()?;
        let mut runs = self.list_runs()?;
        runs.retain(|run| changes.in_audit_window(run));
        Ok(runs)
    }

    /// Logs a new version and returns its number.
    pub(crate) fn append_version(&self, new: NewVersion) -> Result<u64> {
        self.writer()?;
        // The run-id lock first, as `clear` takes them, so the two never deadlock.
        let inner = self.lock();
        let _changes = self
            .changes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let version = self.changes()?.last_version_number().saturating_add(1);
        self.append_line_unlocked(ChangeEntry::Version {
            version,
            after_run: inner.last_run,
            hash: new.hash,
            bytes: new.bytes,
            classification: new.classification,
            summary: new.summary,
        })?;
        Ok(version)
    }

    /// Replaces a version's bytes for readers, after a comment- or whitespace-only edit.
    pub(crate) fn append_bytes_updated(&self, version: u64, bytes: String) -> Result<()> {
        self.append_change(ChangeEntry::BytesUpdated { version, bytes })
    }

    /// Voids a version whose apply failed.
    pub(crate) fn append_apply_failed(&self, version: u64, reason: String) -> Result<()> {
        self.append_change(ChangeEntry::ApplyFailed { version, reason })
    }

    pub(super) fn append_change(&self, entry: ChangeEntry) -> Result<()> {
        self.writer()?;
        let _changes = self
            .changes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.append_line_unlocked(entry)
    }

    fn append_line_unlocked(&self, entry: ChangeEntry) -> Result<()> {
        let line = ChangeLine {
            format: FORMAT,
            at_micros: now_micros(),
            entry,
        };
        append_line(&self.changes_path(), &json_line(&line))
    }
}
