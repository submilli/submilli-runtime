//! Decision records: what an embedder's recorder sees of each capability decision.
//!
//! The recorder is a [`SecurityCheck`] decorator ([`DecisionLog::install`]): it forwards
//! every call to the check it wraps, including `audit`, so the embedder's own audit sees
//! exactly what it would without a recorder, and taps the same `audit` calls. Recording
//! never changes a decision, never fails a call, and never charges guest fuel.
//!
//! Record buffers are charged against the store's host-memory cap ([`HostBudget`]) and a
//! per-run decision cap; a run that reaches either is marked truncated rather than refused.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use serde::Serialize;
use serde_json::Value;

use super::StoreData;
use super::limits::HostBudget;
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
    /// Another decision of the host call already begun for this pair, such as a read-only
    /// or quota refusal after the policy allowed it; begins a call if none is known.
    fn continue_call(&self, caller: &str, capability: &str, line: Option<SourceLine>)
    -> CallTicket;
    /// Marks the latest call's record as a denial the host function treated as a filter.
    fn mark_last_filtered(&self);
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
    /// The context and near misses were dropped to stay within the host-memory cap.
    pub payload_dropped: bool,
}

#[derive(Debug, Clone)]
pub struct DecisionLogConfig {
    /// Decisions kept per run; later ones are counted and mark the run truncated.
    pub max_decisions: usize,
    /// Longest string kept in a recorded context; longer ones end in a marker.
    pub max_context_value_bytes: usize,
}

impl Default for DecisionLogConfig {
    fn default() -> Self {
        Self {
            max_decisions: 10_000,
            max_context_value_bytes: 1024,
        }
    }
}

/// What a run recorded.
#[derive(Debug, Clone, Default)]
pub struct DecisionLogOutput {
    pub records: Vec<DecisionRecord>,
    /// Some decision was not kept in full: past the cap, over the memory charge, or
    /// beyond the pairs the recorder tracks.
    pub truncated: bool,
    /// Decisions not kept at all.
    pub dropped: u64,
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
}

/// The per-run recorder.
pub struct DecisionLog {
    state: Mutex<LogState>,
    budget: HostBudget,
    config: DecisionLogConfig,
    started: Instant,
    clock: AtomicU64,
}

impl DecisionLog {
    /// Wraps the store's security check in a recording decorator, charging the recorder's
    /// buffers to the store's host-memory cap. Returns the log to read results from.
    pub fn install(data: &mut StoreData, config: DecisionLogConfig) -> std::sync::Arc<Self> {
        let log = std::sync::Arc::new(Self {
            state: Mutex::new(LogState::default()),
            budget: data.tenant_limits.host_budget(),
            config,
            started: Instant::now(),
            clock: AtomicU64::new(0),
        });
        data.security_check = std::sync::Arc::new(RecordingCheck {
            inner: data.security_check.clone(),
            log: log.clone(),
        });
        log
    }

    /// Takes the records, releasing their memory charge. The log keeps recording after.
    pub fn finish(&self) -> DecisionLogOutput {
        let mut state = self.lock();
        let records = std::mem::take(&mut state.records);
        let output = DecisionLogOutput {
            records,
            truncated: state.truncated,
            dropped: state.dropped,
        };
        self.budget.release(state.charged);
        state.charged = 0;
        state.pairs.clear();
        output
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, LogState> {
        // A poisoned lock means a recorder call panicked; the data is still sound to read.
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
        if self.budget.charge(bytes).is_err() {
            return false;
        }
        state.charged = state.charged.saturating_add(bytes);
        true
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
            None => self.continue_locked(&mut state, decision.caller, decision.capability, None),
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
            state.dropped = state.dropped.saturating_add(1);
        }
    }

    fn continue_locked(
        &self,
        state: &mut LogState,
        caller: &str,
        capability: &str,
        line: Option<SourceLine>,
    ) -> CallTicket {
        let known = state
            .pairs
            .get(&(caller.to_owned(), capability.to_owned()))
            .and_then(|pair| pair.last);
        match known {
            Some(ticket) => {
                state.last_call_index = Some(ticket.call_index);
                ticket
            }
            None => self.begin(state, caller, capability, line),
        }
    }

    fn build(&self, decision: &AuditDecision<'_>, ticket: CallTicket) -> DecisionRecord {
        let digest = digest_of(decision.context);
        let (context, context_truncated) = if decision.capability == "secrets.get" {
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
                explanation.near_misses.clone(),
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
}

impl Drop for DecisionLog {
    fn drop(&mut self) {
        let charged = self.state.get_mut().map_or_else(
            |poisoned| poisoned.into_inner().charged,
            |state| state.charged,
        );
        self.budget.release(charged);
    }
}

impl DecisionRecorder for DecisionLog {
    fn begin_call(&self, caller: &str, capability: &str, line: Option<SourceLine>) -> CallTicket {
        let mut state = self.lock();
        self.begin(&mut state, caller, capability, line)
    }

    fn continue_call(
        &self,
        caller: &str,
        capability: &str,
        line: Option<SourceLine>,
    ) -> CallTicket {
        let mut state = self.lock();
        self.continue_locked(&mut state, caller, capability, line)
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
        Value::String(text) if text.len() > max_string => {
            *truncated = true;
            let mut end = max_string;
            while end > 0 && !text.is_char_boundary(end) {
                end -= 1;
            }
            let kept = text.get(..end).unwrap_or_default();
            Value::String(format!(
                "{kept}…[truncated, {} of {} bytes kept]",
                kept.len(),
                text.len()
            ))
        }
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
                    .map(|(key, item)| {
                        (
                            key.clone(),
                            cap_value(item, max_string, depth + 1, truncated),
                        )
                    })
                    .collect(),
            )
        }
        other => other.clone(),
    }
}

struct RecordingCheck {
    inner: std::sync::Arc<dyn SecurityCheck>,
    log: std::sync::Arc<DecisionLog>,
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

    fn log_with(cap: u64, config: DecisionLogConfig) -> (StoreData, std::sync::Arc<DecisionLog>) {
        let mut data = StoreData::with_vfs_and_cap(Vfs::none(), cap);
        let log = DecisionLog::install(&mut data, config);
        (data, log)
    }

    #[test]
    fn a_secret_read_keeps_the_key_name_and_nothing_else() {
        let (data, log) = log_with(1 << 20, DecisionLogConfig::default());
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
    fn an_exhausted_memory_budget_keeps_the_verdict_and_drops_the_payload() {
        // Room for the pair bookkeeping and a bare record, not for a big context.
        let (data, log) = log_with(2048, DecisionLogConfig::default());
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
        assert_eq!(data.tenant_limits.host_attached_bytes(), 0, "refunded");
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
    fn the_log_releases_what_it_charged_when_dropped() {
        let (data, log) = log_with(1 << 20, DecisionLogConfig::default());
        let context = serde_json::Value::Null;
        data.security_check.audit(AuditDecision::new(
            "main", "x", &context, true, "policy", None, None,
        ));
        assert!(data.tenant_limits.host_attached_bytes() > 0);
        drop(log);
        let mut data = data;
        // The decorator held the last reference to the log.
        data.security_check = crate::runtime::security::default_check();
        assert_eq!(data.tenant_limits.host_attached_bytes(), 0);
    }
}
