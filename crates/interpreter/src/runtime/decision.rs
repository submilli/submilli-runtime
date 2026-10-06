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
use super::call_log::{self, CallOutcome, CallRecord, ModelUsage, Payload, Side};
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
    /// A host function that can begin calls was entered. Returns the marker its
    /// [`exit_host_call`](Self::exit_host_call) passes back.
    fn enter_host_call(&self) -> u64;
    /// That host function returned (`returned`) or failed. Ends every call begun since
    /// `marker` that is still open; a nested host function has ended its own already.
    fn exit_host_call(&self, marker: u64, returned: bool);
    /// Whether a payload for the call `call_index` would be kept: the call is open and the
    /// payload budget is not spent. Callers check it before building a payload. The
    /// default asks for every payload.
    fn wants_payload(&self, _call_index: u64) -> bool {
        true
    }
    /// One side of a call that reached outside the program.
    fn call_payload(&self, call_index: u64, side: Side, payload: Payload<'_>);
    /// Token counts a model provider reported for a call.
    fn call_usage(&self, call_index: u64, usage: ModelUsage);
}

/// Sees records as they are made, for an embedder that streams a run while it executes.
///
/// Called on the thread running the program, after the recorder has released its lock.
/// It must not block: an implementation that cannot keep up drops what it cannot hold.
/// What it sees is also in the [`DecisionLogOutput`] the run ends with, unless the
/// recorder's own caps dropped it there.
pub trait RecordObserver: Send + Sync {
    fn call_started(&self, _call: &CallRecord) {}
    fn decision(&self, _record: &DecisionRecord) {}
    /// The call as it ended, without its payload contents: each side's meta is `Null`
    /// and it has no body and no masked header names, but it keeps its size, digest,
    /// truncation flag, timing, and usage.
    fn call_finished(&self, _call: &CallRecord) {}
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
    /// Calls kept per run; later ones are counted and mark the run truncated.
    pub max_calls: usize,
    /// Longest body copy a call's request or response keeps. Its digest and size always
    /// describe the full body.
    pub max_payload_bytes: usize,
}

impl Default for DecisionLogConfig {
    fn default() -> Self {
        Self {
            max_decisions: 10_000,
            max_context_value_bytes: 1024,
            max_recorder_bytes: 16 * 1024 * 1024,
            max_line_capture_frames: 1_000_000,
            max_calls: 10_000,
            max_payload_bytes: 1024 * 1024,
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
    /// The run's calls in the order they began.
    pub calls: Vec<CallRecord>,
    /// Calls not kept at all, past the cap or the byte budget.
    pub calls_dropped: u64,
}

/// Distinct (caller, capability) pairs tracked per run. Capability names come from
/// `security.check` calls, so the set is not bounded by the catalog.
const MAX_TRACKED_PAIRS: usize = 4096;
const PAIR_OVERHEAD_BYTES: u64 = 96;
const RECORD_BASE_BYTES: u64 = std::mem::size_of::<DecisionRecord>() as u64;
const CALL_BASE_BYTES: u64 = std::mem::size_of::<CallRecord>() as u64;
/// The share of the recorder's byte budget, as a divisor, kept for what must survive a
/// run that moves a lot of data. One rule: payload bodies and metas stop at the reserve;
/// the digest-only record of a payload whose copy was dropped, and decision records, may
/// use it.
const RECORD_RESERVE_DIVISOR: u64 = 4;
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
    calls: Vec<CallRecord>,
    /// Position in `calls` of each call still running, in call order.
    open_calls: Vec<(u64, usize)>,
    calls_dropped: u64,
}

/// The per-run recorder.
pub struct DecisionLog {
    state: Mutex<LogState>,
    config: DecisionLogConfig,
    started: Instant,
    clock: AtomicU64,
    observer: Option<Arc<dyn RecordObserver>>,
}

impl DecisionLog {
    /// Wraps the store's security check in a recording decorator. Returns the log to read
    /// results from.
    ///
    /// Install it outermost: after any embedder wrapper of the check (such as the server's
    /// audit decorator). Other wrappers do not forward [`SecurityCheck::recorder`], so a
    /// recorder installed beneath one is invisible to the host functions that begin calls.
    pub fn install(data: &mut StoreData, config: DecisionLogConfig) -> Arc<Self> {
        Self::install_observed(data, config, None)
    }

    /// [`install`](Self::install), with an observer that sees each record as it is made.
    pub fn install_observed(
        data: &mut StoreData,
        config: DecisionLogConfig,
        observer: Option<Arc<dyn RecordObserver>>,
    ) -> Arc<Self> {
        let log = Self::new(config, observer);
        data.security_check = log.wrap(data.security_check.clone());
        log
    }

    /// A log not yet attached to a run; [`wrap`](Self::wrap) a check to record through it.
    /// The clock starts now.
    pub fn new(config: DecisionLogConfig, observer: Option<Arc<dyn RecordObserver>>) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(LogState::default()),
            config,
            started: Instant::now(),
            clock: AtomicU64::new(0),
            observer,
        })
    }

    /// `inner`, with every decision it audits also recorded here. For a check made outside
    /// a program run, such as a server's file tool; a run uses [`install`](Self::install).
    pub fn wrap(self: &Arc<Self>, inner: Arc<dyn SecurityCheck>) -> Arc<dyn SecurityCheck> {
        Arc::new(RecordingCheck {
            inner,
            log: self.clone(),
        })
    }

    /// Takes the run's records and releases their byte charge. Call once, when the
    /// run ends: the truncation flags, drop count, and call numbering describe the
    /// whole run, so a later call would not describe only its own records.
    pub fn finish(&self) -> DecisionLogOutput {
        let mut state = self.lock();
        let records = std::mem::take(&mut state.records);
        for (_, position) in std::mem::take(&mut state.open_calls) {
            if let Some(call) = state.calls.get_mut(position) {
                call.outcome = Some(CallOutcome::Unfinished);
            }
        }
        let output = DecisionLogOutput {
            records,
            truncated: state.truncated,
            dropped: state.dropped,
            line_frames: state.line_frames,
            calls: std::mem::take(&mut state.calls),
            calls_dropped: state.calls_dropped,
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
        self.charge_within(state, bytes, self.config.max_recorder_bytes)
    }

    /// Charges `bytes` if the total stays within `limit`.
    fn charge_within(&self, state: &mut LogState, bytes: u64, limit: u64) -> bool {
        let next = state.charged.saturating_add(bytes);
        if next > limit {
            return false;
        }
        state.charged = next;
        true
    }

    /// The most the recorder's charge may reach for payload bodies and metas: the budget
    /// less the reserve. Digest-only records and decisions may spend the reserve.
    fn payload_limit(&self) -> u64 {
        let max = self.config.max_recorder_bytes;
        max.saturating_sub(max / RECORD_RESERVE_DIVISOR)
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
        if !self.charge(&mut state, minimal.saturating_add(payload)) {
            // Keep the digest and the verdict; drop what carries the bytes.
            record.context = Value::Null;
            record.near_misses = Vec::new();
            record.payload_dropped = true;
            record.context_truncated = true;
            state.truncated = true;
            if !self.charge(&mut state, minimal) {
                state.bytes_exhausted = true;
                state.dropped = state.dropped.saturating_add(1);
                return;
            }
        }
        let observed = self.observer.as_ref().map(|_| record.clone());
        state.records.push(record);
        drop(state);
        if let (Some(observer), Some(record)) = (&self.observer, observed) {
            observer.decision(&record);
        }
    }

    /// Opens the call `ticket` begins. Over the call cap or the byte budget it is
    /// counted and the run marked truncated.
    fn open_call(
        &self,
        state: &mut LogState,
        caller: &str,
        capability: &str,
        ticket: CallTicket,
    ) -> Option<CallRecord> {
        let cost =
            CALL_BASE_BYTES.saturating_add(caller.len().saturating_add(capability.len()) as u64);
        if state.calls.len() >= self.config.max_calls
            || state.calls.try_reserve(1).is_err()
            || state.open_calls.try_reserve(1).is_err()
            || !self.charge(state, cost)
        {
            state.truncated = true;
            state.calls_dropped = state.calls_dropped.saturating_add(1);
            return None;
        }
        let call = CallRecord {
            call_index: ticket.call_index,
            caller: caller.to_owned(),
            capability: capability.to_owned(),
            started_micros: ticket.at_micros,
            ended_micros: None,
            outcome: None,
            line: ticket.line,
            request: None,
            response: None,
            usage: None,
        };
        let observed = self.observer.as_ref().map(|_| call.clone());
        state
            .open_calls
            .push((ticket.call_index, state.calls.len()));
        state.calls.push(call);
        observed
    }

    fn call_mut(state: &mut LogState, call_index: u64) -> Option<&mut CallRecord> {
        let position = state
            .open_calls
            .iter()
            .rev()
            .find(|(index, _)| *index == call_index)
            .map(|(_, position)| *position)?;
        state.calls.get_mut(position)
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
        let ticket = self.begin(&mut state, caller, capability, line);
        let opened = self.open_call(&mut state, caller, capability, ticket);
        drop(state);
        if let (Some(observer), Some(call)) = (&self.observer, opened) {
            observer.call_started(&call);
        }
        ticket
    }

    fn enter_host_call(&self) -> u64 {
        self.lock().next_call_index
    }

    fn exit_host_call(&self, marker: u64, returned: bool) {
        let mut state = self.lock();
        let Some(first) = state
            .open_calls
            .iter()
            .position(|(index, _)| *index >= marker)
        else {
            return;
        };
        let ended = self.now_micros();
        let closing: Vec<_> = state.open_calls.drain(first..).collect();
        let mut finished = Vec::new();
        for (_, position) in closing {
            if let Some(call) = state.calls.get_mut(position) {
                call.ended_micros = Some(ended);
                call.outcome = Some(if returned {
                    CallOutcome::Returned
                } else {
                    CallOutcome::Failed
                });
                if self.observer.is_some() {
                    // Observers report sizes and timing; the bodies stay in the log.
                    finished.push(call.without_bodies());
                }
            }
        }
        drop(state);
        if let Some(observer) = &self.observer {
            for call in &finished {
                observer.call_finished(call);
            }
        }
    }

    fn wants_payload(&self, call_index: u64) -> bool {
        let mut state = self.lock();
        // Past the payload limit a record of it, at least its digest, still fits the
        // budget, so a payload is wanted until even that cannot be charged.
        Self::call_mut(&mut state, call_index).is_some()
            && state.charged.saturating_add(call_log::DIGEST_ONLY_COST)
                <= self.config.max_recorder_bytes
    }

    fn call_payload(&self, call_index: u64, side: Side, payload: Payload<'_>) {
        let room = {
            let mut state = self.lock();
            // A call that was not kept, or has ended, takes no payload: skip the hashing
            // and the copy.
            if Self::call_mut(&mut state, call_index).is_none() {
                return;
            }
            self.payload_limit().saturating_sub(state.charged)
        };
        let record = call_log::capture(
            &payload,
            self.config.max_context_value_bytes,
            room,
            self.config.max_payload_bytes,
        );
        let mut state = self.lock();
        // The lock was released while the payload was hashed, so the call may have ended.
        if Self::call_mut(&mut state, call_index).is_none() {
            return;
        }
        let cost = call_log::payload_cost(&record);
        let record = if self.charge_within(&mut state, cost, self.payload_limit()) {
            record
        } else {
            // Only the digest and the size enter the reserve.
            state.truncated = true;
            let mut kept = record.digest_only();
            kept.truncated = true;
            if !self.charge(&mut state, call_log::payload_cost(&kept)) {
                return;
            }
            kept
        };
        let Some(call) = Self::call_mut(&mut state, call_index) else {
            return;
        };
        match side {
            Side::Request => call.request = Some(Box::new(record)),
            Side::Response => call.response = Some(Box::new(record)),
        }
    }

    fn call_usage(&self, call_index: u64, usage: ModelUsage) {
        let mut state = self.lock();
        if let Some(call) = Self::call_mut(&mut state, call_index) {
            call.usage = Some(usage);
        }
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
pub(crate) fn cap_text(text: &str, max: usize, truncated: &mut bool) -> String {
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
    fn a_call_still_open_at_finish_is_unfinished() {
        let (data, log) = log_with(DecisionLogConfig::default());
        let recorder = data.security_check.recorder().unwrap();
        recorder.enter_host_call();
        recorder.begin_call("main", "http.get", None);
        let output = log.finish();
        let [call] = output.calls.as_slice() else {
            panic!("one call: {output:?}");
        };
        assert_eq!(call.outcome, Some(CallOutcome::Unfinished));
        assert_eq!(call.ended_micros, None);
    }

    #[test]
    fn payload_copies_leave_the_decision_reserve_free() {
        let max = 64 * 1024;
        let (data, log) = log_with(DecisionLogConfig {
            max_recorder_bytes: max,
            ..DecisionLogConfig::default()
        });
        let recorder = data.security_check.recorder().unwrap();
        let body = vec![b'x'; 20 * 1024];
        for _ in 0..6 {
            let ticket = recorder.begin_call("main", "http.get", None);
            recorder.call_payload(
                ticket.call_index,
                Side::Response,
                Payload::meta(serde_json::Value::Null).with_body(&body),
            );
        }
        assert!(
            log.lock().charged <= max - max / RECORD_RESERVE_DIVISOR + 6 * 256,
            "bodies stay out of the reserve; only digests may enter it"
        );
        let output = log.finish();
        let responses: Vec<_> = output
            .calls
            .iter()
            .filter_map(|call| call.response.as_deref())
            .collect();
        assert_eq!(responses.len(), 6, "every payload keeps a record");
        assert!(responses.iter().any(|payload| payload.body.is_some()));
        let cut = responses
            .iter()
            .find(|payload| payload.body.is_none())
            .expect("a payload past the reserve keeps no body");
        assert!(cut.truncated && cut.bytes == body.len() as u64 && !cut.digest.is_empty());
        assert!(output.truncated);
    }

    #[test]
    fn digest_only_records_do_not_spend_the_reserve_on_masked_names() {
        let max = 64 * 1024;
        let (data, log) = log_with(DecisionLogConfig {
            max_recorder_bytes: max,
            ..DecisionLogConfig::default()
        });
        let recorder = data.security_check.recorder().unwrap();
        let body = vec![b'x'; 20 * 1024];
        for _ in 0..6 {
            let ticket = recorder.begin_call("main", "http.get", None);
            recorder.call_payload(
                ticket.call_index,
                Side::Response,
                Payload::meta(serde_json::Value::Null).with_body(&body),
            );
        }
        let before = log.lock().charged;
        let names: Vec<String> = (0..64)
            .map(|i| format!("x-token-{i}-{}", "n".repeat(900)))
            .collect();
        let ticket = recorder.begin_call("main", "http.get", None);
        recorder.call_payload(
            ticket.call_index,
            Side::Response,
            Payload::meta(serde_json::Value::Null)
                .with_body(&body)
                .with_masked(names),
        );
        let spent = log.lock().charged - before;
        assert!(
            spent < 512,
            "a payload past the limit enters the reserve as a digest and a size: {spent}"
        );
        audit_denied(&data, recorder.begin_call("main", "fs.read", None));
        let output = log.finish();
        assert_eq!(output.records.len(), 1, "a later denial is still recorded");
        let last = output
            .calls
            .iter()
            .rev()
            .find_map(|call| call.response.as_deref());
        assert!(last.is_some_and(|payload| payload.masked_headers.is_empty()));
    }

    #[test]
    fn a_body_is_sized_to_the_room_its_record_leaves() {
        let max = 16 * 1024;
        let (data, log) = log_with(DecisionLogConfig {
            max_recorder_bytes: max,
            ..DecisionLogConfig::default()
        });
        let recorder = data.security_check.recorder().unwrap();
        let body = vec![b'x'; 64 * 1024];
        let meta = serde_json::json!({ "url": "https://x.test/" });
        let ticket = recorder.begin_call("main", "http.get", None);
        recorder.call_payload(
            ticket.call_index,
            Side::Response,
            Payload::meta(meta.clone()).with_body(&body),
        );
        let charged = log.lock().charged;
        assert!(
            charged <= log.payload_limit(),
            "the copy fits what it is charged"
        );
        let output = log.finish();
        let payload = output.calls[0]
            .response
            .as_deref()
            .expect("a response record");
        let kept = match &payload.body {
            Some(call_log::BodyCopy::Text(text)) => text.len(),
            other => panic!("a cut text body is kept as text: {other:?}"),
        };
        assert!(payload.truncated && kept > 0 && kept < body.len());
        assert_eq!(payload.bytes, body.len() as u64);
        // Binary bodies are charged as base64: a quarter more.
        let (data, log) = log_with(DecisionLogConfig {
            max_recorder_bytes: max,
            ..DecisionLogConfig::default()
        });
        let recorder = data.security_check.recorder().unwrap();
        let binary = vec![0xffu8; 64 * 1024];
        let ticket = recorder.begin_call("main", "http.get", None);
        recorder.call_payload(
            ticket.call_index,
            Side::Response,
            Payload::meta(meta).with_body(&binary),
        );
        let output = log.finish();
        let payload = output.calls[0]
            .response
            .as_deref()
            .expect("a response record");
        let Some(call_log::BodyCopy::Base64(text)) = &payload.body else {
            panic!("a binary body is kept as base64: {payload:?}");
        };
        assert!(payload.truncated);
        assert!(text.len() as u64 <= max - max / RECORD_RESERVE_DIVISOR);
    }

    #[test]
    fn a_payload_is_wanted_exactly_while_its_digest_only_record_fits() {
        let max = 4096;
        let (data, log) = log_with(DecisionLogConfig {
            max_recorder_bytes: max,
            ..DecisionLogConfig::default()
        });
        let recorder = data.security_check.recorder().unwrap();
        recorder.enter_host_call();
        let ticket = recorder.begin_call("main", "http.get", None);
        log.lock().charged = max - call_log::DIGEST_ONLY_COST;
        assert!(recorder.wants_payload(ticket.call_index));
        log.lock().charged = max - call_log::DIGEST_ONLY_COST + 1;
        assert!(!recorder.wants_payload(ticket.call_index));
    }

    #[test]
    fn a_body_with_no_room_is_not_copied() {
        let (data, log) = log_with(DecisionLogConfig {
            max_recorder_bytes: 1024,
            ..DecisionLogConfig::default()
        });
        let recorder = data.security_check.recorder().unwrap();
        let ticket = recorder.begin_call("main", "http.get", None);
        let meta = serde_json::json!({ "url": "u".repeat(400) });
        recorder.call_payload(
            ticket.call_index,
            Side::Response,
            Payload::meta(meta).with_body(&[b'x'; 4096]),
        );
        let output = log.finish();
        let payload = output.calls[0]
            .response
            .as_deref()
            .expect("a response record");
        assert!(payload.body.is_none() && payload.truncated);
        assert_eq!(payload.bytes, 4096);
    }

    #[test]
    fn a_payload_is_wanted_while_its_call_is_open_and_the_budget_lasts() {
        let (data, _log) = log_with(DecisionLogConfig {
            max_recorder_bytes: 4096,
            ..DecisionLogConfig::default()
        });
        let recorder = data.security_check.recorder().unwrap();
        recorder.enter_host_call();
        let ticket = recorder.begin_call("main", "http.get", None);
        assert!(recorder.wants_payload(ticket.call_index));
        assert!(
            !recorder.wants_payload(ticket.call_index + 1),
            "no such call"
        );
        recorder.exit_host_call(0, true);
        assert!(!recorder.wants_payload(ticket.call_index), "the call ended");
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
            // less than this record's strings need. No call records take a share.
            max_recorder_bytes: PAIR_OVERHEAD_BYTES
                + "main".len() as u64
                + "fs.read".len() as u64
                + RECORD_BASE_BYTES
                + 2,
            max_calls: 0,
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
