//! Decision records: what an embedder's recorder sees of each capability decision.
//!
//! The recorder is a [`SecurityCheck`] decorator ([`DecisionLog::install`]): it forwards
//! every call to the check it wraps, including `audit`, so the embedder's own audit sees
//! exactly what it would without a recorder, and taps the same `audit` calls. Recording
//! never changes a decision, never fails a call, and never charges guest fuel.
//!
//! Observation work (the source-line backtrace per call) is never charged to guest fuel;
//! see [`DecisionLogConfig::max_line_capture_frames`] for why and what bounds it. Lines stop
//! being captured when the decision cap is reached, when the byte budget cannot fit even a
//! bare record, after a record has been dropped whole for want of bytes, or when that frame
//! budget is exceeded. A later, smaller record may still be kept after such a drop, without
//! a line. Other truncation (a byte-budget payload drop, the pair cap) does not stop them.
//!
//! Record buffers are charged against the recorder's own byte budget, separate from the
//! run's memory limit so that recording never changes a run's memory behavior, and a
//! per-run decision cap; a run that reaches either is marked truncated rather than refused.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

use serde::Serialize;
use serde_json::Value;

use super::StoreData;
use super::security::{AuditDecision, CheckOutcome, SecurityCheck};

/// What the policy decided. `AskHuman` is a denial today (the approval flow is deferred),
/// kept distinct so a record can say "deferred".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum DecisionAction {
    Allow,
    Deny,
    AskHuman,
}

/// A rule located by caller block and zero-based position.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RuleCitation {
    pub caller: String,
    pub index: usize,
    pub name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case", tag = "kind")]
pub enum DecisionCause {
    /// A rule matched.
    Rule(RuleCitation),
    /// No rule matched. `caller_block` is whether the caller has any rules.
    Default { caller_block: bool },
    /// A runtime invariant refused the call ahead of, or instead of, the policy:
    /// an unattributable caller, `main` calling a `main_denial` capability, a read-only
    /// volume, a quota or egress refusal, or a path the policy could not normalize.
    RuntimeInvariant { reason: String },
    /// The installed policy does not explain its decisions.
    Unexplained,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case", tag = "kind", content = "variable")]
pub enum FailureReasonRecord {
    FieldMissing,
    VariableNotBound(String),
    NotSatisfied,
}

/// One comparison that kept a rule's filter from matching.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FailureRecord {
    pub comparison: String,
    pub actual: Option<Value>,
    pub expected: Option<String>,
    pub reason: FailureReasonRecord,
    pub negated: bool,
}

/// A rule that named the capability but whose filter rejected the call.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NearMissRecord {
    pub rule: RuleCitation,
    pub filter: String,
    pub failures: Vec<FailureRecord>,
}

/// The policy's reasoning for one decision.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DecisionExplanation {
    pub action: DecisionAction,
    pub cause: DecisionCause,
    pub near_misses: Vec<NearMissRecord>,
}

/// Which seam produced a decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "kebab-case", tag = "kind")]
pub enum EntryPath {
    /// A gated standard-library operation.
    #[default]
    GatedOp,
    /// A package's `security.check` call, attributed to its consumer.
    PackageCheck,
    /// A redirect hop of `parent_call_index`'s request; `index` counts its hops from 0.
    RedirectHop {
        parent_call_index: u64,
        index: u32,
    },
    Git,
    /// A file tool of the server, outside any program run.
    FileTool,
}

/// A position in the submitted program.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct SourceLine {
    pub line: u32,
    pub column: Option<u32>,
}

/// Identity of one host call, assigned when it begins.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct CallTicket {
    /// Run-wide, in call order; counts every call the recorder saw.
    pub call_index: u64,
    /// Counts the calls of this (caller, capability) pair in the run, denials included,
    /// from 1. Zero when the recorder stopped tracking pairs at its cap.
    pub seq: u64,
    /// Microseconds since the recorder was installed, from a clock that only moves forward.
    pub at_micros: u64,
    pub line: Option<SourceLine>,
}

/// The call a decision belongs to and how it arrived.
#[derive(Debug, Clone, Copy, Default)]
pub struct CallSite {
    pub ticket: Option<CallTicket>,
    pub entry: EntryPath,
}

impl CallSite {
    pub fn new(ticket: Option<CallTicket>, entry: EntryPath) -> Self {
        Self { ticket, entry }
    }
}

/// The runtime's side of a recorder: identity for each call. Record contents arrive through
/// [`SecurityCheck::audit`].
pub trait DecisionRecorder: Send + Sync {
    /// A new host call. `line` is the program line that led to it, when known.
    fn begin_call(&self, caller: &str, capability: &str, line: Option<SourceLine>) -> CallTicket;
    /// Marks the latest call's record as a denial the host function treated as a filter.
    fn mark_last_filtered(&self);
    /// Whether the next call's source line would be kept. Callers check it before paying
    /// for a backtrace; it is false once the decision cap is reached, the byte budget cannot
    /// fit even a bare record, a record has been dropped whole for want of bytes, or the
    /// frame budget is exceeded. Other truncation (a payload drop, the pair cap) does not
    /// turn it off.
    fn wants_line(&self) -> bool;
    /// Counts the frames of a backtrace captured for a line. Past the recorder's frame
    /// budget it marks the run truncated and stops further captures.
    fn note_line_capture(&self, frames: usize);
}

#[derive(Debug, Clone, Serialize)]
pub struct DecisionRecord {
    pub call_index: u64,
    pub seq: u64,
    pub at_micros: u64,
    pub caller: String,
    pub capability: String,
    /// Capped; see [`DecisionLogConfig::max_context_value_bytes`].
    pub context: Value,
    pub context_truncated: bool,
    /// Hash of the full context, kept when the payload copy is dropped.
    pub context_digest: u64,
    pub allowed: bool,
    pub action: DecisionAction,
    pub cause: DecisionCause,
    pub near_misses: Vec<NearMissRecord>,
    /// The audit's source: `policy`, `invariant`, `read_only`, `quota`, `egress_guard`.
    pub source: String,
    pub rule: Option<usize>,
    pub reason: Option<String>,
    pub entry_path: EntryPath,
    pub line: Option<SourceLine>,
    /// A denial the host function swallowed to filter a listing.
    pub filtered: bool,
    /// The context and near misses were dropped to stay within the recorder's byte budget.
    pub payload_dropped: bool,
}

#[derive(Debug, Clone)]
pub struct DecisionLogConfig {
    /// Decisions kept per run; later ones are counted and mark the run truncated.
    pub max_decisions: usize,
    /// Longest string kept in a recorded context; longer ones end in a marker.
    pub max_context_value_bytes: usize,
    /// Bytes the recorder may hold, independent of the run's memory limit. Past it, payloads
    /// are dropped (verdict and digest kept), then whole records.
    pub max_recorder_bytes: u64,
    /// Wasm frames the recorder may walk, summed over the run's source-line captures.
    ///
    /// Line capture is observation work, deliberately left uncharged to guest fuel so that
    /// recording never changes a run: a recorded run spends exactly the fuel an unrecorded
    /// one does (R19). That deviates from AGENTS.md, "Fuel for host functions", which asks
    /// host work to be charged; this budget bounds the work instead, together with the
    /// decision cap. The fuel plan's "Gated functions call `check_security`" bullet
    /// (`plans/sub-1269-host-fuel-costs.md`) is the written record of the deviation.
    ///
    /// The budget is checked before each capture, so it can be exceeded by at most one
    /// stack: the capture that crosses it keeps its line. Past it no new line is captured
    /// and the run is marked truncated (a redirect hop or git worker check may still carry
    /// a line captured earlier).
    pub max_line_capture_frames: u64,
}

impl Default for DecisionLogConfig {
    fn default() -> Self {
        Self {
            max_decisions: 10_000,
            max_context_value_bytes: 1024,
            max_recorder_bytes: 16 * 1024 * 1024,
            max_line_capture_frames: 1_000_000,
        }
    }
}

/// What a run recorded.
#[derive(Debug, Clone, Default)]
pub struct DecisionLogOutput {
    pub records: Vec<DecisionRecord>,
    /// Some decision was not kept in full: past the cap, over the byte budget, beyond the
    /// pairs the recorder tracks, or the line-capture frame budget was exceeded (after which
    /// no new lines are captured).
    pub truncated: bool,
    /// Decisions not kept at all.
    pub dropped: u64,
    /// Wasm frames walked by line captures over the run (diagnostic).
    pub line_frames: u64,
}

/// Distinct (caller, capability) pairs tracked per run. Capability names come from
/// `security.check` calls, so the set is not bounded by the catalog.
const MAX_TRACKED_PAIRS: usize = 4096;
const PAIR_OVERHEAD_BYTES: u64 = 96;
const RECORD_BASE_BYTES: u64 = std::mem::size_of::<DecisionRecord>() as u64;
const MAX_CONTEXT_DEPTH: usize = 8;
const MAX_CONTEXT_ENTRIES: usize = 64;

#[derive(Default)]
struct PairState {
    count: u64,
    last: Option<CallTicket>,
}

#[derive(Default)]
struct LogState {
    records: Vec<DecisionRecord>,
    pairs: HashMap<(String, String), PairState>,
    next_call_index: u64,
    last_call_index: Option<u64>,
    truncated: bool,
    dropped: u64,
    charged: u64,
    line_frames: u64,
    /// The frame budget was exceeded: no new lines are captured.
    lines_exhausted: bool,
    /// A record was dropped whole for want of bytes: no new lines are captured. A smaller
    /// later record may still be kept, without a line.
    bytes_exhausted: bool,
}

/// The per-run recorder.
pub struct DecisionLog {
    state: Mutex<LogState>,
    config: DecisionLogConfig,
    started: Instant,
    clock: AtomicU64,
}

impl DecisionLog {
    /// Wraps the store's security check in a recording decorator. Returns the log to read
    /// results from.
    ///
    /// Install it outermost: after any embedder wrapper of the check (such as the server's
    /// audit decorator). Other wrappers do not forward [`SecurityCheck::recorder`], so a
    /// recorder installed beneath one is invisible to the host functions that begin calls.
    pub fn install(data: &mut StoreData, config: DecisionLogConfig) -> Arc<Self> {
        let log = Arc::new(Self {
            state: Mutex::new(LogState::default()),
            config,
            started: Instant::now(),
            clock: AtomicU64::new(0),
        });
        data.security_check = Arc::new(RecordingCheck {
            inner: data.security_check.clone(),
            log: log.clone(),
        });
        log
    }

    /// Takes the run's records and releases their byte charge. Call once, when the
    /// run ends: the truncation flags, drop count, and call numbering describe the
    /// whole run, so a later call would not describe only its own records.
    pub fn finish(&self) -> DecisionLogOutput {
        let mut state = self.lock();
        let records = std::mem::take(&mut state.records);
        let output = DecisionLogOutput {
            records,
            truncated: state.truncated,
            dropped: state.dropped,
            line_frames: state.line_frames,
        };
        state.charged = 0;
        state.pairs.clear();
        output
    }

    fn lock(&self) -> MutexGuard<'_, LogState> {
        // A poisoned lock means a recorder call panicked while it may have been updating
        // this state, so the state could be partly updated. Recovering is acceptable here
        // (unlike the accepted poisoned-lock panics in AGENTS.md) only because the log is
        // observation-only: it never feeds a decision, so a skewed record cannot change
        // what the run is allowed to do.
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn now_micros(&self) -> u64 {
        let elapsed = u64::try_from(self.started.elapsed().as_micros()).unwrap_or(u64::MAX);
        let previous = self.clock.fetch_max(elapsed, Ordering::Relaxed);
        previous.max(elapsed)
    }

    fn charge(&self, state: &mut LogState, bytes: u64) -> bool {
        let next = state.charged.saturating_add(bytes);
        if next > self.config.max_recorder_bytes {
            return false;
        }
        state.charged = next;
        true
    }

    /// Whether the decision cap and the byte budget leave room for one more bare record.
    fn can_keep_another(&self, state: &LogState) -> bool {
        state.records.len() < self.config.max_decisions
            && state.charged.saturating_add(RECORD_BASE_BYTES) <= self.config.max_recorder_bytes
    }

    fn begin(
        &self,
        state: &mut LogState,
        caller: &str,
        capability: &str,
        line: Option<SourceLine>,
    ) -> CallTicket {
        let call_index = state.next_call_index;
        state.next_call_index = state.next_call_index.saturating_add(1);
        state.last_call_index = Some(call_index);
        let key = (caller.to_owned(), capability.to_owned());
        if !state.pairs.contains_key(&key) {
            let cost = (caller.len() + capability.len()) as u64 + PAIR_OVERHEAD_BYTES;
            if state.pairs.len() >= MAX_TRACKED_PAIRS || !self.charge(state, cost) {
                state.truncated = true;
                return CallTicket {
                    call_index,
                    seq: 0,
                    at_micros: self.now_micros(),
                    line,
                };
            }
        }
        let pair = state.pairs.entry(key).or_default();
        pair.count = pair.count.saturating_add(1);
        let ticket = CallTicket {
            call_index,
            seq: pair.count,
            at_micros: self.now_micros(),
            line,
        };
        pair.last = Some(ticket);
        ticket
    }

    /// Stores what the audit saw. Called from the decorator's `audit`.
    fn record(&self, decision: &AuditDecision<'_>) {
        let mut state = self.lock();
        let ticket = match decision.site.ticket {
            Some(ticket) => ticket,
            // A decision with no call is its own call, never a follow-up of another one.
            None => self.begin(&mut state, decision.caller, decision.capability, None),
        };
        if state.records.len() >= self.config.max_decisions || state.records.try_reserve(1).is_err()
        {
            state.truncated = true;
            state.dropped = state.dropped.saturating_add(1);
            return;
        }
        let mut record = self.build(decision, ticket);
        let minimal = RECORD_BASE_BYTES
            + (record.caller.len()
                + record.capability.len()
                + record.source.len()
                + record.reason.as_ref().map_or(0, String::len)) as u64;
        let payload = payload_bytes(&record);
        if self.charge(&mut state, minimal.saturating_add(payload)) {
            state.records.push(record);
            return;
        }
        // Keep the digest and the verdict; drop what carries the bytes.
        record.context = Value::Null;
        record.near_misses = Vec::new();
        record.payload_dropped = true;
        record.context_truncated = true;
        state.truncated = true;
        if self.charge(&mut state, minimal) {
            state.records.push(record);
        } else {
            state.bytes_exhausted = true;
            state.dropped = state.dropped.saturating_add(1);
        }
    }

    fn build(&self, decision: &AuditDecision<'_>, ticket: CallTicket) -> DecisionRecord {
        let digest = digest_of(decision.context);
        let (context, mut context_truncated) = if decision.capability == "secrets.get" {
            // The key name is the record; nothing else a secret read carries is kept.
            let name = decision.context.get("name").cloned().unwrap_or(Value::Null);
            cap_context(
                &serde_json::json!({ "name": name }),
                self.config.max_context_value_bytes,
            )
        } else {
            cap_context(decision.context, self.config.max_context_value_bytes)
        };
        let (action, cause, near_misses) = match decision.explanation {
            Some(explanation) => (
                enforced_action(decision.allowed, Some(explanation.action)),
                explanation.cause.clone(),
                self.capped_near_misses(&explanation.near_misses, &mut context_truncated),
            ),
            None => (
                enforced_action(decision.allowed, None),
                fallback_cause(decision),
                Vec::new(),
            ),
        };
        DecisionRecord {
            call_index: ticket.call_index,
            seq: ticket.seq,
            at_micros: ticket.at_micros,
            caller: decision.caller.to_owned(),
            capability: decision.capability.to_owned(),
            context,
            context_truncated,
            context_digest: digest,
            allowed: decision.allowed,
            action,
            cause,
            near_misses,
            source: decision.source.to_owned(),
            rule: decision.rule,
            reason: decision.reason.map(str::to_owned),
            entry_path: decision.site.entry,
            line: ticket.line,
            filtered: false,
            payload_dropped: false,
        }
    }

    /// Near misses carry the call's actual values, which are as unbounded as the context.
    fn capped_near_misses(
        &self,
        near_misses: &[NearMissRecord],
        truncated: &mut bool,
    ) -> Vec<NearMissRecord> {
        let max = self.config.max_context_value_bytes;
        near_misses
            .iter()
            .map(|miss| NearMissRecord {
                rule: miss.rule.clone(),
                filter: miss.filter.clone(),
                failures: miss
                    .failures
                    .iter()
                    .map(|failure| FailureRecord {
                        actual: failure
                            .actual
                            .as_ref()
                            .map(|actual| cap_value(actual, max, 0, truncated)),
                        ..failure.clone()
                    })
                    .collect(),
            })
            .collect()
    }
}

impl DecisionRecorder for DecisionLog {
    fn begin_call(&self, caller: &str, capability: &str, line: Option<SourceLine>) -> CallTicket {
        let mut state = self.lock();
        self.begin(&mut state, caller, capability, line)
    }

    fn wants_line(&self) -> bool {
        let state = self.lock();
        !state.lines_exhausted && !state.bytes_exhausted && self.can_keep_another(&state)
    }

    fn note_line_capture(&self, frames: usize) {
        let mut state = self.lock();
        state.line_frames = state.line_frames.saturating_add(frames as u64);
        if state.line_frames > self.config.max_line_capture_frames {
            state.lines_exhausted = true;
            state.truncated = true;
        }
    }

    fn mark_last_filtered(&self) {
        let mut state = self.lock();
        let Some(last) = state.last_call_index else {
            return;
        };
        if let Some(record) = state
            .records
            .iter_mut()
            .rev()
            .find(|record| record.call_index == last)
        {
            record.filtered = true;
        }
    }
}

/// The action the enforcement path took; the explanation only refines a denial into
/// `AskHuman`, so a disagreeing explanation cannot change what is recorded as allowed.
fn enforced_action(allowed: bool, explained: Option<DecisionAction>) -> DecisionAction {
    match (allowed, explained) {
        (true, _) => DecisionAction::Allow,
        (false, Some(DecisionAction::AskHuman)) => DecisionAction::AskHuman,
        (false, _) => DecisionAction::Deny,
    }
}

fn fallback_cause(decision: &AuditDecision<'_>) -> DecisionCause {
    if decision.source == "policy" {
        return DecisionCause::Unexplained;
    }
    DecisionCause::RuntimeInvariant {
        reason: decision.reason.unwrap_or_default().to_owned(),
    }
}

fn digest_of(context: &Value) -> u64 {
    use std::hash::{Hash, Hasher};
    let Ok(text) = serde_json::to_string(context) else {
        return 0;
    };
    // `DefaultHasher::new()` is keyed with fixed constants, so a digest is stable in a run.
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

fn payload_bytes(record: &DecisionRecord) -> u64 {
    let context = serde_json::to_string(&record.context).map_or(0, |text| text.len());
    let misses = serde_json::to_string(&record.near_misses).map_or(0, |text| text.len());
    // Parsed JSON costs several times its text; charge generously.
    (context.saturating_add(misses) as u64).saturating_mul(2)
}

/// Caps a recorded context: long strings, deep nesting, and wide containers each end in a
/// marker. Returns whether anything was cut.
pub(crate) fn cap_context(context: &Value, max_string_bytes: usize) -> (Value, bool) {
    let mut truncated = false;
    let capped = cap_value(context, max_string_bytes, 0, &mut truncated);
    (capped, truncated)
}

fn cap_value(value: &Value, max_string: usize, depth: usize, truncated: &mut bool) -> Value {
    match value {
        Value::String(text) => Value::String(cap_text(text, max_string, truncated)),
        Value::Array(_) | Value::Object(_) if depth >= MAX_CONTEXT_DEPTH => {
            *truncated = true;
            Value::String("…[truncated, nested too deep]".to_owned())
        }
        Value::Array(items) => {
            if items.len() > MAX_CONTEXT_ENTRIES {
                *truncated = true;
            }
            Value::Array(
                items
                    .iter()
                    .take(MAX_CONTEXT_ENTRIES)
                    .map(|item| cap_value(item, max_string, depth + 1, truncated))
                    .collect(),
            )
        }
        Value::Object(fields) => {
            if fields.len() > MAX_CONTEXT_ENTRIES {
                *truncated = true;
            }
            Value::Object(
                fields
                    .iter()
                    .take(MAX_CONTEXT_ENTRIES)
                    .enumerate()
                    .map(|(position, (key, item))| {
                        (
                            capped_key(key, position, max_string, truncated),
                            cap_value(item, max_string, depth + 1, truncated),
                        )
                    })
                    .collect(),
            )
        }
        other => other.clone(),
    }
}

/// A capped object key. Two long keys can share their kept prefix, so a cut key
/// also names its position in the object to keep both entries.
fn capped_key(key: &str, position: usize, max: usize, truncated: &mut bool) -> String {
    let capped = cap_text(key, max, truncated);
    if capped.len() == key.len() {
        return capped;
    }
    format!("{capped} #{position}")
}

/// `text` cut to `max` bytes at a character boundary, ending in a marker when cut.
fn cap_text(text: &str, max: usize, truncated: &mut bool) -> String {
    if text.len() <= max {
        return text.to_owned();
    }
    *truncated = true;
    let mut end = max;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    let kept = text.get(..end).unwrap_or_default();
    format!(
        "{kept}…[truncated, {} of {} bytes kept]",
        kept.len(),
        text.len()
    )
}

struct RecordingCheck {
    inner: Arc<dyn SecurityCheck>,
    log: Arc<DecisionLog>,
}

impl SecurityCheck for RecordingCheck {
    fn audit(&self, decision: AuditDecision<'_>) {
        self.inner.audit(decision);
        self.log.record(&decision);
    }

    fn audit_context<'a>(
        &self,
        capability: &str,
        context: &'a Value,
        cwd: &str,
    ) -> std::borrow::Cow<'a, Value> {
        self.inner.audit_context(capability, context, cwd)
    }

    fn check_with_cwd(
        &self,
        caller: &str,
        capability: &str,
        context: &Value,
        cwd: &str,
    ) -> CheckOutcome {
        self.inner.check_with_cwd(caller, capability, context, cwd)
    }

    fn check(&self, caller: &str, capability: &str, context: &Value) -> CheckOutcome {
        self.inner.check(caller, capability, context)
    }

    fn recorder(&self) -> Option<&dyn DecisionRecorder> {
        Some(self.log.as_ref())
    }

    fn explain(
        &self,
        caller: &str,
        capability: &str,
        context: &Value,
        cwd: &str,
    ) -> Option<DecisionExplanation> {
        self.inner.explain(caller, capability, context, cwd)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::Vfs;

    fn log_with(config: DecisionLogConfig) -> (StoreData, Arc<DecisionLog>) {
        let mut data = StoreData::with_vfs_and_cap(Vfs::none(), 1 << 20);
        let log = DecisionLog::install(&mut data, config);
        (data, log)
    }

    fn audit_denied(data: &StoreData, ticket: CallTicket) {
        let context = serde_json::json!({});
        data.security_check.audit(
            AuditDecision::new("main", "fs.read", &context, false, "policy", None, None)
                .with_site(CallSite::new(Some(ticket), EntryPath::GatedOp)),
        );
    }

    #[test]
    fn a_full_log_stops_asking_for_lines() {
        let (data, log) = log_with(DecisionLogConfig {
            max_decisions: 2,
            ..DecisionLogConfig::default()
        });
        let recorder = data.security_check.recorder().unwrap();
        for _ in 0..2 {
            assert!(recorder.wants_line());
            audit_denied(&data, recorder.begin_call("main", "fs.read", None));
        }
        assert!(!recorder.wants_line(), "the cap is reached");
        audit_denied(&data, recorder.begin_call("main", "fs.read", None));
        assert!(!recorder.wants_line(), "still off once the log is full");
        assert!(log.finish().truncated);
    }

    #[test]
    fn a_byte_budget_payload_drop_keeps_asking_for_lines() {
        let (data, log) = log_with(DecisionLogConfig {
            max_recorder_bytes: 4 * RECORD_BASE_BYTES,
            ..DecisionLogConfig::default()
        });
        let recorder = data.security_check.recorder().unwrap();
        let context = serde_json::json!({ "path": "p".repeat(900) });
        let ticket = recorder.begin_call("main", "fs.read", None);
        data.security_check.audit(
            AuditDecision::new("main", "fs.read", &context, true, "policy", None, None)
                .with_site(CallSite::new(Some(ticket), EntryPath::GatedOp)),
        );
        assert!(recorder.wants_line(), "a minimal record still fits");
        let output = log.finish();
        assert!(output.truncated);
        assert!(output.records.iter().any(|r| r.payload_dropped));
    }

    #[test]
    fn filling_the_pair_cap_keeps_asking_for_lines() {
        let (data, log) = log_with(DecisionLogConfig::default());
        let recorder = data.security_check.recorder().unwrap();
        for index in 0..=MAX_TRACKED_PAIRS {
            recorder.begin_call("main", &format!("cap.{index}"), None);
        }
        assert!(recorder.wants_line());
        assert!(log.finish().truncated, "the pair cap truncates");
    }

    #[test]
    fn lines_stop_once_the_byte_budget_cannot_fit_a_record() {
        let (data, log) = log_with(DecisionLogConfig {
            max_recorder_bytes: 3 * RECORD_BASE_BYTES,
            ..DecisionLogConfig::default()
        });
        let recorder = data.security_check.recorder().unwrap();
        let mut captures = 0;
        for _ in 0..20 {
            if recorder.wants_line() {
                captures += 1;
                recorder.note_line_capture(5);
            }
            audit_denied(&data, recorder.begin_call("main", "fs.read", None));
        }
        let output = log.finish();
        assert!(output.dropped > 0, "records were dropped whole");
        assert!(
            captures < 20 && output.line_frames == 5 * captures,
            "capture stopped: {captures} captures"
        );
        assert!(
            captures <= output.records.len() as u64 + 1,
            "no captures beyond the first dropped record: {captures} for {} kept",
            output.records.len()
        );
    }

    #[test]
    fn a_budget_below_one_bare_record_wants_no_line() {
        for max_recorder_bytes in [0, RECORD_BASE_BYTES - 1] {
            let (data, _log) = log_with(DecisionLogConfig {
                max_recorder_bytes,
                ..DecisionLogConfig::default()
            });
            let recorder = data.security_check.recorder().unwrap();
            assert!(!recorder.wants_line(), "budget {max_recorder_bytes}");
        }
    }

    #[test]
    fn a_whole_record_drop_stops_lines_though_a_bare_record_would_fit() {
        let (data, log) = log_with(DecisionLogConfig {
            // After the call's pair is charged, the leftover is a bare record plus 2 bytes:
            // less than this record's strings need.
            max_recorder_bytes: PAIR_OVERHEAD_BYTES
                + "main".len() as u64
                + "fs.read".len() as u64
                + RECORD_BASE_BYTES
                + 2,
            ..DecisionLogConfig::default()
        });
        let recorder = data.security_check.recorder().unwrap();
        assert!(recorder.wants_line(), "a bare record fits before any call");
        let ticket = recorder.begin_call("main", "fs.read", None);
        assert!(
            recorder.wants_line(),
            "a bare record still fits after the call"
        );
        audit_denied(&data, ticket);
        assert!(!recorder.wants_line(), "the drop turns lines off");
        let output = log.finish();
        assert_eq!(output.dropped, 1);
        assert!(output.records.is_empty());
    }

    #[test]
    fn the_line_capture_frame_budget_truncates_the_log_once_exceeded() {
        let (data, log) = log_with(DecisionLogConfig {
            max_line_capture_frames: 10,
            ..DecisionLogConfig::default()
        });
        let recorder = data.security_check.recorder().unwrap();
        recorder.note_line_capture(10);
        assert!(recorder.wants_line(), "at the budget is still within it");
        recorder.note_line_capture(1);
        assert!(!recorder.wants_line());
        assert!(log.finish().truncated);
    }

    #[test]
    fn a_secret_read_keeps_the_key_name_and_nothing_else() {
        let (data, log) = log_with(DecisionLogConfig::default());
        let context = serde_json::json!({ "name": "api-key", "value": "s3cret" });
        data.security_check.audit(AuditDecision::new(
            "main",
            "secrets.get",
            &context,
            false,
            "invariant",
            None,
            Some("refused"),
        ));
        let output = log.finish();
        let [record] = output.records.as_slice() else {
            panic!("one record");
        };
        assert_eq!(record.context, serde_json::json!({ "name": "api-key" }));
        assert!(!format!("{record:?}").contains("s3cret"));
    }

    #[test]
    fn an_exhausted_recorder_budget_keeps_the_verdict_and_drops_the_payload() {
        // Room for the pair bookkeeping and a bare record, not for a big context.
        let (data, log) = log_with(DecisionLogConfig {
            max_recorder_bytes: 2048,
            ..DecisionLogConfig::default()
        });
        let context = serde_json::json!({ "path": "p".repeat(900) });
        let recorder = data.security_check.recorder().unwrap();
        let ticket = recorder.begin_call("main", "fs.read", None);
        data.security_check.audit(
            AuditDecision::new("main", "fs.read", &context, true, "policy", None, None)
                .with_site(CallSite::new(Some(ticket), EntryPath::GatedOp)),
        );
        let output = log.finish();
        assert!(output.truncated);
        let [record] = output.records.as_slice() else {
            panic!("the verdict is kept: {output:?}");
        };
        assert!(record.allowed && record.payload_dropped);
        assert_eq!(record.context, serde_json::Value::Null);
        assert_ne!(record.context_digest, 0, "the digest survives");
        assert_eq!(
            data.tenant_limits.host_attached_bytes(),
            0,
            "the run's memory budget is not touched"
        );
    }

    #[test]
    fn caps_mark_long_strings_deep_nesting_and_wide_containers() {
        let long = serde_json::json!({ "a": "é".repeat(100) });
        let (capped, truncated) = cap_context(&long, 11);
        assert!(truncated);
        let text = capped["a"].as_str().unwrap();
        assert!(
            text.starts_with("éééé") && text.contains("truncated"),
            "{text}"
        );

        let mut nested = serde_json::json!(1);
        for _ in 0..20 {
            nested = serde_json::json!([nested]);
        }
        assert!(cap_context(&nested, 100).1);

        let wide = Value::Array((0..200).map(Value::from).collect());
        let (capped, truncated) = cap_context(&wide, 100);
        assert!(truncated);
        assert_eq!(capped.as_array().unwrap().len(), MAX_CONTEXT_ENTRIES);

        let small = serde_json::json!({ "host": "x", "n": [1, 2] });
        assert_eq!(cap_context(&small, 100), (small.clone(), false));
    }

    #[test]
    fn finish_refunds_the_recorders_budget() {
        let (data, log) = log_with(DecisionLogConfig {
            max_recorder_bytes: 4096,
            ..DecisionLogConfig::default()
        });
        let context = serde_json::json!({ "path": "p".repeat(300) });
        for _ in 0..2 {
            data.security_check.audit(AuditDecision::new(
                "main", "fs.read", &context, true, "policy", None, None,
            ));
            let output = log.finish();
            assert!(!output.truncated && output.records.len() == 1, "{output:?}");
        }
        assert_eq!(data.tenant_limits.host_attached_bytes(), 0);
    }

    #[test]
    fn long_keys_sharing_a_prefix_both_survive_capping() {
        let long = "k".repeat(100);
        let value = serde_json::json!({ format!("{long}a"): 1, format!("{long}b"): 2 });
        let mut truncated = false;
        let Value::Object(capped) = cap_value(&value, 16, 0, &mut truncated) else {
            panic!("expected an object");
        };
        assert!(truncated);
        assert_eq!(capped.len(), 2, "{capped:?}");
        let mut values: Vec<_> = capped.values().cloned().collect();
        values.sort_by_key(Value::as_i64);
        assert_eq!(values, vec![serde_json::json!(1), serde_json::json!(2)]);
    }

    #[test]
    fn near_miss_actual_values_and_keys_are_capped_like_the_context() {
        let (data, log) = log_with(DecisionLogConfig {
            max_context_value_bytes: 16,
            ..DecisionLogConfig::default()
        });
        let explanation = DecisionExplanation {
            action: DecisionAction::Deny,
            cause: DecisionCause::Default { caller_block: true },
            near_misses: vec![NearMissRecord {
                rule: RuleCitation {
                    caller: "main".into(),
                    index: 0,
                    name: None,
                },
                filter: "f".into(),
                failures: vec![FailureRecord {
                    comparison: "c".into(),
                    actual: Some(serde_json::json!({ "k".repeat(100): "v".repeat(100) })),
                    expected: None,
                    reason: FailureReasonRecord::NotSatisfied,
                    negated: false,
                }],
            }],
        };
        let context = serde_json::json!({});
        data.security_check.audit(
            AuditDecision::new("main", "fs.read", &context, false, "policy", None, None)
                .with_explanation(Some(&explanation)),
        );
        let output = log.finish();
        let [record] = output.records.as_slice() else {
            panic!("one record");
        };
        assert!(record.context_truncated);
        let actual = record.near_misses[0].failures[0].actual.as_ref().unwrap();
        assert!(
            serde_json::to_string(actual).unwrap().len() < 150,
            "{actual}"
        );
    }
}
