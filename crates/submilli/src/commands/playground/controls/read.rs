//! The read controls: `runs`, `show`, `explain`, `compare`, `audit`, `changes`, and
//! `sessions`. Each reads the store, never the running playground, so each works with
//! the playground stopped; the page link is the only thing a running one adds.

use std::collections::{BTreeMap, BTreeSet};

use interpreter::runtime::{DecisionAction, DecisionCause, DecisionRecord, FailureReasonRecord};
use serde::Serialize;
use serde_json::Value;
use submilli_blueprint::{Blueprint, Citation};
use submilli_server::error::ErrorKind;

use crate::commands::playground::packages::{ClosureEntry, Origin};
use crate::commands::playground::store::changes::{Changes, Version, WindowStart, version_tag};
use crate::commands::playground::store::run::{DecisionRef, RunSummary, StoredRun};
use crate::commands::playground::store::sessions::SessionEntry;

use super::render::{Next, RunRef, next};
use super::{ReadError, Reader};

/// The fixed note on runs the developer's app sent.
pub(crate) const APP_NOTE: &str =
    "Calls the developer's agent makes outside Submilli are not visible here.";

/// What an empty store says.
pub(crate) const EMPTY_STORE: &str = "No runs yet. Start the playground with `submilli \
     playground`, then run a program through it (the page's example, the stand-in, or your \
     app with the app token).";

/// The longest result or console text a read shows without `--include-payloads`.
const MAX_TEXT_BYTES: usize = 4000;

/// A caller and a capability.
type PairKey = (String, String);
/// An audit line: caller, capability, verdict, and what decided.
type AuditKey = (String, String, Verdict, String);

// ---- shared pieces -----------------------------------------------------------------------

/// How a run ended.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "kebab-case", tag = "kind")]
pub(crate) enum Outcome {
    Completed,
    /// A policy denial the program did not catch ended it.
    Denied {
        #[serde(skip_serializing_if = "Option::is_none")]
        decision: Option<DecisionRef>,
    },
    /// A test run stopped at a call the recording could not answer.
    Stopped,
    /// Someone cancelled it while it ran.
    Cancelled,
    Failed {
        error: ErrorKind,
    },
    /// The program never reached the runner.
    NotDispatched {
        error: Option<ErrorKind>,
    },
}

impl Outcome {
    /// How a run with `error` ended. With `cancel_is_test_stop`, a cancel is the test
    /// run stopping at a call it would have had to make live.
    fn of(
        error: Option<ErrorKind>,
        dispatched: bool,
        cancel_is_test_stop: bool,
        denial: Option<DecisionRef>,
    ) -> Self {
        match error {
            None => Self::Completed,
            Some(_) if !dispatched => Self::NotDispatched { error },
            Some(ErrorKind::PermissionDenied) => Self::Denied { decision: denial },
            Some(ErrorKind::Cancelled) if cancel_is_test_stop => Self::Stopped,
            Some(ErrorKind::Cancelled) => Self::Cancelled,
            Some(kind) => Self::Failed { error: kind },
        }
    }

    pub(crate) fn text(&self) -> String {
        match self {
            Self::Completed => "completed".to_owned(),
            Self::Denied { decision: Some(d) } => format!("ended by an uncaught denial ({d})"),
            Self::Denied { decision: None } => "ended by an uncaught denial".to_owned(),
            Self::Stopped => "stopped".to_owned(),
            Self::Cancelled => "cancelled".to_owned(),
            Self::Failed { error } => failed_text(*error),
            Self::NotDispatched { error } => format!(
                "did not start{}",
                error.map_or_else(String::new, |error| format!(": {}", error_words(error)))
            ),
        }
    }

    /// [`Self::text`], with the denials a completed run's program caught, by ref:
    /// `completed, 1 denial caught (2.3)`.
    pub(crate) fn text_with_denials(&self, denied: &[DecisionRef]) -> String {
        match (self, denied) {
            (Self::Completed, [_, ..]) => format!(
                "completed, {} caught ({})",
                super::render::plural(denied.len(), "denial"),
                super::render::refs_text_with(denied, ", ")
            ),
            _ => self.text(),
        }
    }
}

/// A failed run's outcome in words.
fn failed_text(kind: ErrorKind) -> String {
    match kind {
        ErrorKind::CompileError => "failed to compile".to_owned(),
        ErrorKind::RuntimeError => "failed with a runtime error".to_owned(),
        other => format!("failed: {}", error_words(other)),
    }
}

/// An error kind in words.
fn error_words(kind: ErrorKind) -> &'static str {
    match kind {
        ErrorKind::CompileError => "it did not compile",
        ErrorKind::Timeout => "it ran out of time",
        ErrorKind::FuelExhausted => "it ran out of fuel (its CPU budget)",
        ErrorKind::MemoryExhausted => "it ran out of memory",
        ErrorKind::StackExhausted => "it ran out of stack (recursion too deep)",
        ErrorKind::Cancelled => "it was cancelled",
        ErrorKind::PermissionDenied => "a call was denied",
        ErrorKind::RuntimeError => "a runtime error",
        ErrorKind::BlueprintNotFound => "its blueprint was not found",
        ErrorKind::PackageResolution => "a package it imports could not be built or found",
        ErrorKind::InvalidRequest => "its variables or secrets do not fit the blueprint",
    }
}

/// A rule of the blueprint, and where it is in the text of the version the run was
/// decided under.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct RuleOut {
    pub(crate) caller: String,
    /// Zero-based, as the blueprint's rule list counts.
    pub(crate) index: usize,
    /// One-based, as the text cites it: ``rule 2 of `main` ``.
    pub(crate) position: usize,
    pub(crate) name: Option<String>,
    /// 1-based; `None` when the text did not locate it (anchors, flow style, or no text
    /// for the version), and the caller block and position name it instead.
    pub(crate) line: Option<usize>,
    pub(crate) column: Option<usize>,
    pub(crate) end_line: Option<usize>,
}

impl RuleOut {
    /// In full: ``​`name` (rule 1 of `main`, line 17)``, or ``rule 2 of `main`, line 20``.
    pub(crate) fn text(&self) -> String {
        super::render::rule_label(&self.caller, self.index, self.name.as_deref(), self.line)
    }

    /// Its name, or where it is.
    fn short(&self) -> String {
        match &self.name {
            Some(name) => format!("`{}`", super::render::clean(name)),
            None => super::render::rule_place(&self.caller, self.index, self.line),
        }
    }

    /// Its name and place, without the line: for lists of rules.
    fn listed(&self) -> String {
        super::render::rule_label(&self.caller, self.index, self.name.as_deref(), None)
    }
}

/// The blueprint a run was decided under: the latest text of its version.
struct DecidedUnder<'a> {
    version: Option<&'a Version>,
    blueprint: Option<Blueprint>,
}

impl<'a> DecidedUnder<'a> {
    fn find(changes: &'a Changes, tag: Option<&str>) -> Self {
        let version = tag.and_then(|tag| {
            changes
                .versions
                .iter()
                .find(|version| version_tag(version.version) == tag)
        });
        let blueprint = version.and_then(|version| submilli_blueprint::parse(&version.bytes).ok());
        Self { version, blueprint }
    }

    fn rule(&self, caller: &str, index: usize, name: Option<&str>) -> RuleOut {
        let citation = match self.version {
            Some(version) => cite(&version.bytes, caller, index),
            None => Citation::Index {
                caller: caller.to_owned(),
                index,
            },
        };
        let name = name.map(str::to_owned).or_else(|| {
            self.blueprint
                .as_ref()
                .and_then(|blueprint| blueprint.permissions.get(caller))
                .and_then(|rules| rules.get(index))
                .and_then(|rule| rule.name.clone())
        });
        let (line, column, end_line) = match citation {
            Citation::Line(location) => (
                Some(location.line),
                Some(location.column),
                Some(location.end_line),
            ),
            Citation::Index { .. } => (None, None, None),
        };
        RuleOut {
            caller: caller.to_owned(),
            index,
            position: index.saturating_add(1),
            name,
            line,
            column,
            end_line,
        }
    }

    fn filter(&self, caller: &str, index: usize) -> Option<String> {
        self.blueprint
            .as_ref()?
            .permissions
            .get(caller)?
            .get(index)?
            .filter
            .as_ref()
            .map(ToString::to_string)
    }

    fn default_action(&self) -> Option<&'static str> {
        let blueprint = self.blueprint.as_ref()?;
        Some(match blueprint.default_action.unwrap_or_default() {
            submilli_blueprint::DefaultAction::Deny => "deny",
            submilli_blueprint::DefaultAction::Allow => "allow",
            submilli_blueprint::DefaultAction::AskHuman => "ask-human",
        })
    }
}

/// Where a rule is in `text`. The one place the locator is called.
fn cite(text: &str, caller: &str, index: usize) -> Citation {
    submilli_blueprint::locate_rule(text, caller, index)
}

/// What a decision came to: allowed, held for a human, or denied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Verdict {
    Allow,
    Ask,
    Deny,
}

impl Verdict {
    fn of(record: &DecisionRecord) -> Self {
        match (record.allowed, record.action) {
            (true, _) => Self::Allow,
            (false, DecisionAction::AskHuman) => Self::Ask,
            (false, DecisionAction::Allow | DecisionAction::Deny) => Self::Deny,
        }
    }

    /// `allow`, `ask`, or `deny`, as text lists show it.
    pub(crate) fn word(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Ask => "ask",
            Self::Deny => "deny",
        }
    }

    /// What the audit says was done with the call.
    fn done(self) -> &'static str {
        match self {
            Self::Allow => "allowed",
            Self::Ask => "held for a human",
            Self::Deny => "denied",
        }
    }
}

fn is_denial(record: &DecisionRecord) -> bool {
    !record.allowed
}

/// The run's denials as `<run>.<n>`, leaving out those a host function swallowed to
/// filter a listing: the denials the program saw, whether it caught them or not.
fn denied_refs(run: &StoredRun) -> Vec<DecisionRef> {
    run.recording
        .decisions
        .iter()
        .enumerate()
        .filter(|(_, record)| is_denial(record) && !record.filtered)
        .map(|(position, _)| reference(run.id, position))
        .collect()
}

/// The rules that nearly allowed a refused call: those that named its capability but
/// whose filters rejected it.
fn near_miss_rules(record: &DecisionRecord, under: &DecidedUnder<'_>) -> Vec<RuleOut> {
    if !is_denial(record) {
        return Vec::new();
    }
    record
        .near_misses
        .iter()
        .map(|miss| {
            under.rule(
                &miss.rule.caller,
                miss.rule.index,
                miss.rule.name.as_deref(),
            )
        })
        .collect()
}

/// A short phrase for what decided: "by `reads`", "by the default (near misses: ...)".
fn decided_by_phrase(record: &DecisionRecord, under: &DecidedUnder<'_>) -> String {
    let mut phrase = match &record.cause {
        DecisionCause::Rule(rule) => format!(
            "by {}",
            under
                .rule(&rule.caller, rule.index, rule.name.as_deref())
                .short()
        ),
        DecisionCause::Default { .. } => "by the default".to_owned(),
        DecisionCause::RuntimeInvariant { .. } => "by the runtime, ahead of the policy".to_owned(),
        DecisionCause::Unexplained => "by the policy (unexplained)".to_owned(),
    };
    let misses: Vec<String> = near_miss_rules(record, under)
        .iter()
        .map(RuleOut::listed)
        .collect();
    match misses.as_slice() {
        [] => {}
        [only] => phrase.push_str(&format!("; near miss: {only}")),
        _ => phrase.push_str(&format!("; near misses: {}", misses.join(", "))),
    }
    if record.filtered {
        phrase.push_str(" · filtered from a listing");
    }
    phrase
}

/// The cause as an identity, for comparing which rule decided.
fn cause_key(record: &DecisionRecord) -> String {
    match &record.cause {
        DecisionCause::Rule(rule) => match &rule.name {
            Some(name) => format!("rule:{}:{name}", rule.caller),
            None => format!("rule:{}:#{}", rule.caller, rule.index),
        },
        DecisionCause::Default { .. } => "default".to_owned(),
        DecisionCause::RuntimeInvariant { .. } => "runtime".to_owned(),
        DecisionCause::Unexplained => "unexplained".to_owned(),
    }
}

/// The decision at zero-based `position` in run `run`.
fn reference(run: u64, position: usize) -> DecisionRef {
    DecisionRef {
        run,
        n: position.saturating_add(1),
    }
}

fn cap_text(text: &str, include: bool) -> String {
    if include || text.len() <= MAX_TEXT_BYTES {
        return text.to_owned();
    }
    let mut end = MAX_TEXT_BYTES;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    format!(
        "{}…[{} of {} bytes shown; --include-payloads shows all]",
        text.get(..end).unwrap_or_default(),
        end,
        text.len()
    )
}

impl Reader {
    fn header(&self, run: &StoredRun, decision_refs: Vec<DecisionRef>) -> RunRef {
        RunRef {
            run: run.id,
            page: self.page.run(run.id),
            blueprint_version: run.recording.blueprint_version.clone(),
            source: run.label.clone(),
            decision_refs,
        }
    }

    fn summary_header(&self, summary: &RunSummary, decision_refs: Vec<DecisionRef>) -> RunRef {
        RunRef {
            run: summary.id,
            page: self.page.run(summary.id),
            blueprint_version: summary.blueprint_version.clone(),
            source: summary.label.clone(),
            decision_refs,
        }
    }

    fn summaries(&self) -> Result<Vec<RunSummary>, ReadError> {
        match &self.store {
            Some(store) => Ok(store.list_runs()?),
            None => Ok(Vec::new()),
        }
    }

    fn changes(&self) -> Result<Changes, ReadError> {
        match &self.store {
            Some(store) => Ok(store.changes()?),
            None => Ok(Changes::default()),
        }
    }

    /// Each rerun's source run, by the rerun's id.
    fn reruns(&self) -> Result<BTreeMap<u64, u64>, ReadError> {
        match &self.store {
            Some(store) => Ok(store.reruns()?),
            None => Ok(BTreeMap::new()),
        }
    }

    fn load(&self, id: u64) -> Result<StoredRun, ReadError> {
        let store = self.store.as_ref().ok_or(ReadError::UnknownRun(id))?;
        store.load_run(id)?.ok_or(ReadError::UnknownRun(id))
    }
}

// ---- runs ----------------------------------------------------------------------------------

/// Which runs `runs` lists.
#[derive(Debug, Clone, Default)]
pub(crate) struct RunsQuery {
    pub(crate) source: Option<String>,
    pub(crate) since: Option<Since>,
    pub(crate) session: Option<String>,
    pub(crate) limit: usize,
    /// Now, in microseconds since the Unix epoch, for `--since <duration>`.
    pub(crate) now_micros: u64,
}

impl RunsQuery {
    /// Whether a run with these traits is one the query lists.
    fn matches(
        &self,
        id: u64,
        label: &str,
        session: Option<&String>,
        started_at_micros: u64,
    ) -> bool {
        self.source.as_ref().is_none_or(|source| label == source)
            && self
                .session
                .as_ref()
                .is_none_or(|wanted| session == Some(wanted))
            && match self.since {
                None => true,
                Some(Since::Run(after)) => id > after,
                Some(Since::Micros(window)) => {
                    started_at_micros >= self.now_micros.saturating_sub(window)
                }
            }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Since {
    /// Runs that started within this many microseconds of now.
    Micros(u64),
    /// Runs after this one.
    Run(u64),
}

impl std::str::FromStr for Since {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        if let Ok(run) = text.parse::<u64>() {
            return Ok(Self::Run(run));
        }
        let invalid =
            || format!("`{text}` is neither a run id nor a duration like `30s`, `10m`, `2h`, `1d`");
        let split = text.len().checked_sub(1).ok_or_else(invalid)?;
        let (number, unit) = (text.get(..split), text.get(split..));
        let number: u64 = number.and_then(|n| n.parse().ok()).ok_or_else(invalid)?;
        let seconds: u64 = match unit {
            Some("s") => 1,
            Some("m") => 60,
            Some("h") => 3600,
            Some("d") => 86_400,
            _ => return Err(invalid()),
        };
        Ok(Self::Micros(
            number.saturating_mul(seconds).saturating_mul(1_000_000),
        ))
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct RunsResult {
    pub(crate) kind: &'static str,
    /// Runs still in flight, newest first: listed only while the playground runs.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) running: Vec<RunningRow>,
    pub(crate) runs: Vec<RunRow>,
    /// Matching runs left out by the limit.
    pub(crate) more: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) note: Option<&'static str>,
    /// Runs listed without their denials, because their files could not be read.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) warnings: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) empty_message: Option<&'static str>,
    pub(crate) next: Vec<String>,
}

/// A run in flight.
#[derive(Debug, Serialize)]
pub(crate) struct RunningRow {
    pub(crate) run: u64,
    pub(crate) page: Option<String>,
    pub(crate) source: String,
    pub(crate) started_at_micros: u64,
    pub(crate) started_at: String,
    pub(crate) session: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct RunRow {
    /// `decision_refs` holds the run's denials.
    #[serde(flatten)]
    pub(crate) header: RunRef,
    pub(crate) started_at_micros: u64,
    pub(crate) started_at: String,
    pub(crate) outcome: Outcome,
    pub(crate) decisions: usize,
    /// The run's denials, by ref, as every run-bearing result lists them.
    pub(crate) denied: Vec<DecisionRef>,
    pub(crate) session: Option<String>,
    pub(crate) test_of: Option<u64>,
    /// For a rerun, the run whose program it ran again.
    pub(crate) rerun_of: Option<u64>,
}

pub(crate) fn runs(reader: &Reader, query: &RunsQuery) -> Result<RunsResult, ReadError> {
    let mut summaries = reader.summaries()?;
    let store_empty = summaries.is_empty();
    summaries.retain(|summary| {
        query.matches(
            summary.id,
            &summary.label,
            summary.session_id.as_ref(),
            summary.started_at_micros,
        )
    });
    summaries.sort_by_key(|summary| std::cmp::Reverse(summary.id));
    let limit = query.limit.max(1);
    let more = summaries.len().saturating_sub(limit);
    summaries.truncate(limit);
    let reruns = reader.reruns()?;
    let mut rows = Vec::with_capacity(summaries.len());
    let mut warnings = Vec::new();
    for summary in &summaries {
        // Only a run with denials is read in full, for its denials' refs.
        let denials = if summary.denied > 0 {
            match reader.load(summary.id) {
                Ok(run) => denied_refs(&run),
                // Cleared since the listing was read.
                Err(ReadError::UnknownRun(_)) => continue,
                Err(error) => {
                    warnings.push(format!(
                        "run {} is listed without its denials: {error}",
                        summary.id
                    ));
                    Vec::new()
                }
            }
        } else {
            Vec::new()
        };
        let last_denial = denials.last().copied();
        rows.push(RunRow {
            header: reader.summary_header(summary, denials.clone()),
            started_at_micros: summary.started_at_micros,
            started_at: super::render::rfc3339(summary.started_at_micros),
            outcome: Outcome::of(
                summary.error,
                summary.dispatched,
                summary.entry == "test",
                last_denial,
            ),
            decisions: summary.decisions,
            denied: denials,
            session: summary.session_id.clone(),
            test_of: summary.test_of.as_ref().and_then(|link| link.run),
            rerun_of: reruns.get(&summary.id).copied(),
        });
    }
    let running = running_rows(reader, query, &summaries)?;
    let mut suggestions: Vec<Next> = running.iter().map(|row| Next::Cancel(row.run)).collect();
    suggestions.extend(
        rows.iter()
            .filter_map(|row| row.header.decision_refs.first().copied())
            .map(Next::Explain),
    );
    if let Some(row) = rows.first() {
        suggestions.insert(running.len().min(2), Next::Show(row.header.run));
    }
    if rows.len() > 1 && query.session.is_none() {
        suggestions.push(Next::Sessions);
    }
    suggestions.truncate(4);
    Ok(RunsResult {
        kind: "runs",
        note: (query.source.as_deref() == Some("app")
            || rows.iter().any(|row| row.header.source == "app"))
        .then_some(APP_NOTE),
        empty_message: (rows.is_empty() && running.is_empty()).then_some(if store_empty {
            EMPTY_STORE
        } else {
            "No runs match."
        }),
        running,
        runs: rows,
        more,
        warnings,
        next: next(suggestions),
    })
}

/// The runs in flight that `query` matches, newest first: only while the playground that
/// noted them runs, and never one already stored.
fn running_rows(
    reader: &Reader,
    query: &RunsQuery,
    stored: &[RunSummary],
) -> Result<Vec<RunningRow>, ReadError> {
    let Some(store) = reader.store.as_ref().filter(|_| reader.page.base.is_some()) else {
        return Ok(Vec::new());
    };
    let mut rows: Vec<RunningRow> = store
        .running()?
        .into_iter()
        .filter(|run| {
            stored.iter().all(|summary| summary.id != run.id)
                && query.matches(
                    run.id,
                    &run.label,
                    run.session_id.as_ref(),
                    run.started_at_micros,
                )
        })
        .map(|run| RunningRow {
            run: run.id,
            page: reader.page.run(run.id),
            source: run.label,
            started_at: super::render::rfc3339(run.started_at_micros),
            started_at_micros: run.started_at_micros,
            session: run.session_id,
        })
        .collect();
    rows.sort_by_key(|row| std::cmp::Reverse(row.run));
    Ok(rows)
}

// ---- show ----------------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub(crate) struct ShowResult {
    pub(crate) kind: &'static str,
    #[serde(flatten)]
    pub(crate) header: RunRef,
    pub(crate) blueprint: String,
    pub(crate) entry: String,
    pub(crate) started_at_micros: u64,
    pub(crate) started_at: String,
    pub(crate) wall_ms: u64,
    pub(crate) outcome: Outcome,
    /// The denials the program saw, by ref: with a `completed` outcome, the ones it
    /// caught; with a denied one, the last is the one that ended it.
    pub(crate) denied: Vec<DecisionRef>,
    pub(crate) session: Option<String>,
    pub(crate) variables: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) note: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) test: Option<TestInfo>,
    /// For a rerun, the run whose program it ran again.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) rerun_of: Option<u64>,
    pub(crate) decision_count: usize,
    /// Identical allowed decisions collapsed into one line; denials one per line.
    pub(crate) decisions: Vec<DecisionLine>,
    pub(crate) decisions_dropped: u64,
    pub(crate) log_truncated: bool,
    pub(crate) calls: CallSummary,
    pub(crate) payloads_included: bool,
    pub(crate) next: Vec<String>,
    pub(crate) untrusted: ShowUntrusted,
}

#[derive(Debug, Serialize)]
pub(crate) struct DecisionLine {
    /// Every decision the line stands for.
    pub(crate) refs: Vec<DecisionRef>,
    pub(crate) outcome: Verdict,
    pub(crate) caller: String,
    pub(crate) capability: String,
    pub(crate) decided_by: String,
    /// For a refusal, the rules that nearly allowed it, as `decided_by` lists them.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) near_misses: Vec<RuleOut>,
    pub(crate) filtered: bool,
}

#[derive(Debug, Default, Serialize)]
pub(crate) struct CallSummary {
    pub(crate) count: usize,
    pub(crate) returned: usize,
    pub(crate) failed: usize,
    pub(crate) unfinished: usize,
    /// Calls whose request or response kept a body copy.
    pub(crate) with_bodies: usize,
}

impl CallSummary {
    pub(crate) fn text(&self) -> String {
        if self.count == 0 {
            return "none".to_owned();
        }
        let mut parts = Vec::new();
        for (count, word) in [
            (self.returned, "returned"),
            (self.failed, "failed"),
            (self.unfinished, "unfinished"),
        ] {
            if count > 0 {
                parts.push(format!("{count} {word}"));
            }
        }
        format!("{} ({})", self.count, parts.join(", "))
    }
}

/// What a test run keeps of its test.
#[derive(Debug, Serialize)]
pub(crate) struct TestInfo {
    pub(crate) source_run: Option<u64>,
    pub(crate) status: String,
    /// `recorded`, `reads-live`, or `live`, when the report is stored.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) mode: Option<String>,
    /// Variables the test run bound that the tested run did not.
    pub(crate) variables_filled: Vec<String>,
    /// Variables the tested run bound that the test run did not.
    pub(crate) variables_dropped: Vec<String>,
    pub(crate) local_state: String,
    /// Calls answered from the recording; their keys are in `untrusted.test`.
    pub(crate) served: Vec<ServedOut>,
    /// The call the run stopped at; its key and detail are in `untrusted.test`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) stopped: Option<StopOut>,
    /// Calls with nothing recorded that went live; their keys are in `untrusted.test`.
    pub(crate) went_live: Vec<LiveOut>,
    /// After a stop: that running on live takes an explicit opt-in.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) live_note: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) not_stored: Option<&'static str>,
}

#[derive(Debug, Serialize)]
pub(crate) struct ServedOut {
    pub(crate) source_call: u64,
    pub(crate) capability: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct StopOut {
    /// Why the recording could not answer it (`no-recording`, `request-differs`, ...).
    pub(crate) reason: String,
    pub(crate) caller: Option<String>,
    pub(crate) capability: Option<String>,
    pub(crate) line: Option<u32>,
    pub(crate) test_call: Option<u64>,
    /// The recorded call nearest to it.
    pub(crate) nearest: Option<ServedOut>,
}

#[derive(Debug, Serialize)]
pub(crate) struct LiveOut {
    pub(crate) reason: String,
    pub(crate) test_call: Option<u64>,
}

/// What came from inside a test run's calls: their keys (URLs, tool names) and why the
/// run stopped.
#[derive(Debug, Default, Serialize)]
pub(crate) struct TestUntrusted {
    /// Each served call's key, in `served` order.
    pub(crate) served_keys: Vec<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) stop_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) stop_detail: Option<String>,
    /// Each live call's key, in `went_live` order.
    pub(crate) went_live_keys: Vec<String>,
}

/// Everything in a shown run that came from inside it.
#[derive(Debug, Default, Serialize)]
pub(crate) struct ShowUntrusted {
    /// Each decision line's context, by the line's first ref.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) contexts: BTreeMap<DecisionRef, Value>,
    /// Why the runtime refused a call, by ref.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) reasons: BTreeMap<DecisionRef, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) error: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) diagnostics: Vec<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) result: Option<String>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub(crate) console: String,
    /// Each call's request and response, with bodies: only with `--include-payloads`.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) calls: BTreeMap<u64, Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) test: Option<TestUntrusted>,
}

/// What a test run says about its local state: true of every test run.
const TEST_LOCAL_STATE: &str = "local files and session data came from today, not from the \
     recorded run";
const TEST_NOT_STORED: &str = "the test report (calls served from the recording, where and \
     why it stopped) is not stored with the run";

pub(crate) fn show(
    reader: &Reader,
    id: u64,
    include_payloads: bool,
) -> Result<ShowResult, ReadError> {
    show_stored(reader, &reader.load(id)?, include_payloads)
}

/// [`show`] of a run already loaded.
pub(crate) fn show_stored(
    reader: &Reader,
    run: &StoredRun,
    include_payloads: bool,
) -> Result<ShowResult, ReadError> {
    let changes = reader.changes()?;
    let under = DecidedUnder::find(&changes, run.recording.blueprint_version.as_deref());
    let mut untrusted = ShowUntrusted::default();
    let lines = decision_lines(run, &under, &mut untrusted);
    let calls = call_summary(run, include_payloads, &mut untrusted);

    let is_test = run.entry == "test" || run.test_of.is_some();
    // A test run whose stored report names no stop was cancelled, not stopped.
    let stopped_by_test = is_test
        && run
            .test_report
            .as_ref()
            .is_none_or(|report| report.stopped.is_some());
    // A cancelled run's error is the runtime's own note that it was cancelled, which the
    // outcome already says; a test run's stop keeps its message, which says where.
    if let Some(error) = run
        .error
        .as_ref()
        .filter(|error| error.kind != ErrorKind::Cancelled || stopped_by_test)
    {
        untrusted.error = Some(cap_text(&error.message, include_payloads));
        untrusted.diagnostics.clone_from(&error.diagnostics);
    }
    untrusted.result = run
        .result
        .as_deref()
        .map(|result| cap_text(result, include_payloads));
    untrusted.console = cap_text(&run.console, include_payloads);

    let denied = denied_refs(run);
    let outcome = Outcome::of(
        run.error.as_ref().map(|error| error.kind),
        run.dispatched,
        stopped_by_test,
        denied.last().copied(),
    );
    let test = if is_test {
        let (info, test_untrusted) = test_info(reader, run, &outcome);
        untrusted.test = test_untrusted;
        Some(info)
    } else {
        None
    };
    let rerun_of = reader.reruns()?.get(&run.id).copied();
    let next = show_next(run, &denied, &lines, test.as_ref(), rerun_of);
    let refs = (1..=run.recording.decisions.len())
        .map(|n| DecisionRef { run: run.id, n })
        .collect();
    Ok(ShowResult {
        kind: "run",
        header: reader.header(run, refs),
        blueprint: run.recording.blueprint_name.clone(),
        entry: run.entry.clone(),
        started_at_micros: run.started_at_micros,
        started_at: super::render::rfc3339(run.started_at_micros),
        wall_ms: run.wall_ms,
        outcome,
        denied,
        session: run.recording.session_id.clone(),
        variables: run.recording.variables.clone(),
        note: (run.label == "app").then_some(APP_NOTE),
        test,
        rerun_of,
        decision_count: run.recording.decisions.len(),
        decisions: lines,
        decisions_dropped: run.decisions_dropped,
        log_truncated: run.recording.log_truncated,
        calls,
        payloads_included: include_payloads,
        next,
        untrusted,
    })
}

/// The run's decisions as `show` lists them, with each line's context and any runtime
/// reason in `untrusted`. Identical allowed decisions (same caller, capability, cause,
/// and context) share a line; a run is decided under one version, so these are within
/// one version. Denials never share one.
fn decision_lines(
    run: &StoredRun,
    under: &DecidedUnder<'_>,
    untrusted: &mut ShowUntrusted,
) -> Vec<DecisionLine> {
    let mut lines: Vec<DecisionLine> = Vec::new();
    let mut groups: BTreeMap<(String, String, String, String), usize> = BTreeMap::new();
    for (position, record) in run.recording.decisions.iter().enumerate() {
        let decision_ref = reference(run.id, position);
        if record.allowed {
            let key = (
                record.caller.clone(),
                record.capability.clone(),
                cause_key(record),
                serde_json::to_string(&record.context).unwrap_or_default(),
            );
            if let Some(line) = groups.get(&key).and_then(|index| lines.get_mut(*index)) {
                line.refs.push(decision_ref);
                continue;
            }
            groups.insert(key, lines.len());
        }
        if !record.context.is_null() {
            untrusted
                .contexts
                .insert(decision_ref, record.context.clone());
        }
        if let DecisionCause::RuntimeInvariant { reason } = &record.cause {
            untrusted.reasons.insert(decision_ref, reason.clone());
        }
        lines.push(DecisionLine {
            refs: vec![decision_ref],
            outcome: Verdict::of(record),
            caller: record.caller.clone(),
            capability: record.capability.clone(),
            decided_by: decided_by_phrase(record, under),
            near_misses: near_miss_rules(record, under),
            filtered: record.filtered,
        });
    }
    lines
}

/// The run's calls counted by how they ended, with each call's request and response in
/// `untrusted` when `include_payloads` asks for them.
fn call_summary(
    run: &StoredRun,
    include_payloads: bool,
    untrusted: &mut ShowUntrusted,
) -> CallSummary {
    let mut calls = CallSummary::default();
    for call in &run.recording.calls {
        calls.count = calls.count.saturating_add(1);
        let count = match call.outcome {
            Some(interpreter::runtime::CallOutcome::Returned) => &mut calls.returned,
            Some(interpreter::runtime::CallOutcome::Failed) => &mut calls.failed,
            Some(interpreter::runtime::CallOutcome::Unfinished) | None => &mut calls.unfinished,
        };
        *count = count.saturating_add(1);
        let has_body = [&call.request, &call.response]
            .iter()
            .any(|side| side.as_ref().is_some_and(|payload| payload.body.is_some()));
        if has_body {
            calls.with_bodies = calls.with_bodies.saturating_add(1);
        }
        if include_payloads {
            untrusted.calls.insert(
                call.call_index,
                serde_json::json!({
                    "capability": call.capability,
                    "request": call.request,
                    "response": call.response,
                }),
            );
        }
    }
    calls
}

/// What `show` suggests next: explaining and drafting from the first of the denials it
/// lists (`denied`), comparing a test or rerun with its source, and the other runs of its
/// session.
fn show_next(
    run: &StoredRun,
    denied: &[DecisionRef],
    lines: &[DecisionLine],
    test: Option<&TestInfo>,
    rerun_of: Option<u64>,
) -> Vec<String> {
    let mut suggestions = Vec::new();
    for denial in denied.iter().take(2) {
        suggestions.push(Next::Explain(*denial));
        suggestions.push(Next::DraftRule(*denial));
    }
    if let Some(source) = test.and_then(|test| test.source_run) {
        suggestions.push(Next::Compare(source, run.id));
    }
    if let Some(source) = rerun_of {
        suggestions.push(Next::Compare(source, run.id));
    }
    if suggestions.is_empty()
        && let Some(first) = lines.first().and_then(|line| line.refs.first())
    {
        suggestions.push(Next::Explain(*first));
    }
    if let Some(session) = &run.recording.session_id {
        suggestions.push(Next::RunsInSession(session.clone()));
    }
    next(suggestions)
}

fn test_info(
    reader: &Reader,
    run: &StoredRun,
    outcome: &Outcome,
) -> (TestInfo, Option<TestUntrusted>) {
    let source_run = run.test_of.as_ref().and_then(|link| link.run);
    let status = match outcome {
        Outcome::Stopped => {
            "stopped at a call with nothing recorded (what it stopped at is in run-data)".to_owned()
        }
        other => other.text(),
    };
    let Some(report) = &run.test_report else {
        let source = source_run.and_then(|id| reader.load(id).ok());
        let (filled, dropped) = match &source {
            Some(source) => {
                let before: BTreeSet<&String> = source.recording.variables.keys().collect();
                let after: BTreeSet<&String> = run.recording.variables.keys().collect();
                (
                    after
                        .difference(&before)
                        .map(|name| (*name).clone())
                        .collect(),
                    before
                        .difference(&after)
                        .map(|name| (*name).clone())
                        .collect(),
                )
            }
            None => (Vec::new(), Vec::new()),
        };
        let info = TestInfo {
            source_run,
            status,
            mode: None,
            variables_filled: filled,
            variables_dropped: dropped,
            local_state: TEST_LOCAL_STATE.to_owned(),
            served: Vec::new(),
            stopped: None,
            went_live: Vec::new(),
            live_note: None,
            not_stored: Some(TEST_NOT_STORED),
        };
        return (info, None);
    };
    let mut untrusted = TestUntrusted::default();
    let served = report
        .served
        .iter()
        .map(|call| {
            untrusted.served_keys.push(call.key.clone());
            ServedOut {
                source_call: call.source_call_index,
                capability: call.capability.clone(),
            }
        })
        .collect();
    let went_live = report
        .went_live
        .iter()
        .map(|call| {
            untrusted.went_live_keys.push(call.key.clone());
            LiveOut {
                reason: call.reason.clone(),
                test_call: call.test_call_index,
            }
        })
        .collect();
    let stopped = report.stopped.as_ref().map(|stop| {
        untrusted.stop_key = Some(stop.key.clone());
        untrusted.stop_detail = Some(stop.detail.clone());
        StopOut {
            reason: stop.reason.clone(),
            caller: stop.caller.clone(),
            capability: stop.capability.clone(),
            line: stop.line.map(|line| line.line),
            test_call: stop.test_call_index,
            nearest: stop.nearest.as_ref().map(|nearest| ServedOut {
                source_call: nearest.call_index,
                capability: nearest.capability.clone(),
            }),
        }
    });
    let live_note = match (&stopped, source_run) {
        (Some(_), Some(source)) => Some(format!(
            "nothing was recorded for this call, so the test stopped before making it; it \
             continues only live, and live execution takes an explicit opt-in: `submilli \
             playground test {source} --reads-live` lets unrecorded reads through, `--live` \
             every call"
        )),
        (Some(_), None) => Some(
            "nothing was recorded for this call, so the test stopped before making it; it \
             continues only live, which takes an explicit opt-in"
                .to_owned(),
        ),
        (None, _) => None,
    };
    let local = &report.local_state;
    let mut local_state = TEST_LOCAL_STATE.to_owned();
    if !local.session_found {
        local_state.push_str(" (the recorded session was gone, so it started empty)");
    }
    if !local.volumes_copied.is_empty() {
        local_state.push_str(&format!(
            "; writable volumes copied: {}",
            local.volumes_copied.join(", ")
        ));
    }
    let info = TestInfo {
        source_run,
        status,
        mode: Some(report.mode.clone()),
        variables_filled: report.variables.filled.keys().cloned().collect(),
        variables_dropped: report.variables.dropped.clone(),
        local_state,
        served,
        stopped,
        went_live,
        live_note,
        not_stored: None,
    };
    (info, Some(untrusted))
}

// ---- explain -------------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub(crate) struct ExplainResult {
    pub(crate) kind: &'static str,
    /// `page` here links the decision itself.
    #[serde(flatten)]
    pub(crate) header: RunRef,
    pub(crate) decision: DecisionRef,
    pub(crate) outcome: Verdict,
    pub(crate) caller: String,
    pub(crate) capability: String,
    pub(crate) decided_by: DecidedBy,
    pub(crate) near_misses: Vec<NearMissOut>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) version_note: Option<&'static str>,
    pub(crate) next: Vec<String>,
    pub(crate) untrusted: ExplainUntrusted,
}

/// What kind of thing decided a call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum DecidedKind {
    Rule,
    Default,
    /// The runtime, ahead of the policy.
    Runtime,
    /// The policy, which did not say how.
    Unexplained,
}

#[derive(Debug, Serialize)]
pub(crate) struct DecidedBy {
    pub(crate) kind: DecidedKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) rule: Option<RuleOut>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) filter: Option<String>,
    /// The blueprint's `default:`, when the default decided.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) default_action: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) caller_block: Option<bool>,
}

impl DecidedBy {
    pub(crate) fn is_default(&self) -> bool {
        self.kind == DecidedKind::Default
    }

    pub(crate) fn text(&self) -> String {
        match (self.kind, &self.rule) {
            (DecidedKind::Rule, Some(rule)) => rule.text(),
            (DecidedKind::Default, _) => {
                let default = self
                    .default_action
                    .map_or_else(String::new, |action| format!("default: {action}; "));
                let block = if self.caller_block == Some(true) {
                    "no rule of the caller's block matched"
                } else {
                    "the caller has no rules"
                };
                format!("the default ({default}{block})")
            }
            (DecidedKind::Runtime, _) => {
                "the runtime, ahead of the policy (its reason is in run-data)".to_owned()
            }
            (DecidedKind::Rule | DecidedKind::Unexplained, _) => {
                "the policy, which did not explain it".to_owned()
            }
        }
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct NearMissOut {
    pub(crate) rule: RuleOut,
    pub(crate) filter: String,
    /// Each comparison that failed; its values are in `untrusted.near_misses`, same
    /// positions.
    pub(crate) failures: Vec<FailureOut>,
}

#[derive(Debug, Serialize)]
pub(crate) struct FailureOut {
    /// The comparison as the blueprint writes it.
    pub(crate) comparison: String,
    pub(crate) reason: String,
    pub(crate) negated: bool,
}

#[derive(Debug, Serialize)]
pub(crate) struct ExplainUntrusted {
    pub(crate) context: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) reason: Option<String>,
    /// Per near miss, per failed comparison: the call's value and the value the rule
    /// needed.
    pub(crate) near_misses: Vec<Vec<FailureValues>>,
}

#[derive(Debug, Serialize)]
pub(crate) struct FailureValues {
    pub(crate) actual: Value,
    pub(crate) expected: Value,
}

const NO_VERSION_TEXT: &str = "the change log holds no text for this run's blueprint \
     version, so rules are cited by caller block and position";

pub(crate) fn explain(reader: &Reader, decision: DecisionRef) -> Result<ExplainResult, ReadError> {
    let run = reader.load(decision.run)?;
    let record = run.decision(decision.n).ok_or(ReadError::UnknownDecision {
        decision,
        count: run.recording.decisions.len(),
    })?;
    let changes = reader.changes()?;
    let under = DecidedUnder::find(&changes, run.recording.blueprint_version.as_deref());

    let (decided_by, reason) = match &record.cause {
        DecisionCause::Rule(rule) => (
            DecidedBy {
                kind: DecidedKind::Rule,
                rule: Some(under.rule(&rule.caller, rule.index, rule.name.as_deref())),
                filter: under.filter(&rule.caller, rule.index),
                default_action: None,
                caller_block: None,
            },
            None,
        ),
        DecisionCause::Default { caller_block } => (
            DecidedBy {
                kind: DecidedKind::Default,
                rule: None,
                filter: None,
                default_action: under.default_action(),
                caller_block: Some(*caller_block),
            },
            None,
        ),
        DecisionCause::RuntimeInvariant { reason } => (
            DecidedBy {
                kind: DecidedKind::Runtime,
                rule: None,
                filter: None,
                default_action: None,
                caller_block: None,
            },
            Some(reason.clone()),
        ),
        DecisionCause::Unexplained => (
            DecidedBy {
                kind: DecidedKind::Unexplained,
                rule: None,
                filter: None,
                default_action: None,
                caller_block: None,
            },
            record.reason.clone(),
        ),
    };

    let mut near_misses = Vec::new();
    let mut values = Vec::new();
    for miss in &record.near_misses {
        let mut failures = Vec::new();
        let mut miss_values = Vec::new();
        for failure in &miss.failures {
            failures.push(FailureOut {
                comparison: failure.comparison.clone(),
                reason: match &failure.reason {
                    FailureReasonRecord::FieldMissing => "the call has no such field".to_owned(),
                    FailureReasonRecord::VariableNotBound(variable) => {
                        format!("variable `{variable}` is not bound")
                    }
                    FailureReasonRecord::NotSatisfied if failure.negated => {
                        "it held, under `not`".to_owned()
                    }
                    FailureReasonRecord::NotSatisfied => "the values differ".to_owned(),
                },
                negated: failure.negated,
            });
            miss_values.push(FailureValues {
                actual: failure.actual.clone().unwrap_or(Value::Null),
                expected: failure.expected.clone().map_or(Value::Null, Value::String),
            });
        }
        near_misses.push(NearMissOut {
            rule: under.rule(
                &miss.rule.caller,
                miss.rule.index,
                miss.rule.name.as_deref(),
            ),
            filter: miss.filter.clone(),
            failures,
        });
        values.push(miss_values);
    }

    let mut suggestions = vec![Next::Show(run.id)];
    if is_denial(record) {
        suggestions.push(Next::DraftRule(decision));
    }
    let mut header = reader.header(&run, vec![decision]);
    header.page = reader.page.decision(decision);
    Ok(ExplainResult {
        kind: "decision",
        header,
        decision,
        outcome: Verdict::of(record),
        caller: record.caller.clone(),
        capability: record.capability.clone(),
        decided_by,
        near_misses,
        version_note: under.version.is_none().then_some(NO_VERSION_TEXT),
        next: next(suggestions),
        untrusted: ExplainUntrusted {
            context: record.context.clone(),
            reason,
            near_misses: values,
        },
    })
}

// ---- compare -------------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub(crate) struct CompareResult {
    pub(crate) kind: &'static str,
    /// The earlier run; `decision_refs` are its side of each change.
    pub(crate) before: RunRef,
    /// The later run.
    pub(crate) after: RunRef,
    pub(crate) changes: Vec<Flip>,
    /// Decisions with no counterpart in the other run.
    pub(crate) unmatched: Vec<String>,
    pub(crate) alignment: &'static str,
    pub(crate) next: Vec<String>,
    pub(crate) untrusted: CompareUntrusted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum FlipKind {
    NewlyAllowed,
    NewlyDenied,
    DifferentRule,
}

impl FlipKind {
    pub(crate) fn text(self) -> &'static str {
        match self {
            Self::NewlyAllowed => "newly allowed",
            Self::NewlyDenied => "newly denied",
            Self::DifferentRule => "different rule",
        }
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct Flip {
    pub(crate) kind: FlipKind,
    pub(crate) before: DecisionRef,
    pub(crate) after: DecisionRef,
    pub(crate) caller: String,
    pub(crate) capability: String,
    /// What decided in the later run.
    pub(crate) decided_by: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) rule: Option<RuleOut>,
    /// For a refusal in the later run, the rules that nearly allowed it.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) near_misses: Vec<RuleOut>,
    /// What decided in the earlier run, for a different-rule change.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) was: Option<String>,
}

#[derive(Debug, Default, Serialize)]
pub(crate) struct CompareUntrusted {
    /// Each changed decision's context in the later run, by its ref there.
    pub(crate) contexts: BTreeMap<DecisionRef, Value>,
}

/// How `compare` pairs decisions, as it reports it.
pub(crate) const ALIGNMENT: &str = "the nth decision of each caller and capability in one run \
     with the nth of the same caller and capability in the other";

pub(crate) fn compare(reader: &Reader, a: u64, b: u64) -> Result<CompareResult, ReadError> {
    let (first, second) = if a <= b { (a, b) } else { (b, a) };
    let before = reader.load(first)?;
    let after = reader.load(second)?;
    let changes = reader.changes()?;
    let before_under = DecidedUnder::find(&changes, before.recording.blueprint_version.as_deref());
    let after_under = DecidedUnder::find(&changes, after.recording.blueprint_version.as_deref());

    // A program run twice makes the same calls in the same order per caller and
    // capability, so the nth of each pair lines up with the nth in the other run.
    let positions = |run: &StoredRun| -> BTreeMap<PairKey, Vec<usize>> {
        let mut map: BTreeMap<PairKey, Vec<usize>> = BTreeMap::new();
        for (position, record) in run.recording.decisions.iter().enumerate() {
            map.entry((record.caller.clone(), record.capability.clone()))
                .or_default()
                .push(position);
        }
        map
    };
    let before_positions = positions(&before);
    let after_positions = positions(&after);

    let mut flips = Vec::new();
    let mut untrusted = CompareUntrusted::default();
    let mut before_refs = Vec::new();
    let mut after_refs = Vec::new();
    for (key, after_list) in &after_positions {
        let before_list = before_positions.get(key).map_or(&[][..], Vec::as_slice);
        for (nth, after_position) in after_list.iter().enumerate() {
            let Some(before_position) = before_list.get(nth) else {
                continue;
            };
            let (Some(was), Some(now)) = (
                before.recording.decisions.get(*before_position),
                after.recording.decisions.get(*after_position),
            ) else {
                continue;
            };
            let kind = match (was.allowed, now.allowed) {
                (false, true) => FlipKind::NewlyAllowed,
                (true, false) => FlipKind::NewlyDenied,
                _ if cause_key(was) != cause_key(now) => FlipKind::DifferentRule,
                _ => continue,
            };
            let before_ref = reference(before.id, *before_position);
            let after_ref = reference(after.id, *after_position);
            if !now.context.is_null() {
                untrusted.contexts.insert(after_ref, now.context.clone());
            }
            let rule = match &now.cause {
                DecisionCause::Rule(rule) => {
                    Some(after_under.rule(&rule.caller, rule.index, rule.name.as_deref()))
                }
                _ => None,
            };
            before_refs.push(before_ref);
            after_refs.push(after_ref);
            flips.push(Flip {
                kind,
                before: before_ref,
                after: after_ref,
                caller: now.caller.clone(),
                capability: now.capability.clone(),
                decided_by: decided_by_phrase(now, &after_under),
                near_misses: near_miss_rules(now, &after_under),
                rule,
                was: (kind == FlipKind::DifferentRule)
                    .then(|| decided_by_phrase(was, &before_under)),
            });
        }
    }
    let mut unmatched = surplus(&after_positions, &before_positions, after.id, before.id);
    unmatched.extend(surplus(
        &before_positions,
        &after_positions,
        before.id,
        after.id,
    ));
    flips.sort_by_key(|flip| flip.after);

    let mut suggestions: Vec<Next> = flips
        .iter()
        .take(3)
        .map(|flip| Next::Explain(flip.after))
        .collect();
    suggestions.push(Next::Show(after.id));
    Ok(CompareResult {
        kind: "compare",
        before: reader.header(&before, before_refs),
        after: reader.header(&after, after_refs),
        changes: flips,
        unmatched,
        alignment: ALIGNMENT,
        next: next(suggestions),
        untrusted,
    })
}

/// For each caller and capability that run `run` decided more often than run `other`,
/// how many more, in words.
fn surplus(
    positions: &BTreeMap<PairKey, Vec<usize>>,
    other_positions: &BTreeMap<PairKey, Vec<usize>>,
    run: u64,
    other: u64,
) -> Vec<String> {
    positions
        .iter()
        .filter_map(|(key, list)| {
            let more = list
                .len()
                .saturating_sub(other_positions.get(key).map_or(0, Vec::len));
            let (caller, capability) = key;
            (more > 0).then(|| {
                format!(
                    "{more} more {caller} → {capability} decisions in run {run} than in run \
                     {other}"
                )
            })
        })
        .collect()
}

// ---- audit ---------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct AuditQuery {
    pub(crate) default_only: bool,
    pub(crate) packages_only: bool,
}

#[derive(Debug, Serialize)]
pub(crate) struct AuditResult {
    pub(crate) kind: &'static str,
    /// What the audit window is, in words.
    pub(crate) window: String,
    /// `decision_refs` are each run's decisions the audit lists.
    pub(crate) runs: Vec<RunRef>,
    pub(crate) filters: Vec<&'static str>,
    pub(crate) groups: Vec<AuditGroup>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) packages_note: Option<String>,
    pub(crate) next: Vec<String>,
    pub(crate) untrusted: AuditUntrusted,
}

#[derive(Debug, Serialize)]
pub(crate) struct AuditGroup {
    pub(crate) caller: String,
    pub(crate) capability: String,
    pub(crate) outcome: Verdict,
    pub(crate) decided_by: String,
    pub(crate) refs: Vec<DecisionRef>,
    /// For a package caller: how it came to be in the closure.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) origin: Option<String>,
}

#[derive(Debug, Default, Serialize)]
pub(crate) struct AuditUntrusted {
    /// Each listed decision's context, by ref.
    pub(crate) contexts: BTreeMap<DecisionRef, Value>,
}

pub(crate) fn audit(reader: &Reader, query: AuditQuery) -> Result<AuditResult, ReadError> {
    let changes = reader.changes()?;
    let window = match changes.audit_window().1 {
        WindowStart::Beginning => "every run (no blueprint version logged yet)".to_owned(),
        WindowStart::Clear => "the runs since the last clear".to_owned(),
        WindowStart::Version(version) => {
            format!("the runs decided under blueprint v{version}, the latest edit")
        }
    };
    let summaries = match &reader.store {
        Some(store) => store.audit_window_runs()?,
        None => Vec::new(),
    };
    let mut filters = Vec::new();
    if query.default_only {
        filters.push("calls allowed only by the default (no rule matched them)");
    }
    if query.packages_only {
        filters.push("calls made by packages");
    }

    let mut groups: Vec<AuditGroup> = Vec::new();
    let mut index: BTreeMap<AuditKey, usize> = BTreeMap::new();
    let mut untrusted = AuditUntrusted::default();
    let mut runs = Vec::new();
    for summary in &summaries {
        let run = reader.load(summary.id)?;
        let under = DecidedUnder::find(&changes, run.recording.blueprint_version.as_deref());
        let mut refs = Vec::new();
        for (position, record) in run.recording.decisions.iter().enumerate() {
            let by_default = matches!(record.cause, DecisionCause::Default { .. });
            if query.default_only && !(record.allowed && by_default) {
                continue;
            }
            if query.packages_only && record.caller == "main" {
                continue;
            }
            let decision_ref = reference(run.id, position);
            let verdict = Verdict::of(record);
            let decided_by = if record.allowed && by_default {
                allowed_by_default_phrase(record, &under)
            } else {
                format!("{} {}", verdict.done(), decided_by_phrase(record, &under))
            };
            let key = (
                record.caller.clone(),
                record.capability.clone(),
                verdict,
                decided_by.clone(),
            );
            let slot = *index.entry(key).or_insert_with(|| {
                groups.push(AuditGroup {
                    caller: record.caller.clone(),
                    capability: record.capability.clone(),
                    outcome: verdict,
                    decided_by,
                    refs: Vec::new(),
                    origin: None,
                });
                groups.len().saturating_sub(1)
            });
            if let Some(group) = groups.get_mut(slot) {
                group.refs.push(decision_ref);
            }
            if !record.context.is_null() {
                untrusted
                    .contexts
                    .insert(decision_ref, record.context.clone());
            }
            refs.push(decision_ref);
        }
        runs.push(reader.header(&run, refs));
    }

    let mut packages_note = None;
    if groups.iter().any(|group| group.caller != "main") {
        match reader.closure() {
            Ok(closure) => {
                for group in groups.iter_mut().filter(|group| group.caller != "main") {
                    group.origin = Some(origin_text(&group.caller, &closure));
                }
            }
            Err(message) => {
                packages_note = Some(format!(
                    "where the packages came from is unknown: {message}"
                ));
            }
        }
    }

    let mut suggestions: Vec<Next> = groups
        .iter()
        .take(3)
        .filter_map(|group| group.refs.first().copied())
        .map(Next::Explain)
        .collect();
    if !query.default_only {
        suggestions.push(Next::Audit { default_only: true });
    }
    suggestions.push(Next::Changes);
    Ok(AuditResult {
        kind: "audit",
        window,
        runs,
        filters,
        groups,
        packages_note,
        next: next(suggestions),
        untrusted,
    })
}

/// What the audit says of a call the default allowed: that no rule matched it, and the
/// rules that named its capability but whose filters rejected it, when there are some.
fn allowed_by_default_phrase(record: &DecisionRecord, under: &DecidedUnder<'_>) -> String {
    let misses: Vec<String> = record
        .near_misses
        .iter()
        .map(|miss| {
            under
                .rule(
                    &miss.rule.caller,
                    miss.rule.index,
                    miss.rule.name.as_deref(),
                )
                .listed()
        })
        .collect();
    let mut phrase = "allowed by the default (no rule matched it)".to_owned();
    match misses.as_slice() {
        [] => {}
        [only] => phrase.push_str(&format!("; near miss: {only}")),
        _ => phrase.push_str(&format!("; near misses: {}", misses.join(", "))),
    }
    phrase
}

/// How `package` came to be in the closure, in words.
pub(crate) fn origin_text(package: &str, closure: &[ClosureEntry]) -> String {
    let Some(entry) = closure.iter().find(|entry| entry.name == package) else {
        return format!("`{package}` is not in the blueprint's package closure");
    };
    match entry.origin {
        Origin::Blueprint => format!("`{package}` is listed in the blueprint's packages"),
        Origin::Dependency if entry.required_by.is_empty() => {
            format!("Nobody chose `{package}`; it arrived as a dependency")
        }
        Origin::Dependency => {
            let by: Vec<String> = entry
                .required_by
                .iter()
                .map(|name| format!("`{name}`"))
                .collect();
            format!(
                "Nobody chose `{package}`; it arrived as a dependency of {}",
                by.join(", ")
            )
        }
    }
}

// ---- changes -------------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub(crate) struct ChangesResult {
    pub(crate) kind: &'static str,
    pub(crate) versions: Vec<VersionOut>,
    /// Versions whose apply failed: never in force.
    pub(crate) voided: Vec<u64>,
    pub(crate) next: Vec<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct VersionOut {
    pub(crate) version: u64,
    pub(crate) at_micros: u64,
    pub(crate) at: String,
    /// `widening`, `narrowing`, `mixed`, `unknown`, or `initial`.
    pub(crate) classification: String,
    /// The change in plain language.
    pub(crate) summary: String,
    /// The changes that removed a rule's pin to a session variable.
    pub(crate) pin_removals: Vec<String>,
    pub(crate) current: bool,
    /// Runs after this id were decided under this version or a later one.
    pub(crate) after_run: u64,
    pub(crate) changes: Value,
    /// The version's file text: only for `--version`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) text: Option<String>,
}

pub(crate) fn changes(reader: &Reader, only: Option<u64>) -> Result<ChangesResult, ReadError> {
    let changes = reader.changes()?;
    let current = changes.current().map(|version| version.version);
    let mut versions = Vec::new();
    for version in &changes.versions {
        if only.is_some_and(|only| only != version.version) {
            continue;
        }
        let classification = &version.classification;
        let pin_removals = classification["changes"]
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter(|change| change["pin"]["kind"] == "removed")
                    .filter_map(|change| change["summary"].as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        versions.push(VersionOut {
            version: version.version,
            at_micros: version.at_micros,
            at: super::render::rfc3339(version.at_micros),
            classification: classification["classification"]
                .as_str()
                .unwrap_or("unknown")
                .to_owned(),
            summary: version.summary.clone(),
            pin_removals,
            current: current == Some(version.version),
            after_run: version.after_run,
            changes: classification["changes"].clone(),
            text: only.map(|_| version.bytes.clone()),
        });
    }
    if let Some(only) = only
        && versions.is_empty()
    {
        return Err(ReadError::UnknownVersion {
            version: only,
            voided: changes.voided.contains(&only),
        });
    }
    versions.reverse();
    Ok(ChangesResult {
        kind: "changes",
        versions,
        voided: changes.voided.clone(),
        next: next([Next::Audit {
            default_only: false,
        }]),
    })
}

// ---- sessions ------------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub(crate) struct SessionsResult {
    pub(crate) kind: &'static str,
    pub(crate) sessions: Vec<SessionRow>,
    pub(crate) more: usize,
    pub(crate) next: Vec<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct SessionRow {
    pub(crate) session: String,
    pub(crate) started_at_micros: u64,
    pub(crate) started_at: String,
    /// The labels of its runs, and of whoever started it.
    pub(crate) sources: Vec<String>,
    pub(crate) variables: BTreeMap<String, String>,
    pub(crate) run_count: usize,
    pub(crate) runs: Vec<u64>,
    /// Whether any call in it was denied.
    pub(crate) denied: bool,
    /// For a session started through the playground: whether it is still open.
    pub(crate) open: Option<bool>,
    /// A started session not ended here but idle longer than the idle timeout it
    /// started with, which the server has let expire.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub(crate) expired: bool,
    /// When it last did something: its start, or its last run's end.
    #[serde(skip)]
    last_active_micros: u64,
    /// The idle timeout its start recorded, in milliseconds.
    #[serde(skip)]
    idle_timeout_ms: Option<u64>,
}

/// `id`'s row, added with no runs, first seen at `at`, when there is none yet.
fn session_row<'r>(
    rows: &'r mut BTreeMap<String, SessionRow>,
    id: &str,
    at: u64,
) -> &'r mut SessionRow {
    rows.entry(id.to_owned()).or_insert_with(|| SessionRow {
        session: id.to_owned(),
        started_at_micros: at,
        started_at: String::new(),
        sources: Vec::new(),
        variables: BTreeMap::new(),
        run_count: 0,
        runs: Vec::new(),
        denied: false,
        open: None,
        expired: false,
        last_active_micros: at,
        idle_timeout_ms: None,
    })
}

/// Marks as expired, and no longer open, each open session idle at `now_micros` longer
/// than the idle timeout it started with, as the server judges it. A session whose start
/// recorded none is left as it is.
fn mark_expired(rows: &mut BTreeMap<String, SessionRow>, now_micros: u64) {
    for session in rows.values_mut() {
        let Some(timeout_ms) = session.idle_timeout_ms else {
            continue;
        };
        let idle = now_micros.saturating_sub(session.last_active_micros);
        if session.open == Some(true) && idle > timeout_ms.saturating_mul(1000) {
            session.open = Some(false);
            session.expired = true;
        }
    }
}

/// The sessions, newest first; a started session idle past its idle timeout at
/// `now_micros` lists as expired.
pub(crate) fn sessions(
    reader: &Reader,
    limit: usize,
    now_micros: u64,
) -> Result<SessionsResult, ReadError> {
    let mut rows: BTreeMap<String, SessionRow> = BTreeMap::new();
    if let Some(store) = &reader.store {
        for line in store.session_log()? {
            let session = session_row(&mut rows, line.entry.session_id(), line.at_micros);
            match line.entry {
                SessionEntry::Started {
                    variables,
                    label,
                    idle_timeout_ms,
                    ..
                } => {
                    session.started_at_micros = session.started_at_micros.min(line.at_micros);
                    session.last_active_micros = session.last_active_micros.max(line.at_micros);
                    session.variables = variables;
                    if !session.sources.contains(&label) {
                        session.sources.push(label);
                    }
                    session.open = Some(true);
                    session.idle_timeout_ms = idle_timeout_ms;
                }
                SessionEntry::Ended { .. } => session.open = Some(false),
            }
        }
    }
    for summary in reader.summaries()? {
        let Some(id) = &summary.session_id else {
            continue;
        };
        let session = session_row(&mut rows, id, summary.started_at_micros);
        session.started_at_micros = session.started_at_micros.min(summary.started_at_micros);
        if session.variables.is_empty() {
            session.variables = summary.variables.clone();
        }
        if !session.sources.contains(&summary.label) {
            session.sources.push(summary.label.clone());
        }
        session.run_count = session.run_count.saturating_add(1);
        session.runs.push(summary.id);
        session.denied |= summary.denied > 0;
        let ended = summary
            .started_at_micros
            .saturating_add(summary.wall_ms.saturating_mul(1000));
        session.last_active_micros = session.last_active_micros.max(ended);
    }
    mark_expired(&mut rows, now_micros);
    let mut sessions: Vec<SessionRow> = rows.into_values().collect();
    sessions.sort_by(|a, b| {
        b.started_at_micros
            .cmp(&a.started_at_micros)
            .then_with(|| b.runs.last().cmp(&a.runs.last()))
    });
    let limit = limit.max(1);
    let more = sessions.len().saturating_sub(limit);
    sessions.truncate(limit);
    for session in &mut sessions {
        session.started_at = super::render::rfc3339(session.started_at_micros);
    }
    let suggestions: Vec<Next> = sessions
        .iter()
        .take(2)
        .map(|session| Next::RunsInSession(session.session.clone()))
        .collect();
    Ok(SessionsResult {
        kind: "sessions",
        sessions,
        more,
        next: next(suggestions),
    })
}
