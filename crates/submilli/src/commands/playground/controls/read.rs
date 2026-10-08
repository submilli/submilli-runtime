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
/// An audit line: caller, capability, outcome, and what decided.
type AuditKey = (String, String, &'static str, String);

// ---- shared pieces -----------------------------------------------------------------------

/// How a run ended.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "kebab-case", tag = "kind")]
pub(crate) enum Outcome {
    Completed,
    /// A policy denial the program did not catch ended it.
    Denied {
        #[serde(skip_serializing_if = "Option::is_none")]
        decision: Option<String>,
    },
    /// A test run stopped at a call the recording could not answer, or any run was
    /// cancelled.
    Stopped,
    Failed {
        error: String,
    },
    /// The program never reached the runner.
    NotDispatched {
        error: Option<String>,
    },
}

impl Outcome {
    fn of(error: Option<ErrorKind>, dispatched: bool, test: bool, denial: Option<String>) -> Self {
        match error {
            None => Self::Completed,
            Some(_) if !dispatched => Self::NotDispatched {
                error: error.map(kind_name),
            },
            Some(ErrorKind::PermissionDenied) => Self::Denied { decision: denial },
            Some(ErrorKind::Cancelled) if test => Self::Stopped,
            Some(kind) => Self::Failed {
                error: kind_name(kind),
            },
        }
    }

    pub(crate) fn text(&self) -> String {
        match self {
            Self::Completed => "completed".to_owned(),
            Self::Denied { decision: Some(d) } => format!("ended by an uncaught denial ({d})"),
            Self::Denied { decision: None } => "ended by an uncaught denial".to_owned(),
            Self::Stopped => "stopped".to_owned(),
            Self::Failed { error } => format!("failed: {error}"),
            Self::NotDispatched { error } => format!(
                "did not start{}",
                error
                    .as_ref()
                    .map_or_else(String::new, |error| format!(": {error}"))
            ),
        }
    }
}

fn kind_name(kind: ErrorKind) -> String {
    serde_json::to_value(kind)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| format!("{kind:?}"))
}

/// A rule of the blueprint, and where it is in the text of the version the run was
/// decided under.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct RuleOut {
    pub(crate) caller: String,
    pub(crate) index: usize,
    pub(crate) name: Option<String>,
    /// 1-based; `None` when the text did not locate it (anchors, flow style, or no text
    /// for the version), and the caller block and index name it instead.
    pub(crate) line: Option<usize>,
    pub(crate) column: Option<usize>,
    pub(crate) end_line: Option<usize>,
}

impl RuleOut {
    pub(crate) fn text(&self) -> String {
        let name = self.name.as_deref().map_or_else(String::new, |name| {
            format!("`{}` ", super::render::clean(name))
        });
        let place = match self.line {
            Some(line) => format!("{} #{}, line {line}", self.caller, self.index),
            None => format!("{} #{}", self.caller, self.index),
        };
        format!("{name}({})", super::render::clean(&place))
    }

    fn short(&self) -> String {
        match &self.name {
            Some(name) => format!("`{}`", super::render::clean(name)),
            None => format!("{} #{}", super::render::clean(&self.caller), self.index),
        }
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

fn outcome_word(record: &DecisionRecord) -> &'static str {
    match (record.allowed, record.action) {
        (true, _) => "allow",
        (false, DecisionAction::AskHuman) => "ask",
        (false, _) => "deny",
    }
}

fn is_denial(record: &DecisionRecord) -> bool {
    !record.allowed
}

/// A short phrase for what decided: "by `reads`", "by the default".
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
    if is_denial(record) && !record.near_misses.is_empty() {
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
                    .short()
            })
            .collect();
        phrase.push_str(&format!(" (near miss: {})", misses.join(", ")));
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

fn reference(run: u64, position: usize) -> String {
    DecisionRef {
        run,
        n: position.saturating_add(1),
    }
    .to_string()
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
    fn header(&self, run: &StoredRun, decision_refs: Vec<String>) -> RunRef {
        RunRef {
            run: run.id,
            page: self.page.run(run.id),
            blueprint_version: run.recording.blueprint_version.clone(),
            source: run.label.clone(),
            decision_refs,
        }
    }

    fn summary_header(&self, summary: &RunSummary, decision_refs: Vec<String>) -> RunRef {
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
    pub(crate) runs: Vec<RunRow>,
    /// Matching runs left out by the limit.
    pub(crate) more: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) note: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) empty_message: Option<&'static str>,
    pub(crate) next: Vec<String>,
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
    pub(crate) denied: usize,
    pub(crate) session: Option<String>,
    pub(crate) test_of: Option<u64>,
}

pub(crate) fn runs(reader: &Reader, query: &RunsQuery) -> Result<RunsResult, ReadError> {
    let mut summaries = reader.summaries()?;
    let store_empty = summaries.is_empty();
    summaries.retain(|summary| {
        query
            .source
            .as_ref()
            .is_none_or(|source| &summary.label == source)
            && query
                .session
                .as_ref()
                .is_none_or(|session| summary.session_id.as_ref() == Some(session))
            && match query.since {
                None => true,
                Some(Since::Run(run)) => summary.id > run,
                Some(Since::Micros(window)) => {
                    summary.started_at_micros >= query.now_micros.saturating_sub(window)
                }
            }
    });
    summaries.sort_by_key(|summary| std::cmp::Reverse(summary.id));
    let limit = query.limit.max(1);
    let more = summaries.len().saturating_sub(limit);
    summaries.truncate(limit);
    let mut rows = Vec::with_capacity(summaries.len());
    for summary in &summaries {
        // Only a run with denials is read in full, for its denials' refs.
        let (denials, last_denial) = if summary.denied > 0 {
            let run = reader.load(summary.id)?;
            let refs: Vec<String> = run
                .recording
                .decisions
                .iter()
                .enumerate()
                .filter(|(_, record)| is_denial(record) && !record.filtered)
                .map(|(position, _)| reference(run.id, position))
                .collect();
            let last = refs.last().cloned();
            (refs, last)
        } else {
            (Vec::new(), None)
        };
        rows.push(RunRow {
            header: reader.summary_header(summary, denials),
            started_at_micros: summary.started_at_micros,
            started_at: super::render::rfc3339(summary.started_at_micros),
            outcome: Outcome::of(
                summary.error,
                summary.dispatched,
                summary.entry == "test",
                last_denial,
            ),
            decisions: summary.decisions,
            denied: summary.denied,
            session: summary.session_id.clone(),
            test_of: summary.test_of.as_ref().and_then(|link| link.run),
        });
    }
    let mut suggestions = Vec::new();
    for row in &rows {
        if let Some(denial) = row.header.decision_refs.first()
            && let Ok(decision) = denial.parse()
        {
            suggestions.push(Next::Explain(decision));
        }
    }
    if let Some(row) = rows.first() {
        suggestions.insert(0, Next::Show(row.header.run));
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
        empty_message: rows.is_empty().then_some(if store_empty {
            EMPTY_STORE
        } else {
            "No runs match."
        }),
        runs: rows,
        more,
        next: next(suggestions),
    })
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
    pub(crate) session: Option<String>,
    pub(crate) variables: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) note: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) test: Option<TestInfo>,
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
    pub(crate) refs: Vec<String>,
    pub(crate) outcome: &'static str,
    pub(crate) caller: String,
    pub(crate) capability: String,
    pub(crate) decided_by: String,
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
    /// Variables the test run bound that the tested run did not.
    pub(crate) variables_filled: Vec<String>,
    /// Variables the tested run bound that the test run did not.
    pub(crate) variables_dropped: Vec<String>,
    pub(crate) local_state: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) not_stored: Option<&'static str>,
}

/// Everything in a shown run that came from inside it.
#[derive(Debug, Default, Serialize)]
pub(crate) struct ShowUntrusted {
    /// Each decision line's context, by the line's first ref.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) contexts: BTreeMap<String, Value>,
    /// Why the runtime refused a call, by ref.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) reasons: BTreeMap<String, String>,
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
    let run = reader.load(id)?;
    let changes = reader.changes()?;
    let under = DecidedUnder::find(&changes, run.recording.blueprint_version.as_deref());
    let mut untrusted = ShowUntrusted::default();

    // Identical allowed decisions (same caller, capability, cause, and context) share a
    // line; a run is decided under one version, so these are within one version.
    let mut lines: Vec<DecisionLine> = Vec::new();
    let mut groups: BTreeMap<(String, String, String, String), usize> = BTreeMap::new();
    let mut last_denial = None;
    for (position, record) in run.recording.decisions.iter().enumerate() {
        let decision_ref = reference(run.id, position);
        if is_denial(record) && !record.filtered {
            last_denial = Some(decision_ref.clone());
        }
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
                .insert(decision_ref.clone(), record.context.clone());
        }
        if let DecisionCause::RuntimeInvariant { reason } = &record.cause {
            untrusted
                .reasons
                .insert(decision_ref.clone(), reason.clone());
        }
        lines.push(DecisionLine {
            refs: vec![decision_ref],
            outcome: outcome_word(record),
            caller: record.caller.clone(),
            capability: record.capability.clone(),
            decided_by: decided_by_phrase(record, &under),
            filtered: record.filtered,
        });
    }

    let mut calls = CallSummary::default();
    for call in &run.recording.calls {
        calls.count += 1;
        match call.outcome {
            Some(interpreter::runtime::CallOutcome::Returned) => calls.returned += 1,
            Some(interpreter::runtime::CallOutcome::Failed) => calls.failed += 1,
            Some(interpreter::runtime::CallOutcome::Unfinished) | None => calls.unfinished += 1,
        }
        let has_body = [&call.request, &call.response]
            .iter()
            .any(|side| side.as_ref().is_some_and(|payload| payload.body.is_some()));
        if has_body {
            calls.with_bodies += 1;
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

    if let Some(error) = &run.error {
        untrusted.error = Some(cap_text(&error.message, include_payloads));
        untrusted.diagnostics.clone_from(&error.diagnostics);
    }
    untrusted.result = run
        .result
        .as_deref()
        .map(|result| cap_text(result, include_payloads));
    untrusted.console = cap_text(&run.console, include_payloads);

    let is_test = run.entry == "test" || run.test_of.is_some();
    let outcome = Outcome::of(
        run.error.as_ref().map(|error| error.kind),
        run.dispatched,
        is_test,
        last_denial,
    );
    let test = if is_test {
        Some(test_info(reader, &run, &outcome))
    } else {
        None
    };

    let denials: Vec<DecisionRef> = run
        .recording
        .decisions
        .iter()
        .enumerate()
        .filter(|(_, record)| is_denial(record))
        .map(|(position, _)| DecisionRef {
            run: run.id,
            n: position.saturating_add(1),
        })
        .collect();
    let mut suggestions = Vec::new();
    for denial in denials.iter().take(2) {
        suggestions.push(Next::Explain(*denial));
        suggestions.push(Next::DraftRule(*denial));
    }
    if let Some(source) = test.as_ref().and_then(|test| test.source_run) {
        suggestions.push(Next::Compare(source, run.id));
    }
    if suggestions.is_empty()
        && let Some(first) = lines.first().and_then(|line| line.refs.first())
        && let Ok(decision) = first.parse()
    {
        suggestions.push(Next::Explain(decision));
    }
    if let Some(session) = &run.recording.session_id {
        suggestions.push(Next::RunsInSession(session.clone()));
    }

    let refs = (1..=run.recording.decisions.len())
        .map(|n| DecisionRef { run: run.id, n }.to_string())
        .collect();
    Ok(ShowResult {
        kind: "run",
        header: reader.header(&run, refs),
        blueprint: run.recording.blueprint_name.clone(),
        entry: run.entry.clone(),
        started_at_micros: run.started_at_micros,
        started_at: super::render::rfc3339(run.started_at_micros),
        wall_ms: run.wall_ms,
        outcome,
        session: run.recording.session_id.clone(),
        variables: run.recording.variables.clone(),
        note: (run.label == "app").then_some(APP_NOTE),
        test,
        decision_count: run.recording.decisions.len(),
        decisions: lines,
        decisions_dropped: run.decisions_dropped,
        log_truncated: run.recording.log_truncated,
        calls,
        payloads_included: include_payloads,
        next: next(suggestions),
        untrusted,
    })
}

fn test_info(reader: &Reader, run: &StoredRun, outcome: &Outcome) -> TestInfo {
    let source_run = run.test_of.as_ref().and_then(|link| link.run);
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
    let status = match outcome {
        Outcome::Stopped => {
            "stopped at a call with nothing recorded (what it stopped at is in run-data)".to_owned()
        }
        other => other.text(),
    };
    TestInfo {
        source_run,
        status,
        variables_filled: filled,
        variables_dropped: dropped,
        local_state: TEST_LOCAL_STATE,
        not_stored: Some(TEST_NOT_STORED),
    }
}

// ---- explain -------------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub(crate) struct ExplainResult {
    pub(crate) kind: &'static str,
    /// `page` here links the decision itself.
    #[serde(flatten)]
    pub(crate) header: RunRef,
    pub(crate) decision: String,
    pub(crate) outcome: &'static str,
    pub(crate) caller: String,
    pub(crate) capability: String,
    pub(crate) decided_by: DecidedBy,
    pub(crate) near_misses: Vec<NearMissOut>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) version_note: Option<&'static str>,
    pub(crate) next: Vec<String>,
    pub(crate) untrusted: ExplainUntrusted,
}

#[derive(Debug, Serialize)]
pub(crate) struct DecidedBy {
    /// `rule`, `default`, `runtime`, or `unexplained`.
    pub(crate) kind: &'static str,
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
        self.kind == "default"
    }

    pub(crate) fn text(&self) -> String {
        match (self.kind, &self.rule) {
            ("rule", Some(rule)) => format!("rule {}", rule.text()),
            ("default", _) => {
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
            ("runtime", _) => {
                "the runtime, ahead of the policy (its reason is in run-data)".to_owned()
            }
            _ => "the policy, which did not explain it".to_owned(),
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
                kind: "rule",
                rule: Some(under.rule(&rule.caller, rule.index, rule.name.as_deref())),
                filter: under.filter(&rule.caller, rule.index),
                default_action: None,
                caller_block: None,
            },
            None,
        ),
        DecisionCause::Default { caller_block } => (
            DecidedBy {
                kind: "default",
                rule: None,
                filter: None,
                default_action: under.default_action(),
                caller_block: Some(*caller_block),
            },
            None,
        ),
        DecisionCause::RuntimeInvariant { reason } => (
            DecidedBy {
                kind: "runtime",
                rule: None,
                filter: None,
                default_action: None,
                caller_block: None,
            },
            Some(reason.clone()),
        ),
        DecisionCause::Unexplained => (
            DecidedBy {
                kind: "unexplained",
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
    let mut header = reader.header(&run, vec![decision.to_string()]);
    header.page = reader.page.decision(decision);
    Ok(ExplainResult {
        kind: "decision",
        header,
        decision: decision.to_string(),
        outcome: outcome_word(record),
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
    pub(crate) before: String,
    pub(crate) after: String,
    pub(crate) caller: String,
    pub(crate) capability: String,
    /// What decided in the later run.
    pub(crate) decided_by: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) rule: Option<RuleOut>,
    /// What decided in the earlier run, for a different-rule change.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) was: Option<String>,
}

#[derive(Debug, Default, Serialize)]
pub(crate) struct CompareUntrusted {
    /// Each changed decision's context in the later run, by its ref there.
    pub(crate) contexts: BTreeMap<String, Value>,
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
    let mut unmatched = Vec::new();
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
                untrusted
                    .contexts
                    .insert(after_ref.clone(), now.context.clone());
            }
            let rule = match &now.cause {
                DecisionCause::Rule(rule) => {
                    Some(after_under.rule(&rule.caller, rule.index, rule.name.as_deref()))
                }
                _ => None,
            };
            before_refs.push(before_ref.clone());
            after_refs.push(after_ref.clone());
            flips.push(Flip {
                kind,
                before: before_ref,
                after: after_ref,
                caller: now.caller.clone(),
                capability: now.capability.clone(),
                decided_by: decided_by_phrase(now, &after_under),
                rule,
                was: (kind == FlipKind::DifferentRule)
                    .then(|| decided_by_phrase(was, &before_under)),
            });
        }
        if after_list.len() > before_list.len() {
            unmatched.push(format!(
                "{} more {} → {} decisions in run {} than in run {}",
                after_list.len() - before_list.len(),
                key.0,
                key.1,
                after.id,
                before.id
            ));
        }
    }
    for (key, before_list) in &before_positions {
        let after_len = after_positions.get(key).map_or(0, Vec::len);
        if before_list.len() > after_len {
            unmatched.push(format!(
                "{} more {} → {} decisions in run {} than in run {}",
                before_list.len() - after_len,
                key.0,
                key.1,
                before.id,
                after.id
            ));
        }
    }
    flips.sort_by_key(|flip| flip.after.parse::<DecisionRef>().ok());

    let mut suggestions = Vec::new();
    for flip in flips.iter().take(3) {
        if let Ok(decision) = flip.after.parse() {
            suggestions.push(Next::Explain(decision));
        }
    }
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
    pub(crate) outcome: &'static str,
    pub(crate) decided_by: String,
    pub(crate) refs: Vec<String>,
    /// For a package caller: how it came to be in the closure.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) origin: Option<String>,
}

#[derive(Debug, Default, Serialize)]
pub(crate) struct AuditUntrusted {
    /// Each listed decision's context, by ref.
    pub(crate) contexts: BTreeMap<String, Value>,
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
        filters.push("calls allowed only by the default (no rule names them)");
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
            let decided_by = if record.allowed && by_default {
                "allowed by the default (no rule names it)".to_owned()
            } else {
                let verb = match outcome_word(record) {
                    "allow" => "allowed",
                    "ask" => "held for a human",
                    _ => "denied",
                };
                format!("{verb} {}", decided_by_phrase(record, &under))
            };
            let key = (
                record.caller.clone(),
                record.capability.clone(),
                outcome_word(record),
                decided_by.clone(),
            );
            let slot = *index.entry(key).or_insert_with(|| {
                groups.push(AuditGroup {
                    caller: record.caller.clone(),
                    capability: record.capability.clone(),
                    outcome: outcome_word(record),
                    decided_by,
                    refs: Vec::new(),
                    origin: None,
                });
                groups.len() - 1
            });
            if let Some(group) = groups.get_mut(slot) {
                group.refs.push(decision_ref.clone());
            }
            if !record.context.is_null() {
                untrusted
                    .contexts
                    .insert(decision_ref.clone(), record.context.clone());
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

    let mut suggestions = Vec::new();
    for group in groups.iter().take(3) {
        if let Some(Ok(decision)) = group.refs.first().map(|r| r.parse()) {
            suggestions.push(Next::Explain(decision));
        }
    }
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
}

pub(crate) fn sessions(reader: &Reader, limit: usize) -> Result<SessionsResult, ReadError> {
    let mut rows: BTreeMap<String, SessionRow> = BTreeMap::new();
    let row = |rows: &mut BTreeMap<String, SessionRow>, id: &str, at: u64| {
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
        });
    };
    if let Some(store) = &reader.store {
        for line in store.session_log()? {
            row(&mut rows, line.entry.session_id(), line.at_micros);
            let Some(session) = rows.get_mut(line.entry.session_id()) else {
                continue;
            };
            match line.entry {
                SessionEntry::Started {
                    variables, label, ..
                } => {
                    session.started_at_micros = session.started_at_micros.min(line.at_micros);
                    session.variables = variables;
                    if !session.sources.contains(&label) {
                        session.sources.push(label);
                    }
                    session.open = Some(true);
                }
                SessionEntry::Ended { .. } => session.open = Some(false),
            }
        }
    }
    for summary in reader.summaries()? {
        let Some(id) = &summary.session_id else {
            continue;
        };
        row(&mut rows, id, summary.started_at_micros);
        let Some(session) = rows.get_mut(id) else {
            continue;
        };
        session.started_at_micros = session.started_at_micros.min(summary.started_at_micros);
        if session.variables.is_empty() {
            session.variables = summary.variables.clone();
        }
        if !session.sources.contains(&summary.label) {
            session.sources.push(summary.label.clone());
        }
        session.run_count += 1;
        session.runs.push(summary.id);
        session.denied |= summary.denied > 0;
    }
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
