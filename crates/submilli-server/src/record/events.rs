//! Session events: what each session's runs and tool calls do, streamed while they happen.
//!
//! A [`RunRecorderFactory`] that [wants events](RunRecorderFactory::wants_events) receives
//! them through [`RunRecorderFactory::event`], one at a time, on a task of their own, in
//! the order the server took them. Every event carries a server-wide sequence number; a
//! gap in it means events were dropped.
//!
//! Delivery is off the run's critical path. Runs and tool calls enqueue into a bounded
//! buffer and never wait for the embedder. Past the buffer's count or byte budget an event
//! is dropped and counted, never blocking or failing the run. The buffer keeps a reserve
//! only decisions, run starts and ends, and tool calls may use, so an overflow drops
//! progress events (a call starting or finishing) first; a run's end reports how many of
//! its events were dropped, and its [`FinishedRun`](super::FinishedRun) still has every
//! decision and call the recorder kept.
//!
//! The buffer has its own budget, separate from the run's memory limit, so that streaming
//! a run never changes how much memory it may use (the same choice the decision recorder
//! makes).
//!
//! Events exist only for runs whose [`RunRecorderFactory::start`] returned a recorder.
//! A failure before a run starts (an unknown blueprint, invalid variables) has no run to
//! stream.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard};

use interpreter::runtime::{
    CallOutcome, CallRecord, DecisionRecord, ModelUsage, RecordObserver, SourceLine,
};
use serde::Serialize;
use uuid::Uuid;

use super::{RunEntry, RunRecorderFactory, RunStart};
use crate::error::ErrorKind;

/// The version of [`SessionEvent`]'s shape.
pub const EVENT_SCHEMA: u32 = 1;

/// One thing that happened in a session.
#[derive(Debug, Clone, Serialize)]
pub struct SessionEvent {
    pub schema: u32,
    pub event_id: String,
    /// Server-wide, from 1, in the order the server took events. A gap means events were
    /// dropped.
    pub seq: u64,
    /// Microseconds since the Unix epoch when the server took the event.
    pub at_micros: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// The run's `execution_id`; absent for a tool call outside a run.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    /// The MCP tool call this belongs to, when its client named it (Claude Code sends
    /// `claudecode/toolUseId` in the request's `_meta`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(flatten)]
    pub kind: EventKind,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "kebab-case", tag = "kind")]
pub enum EventKind {
    RunStarted {
        label: String,
        entry: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        client: Option<String>,
        blueprint: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        blueprint_hash: Option<String>,
        /// SHA-256 of the program, hex; absent for a file tool.
        #[serde(skip_serializing_if = "Option::is_none")]
        code_hash: Option<String>,
    },
    CallStarted {
        call_index: u64,
        caller: String,
        capability: String,
        /// Microseconds since the run's recorder started.
        started_micros: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        line: Option<SourceLine>,
    },
    Decision {
        record: Box<DecisionRecord>,
    },
    CallFinished {
        call_index: u64,
        capability: String,
        started_micros: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        ended_micros: Option<u64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        outcome: Option<CallOutcome>,
        /// The full size of what the call sent, counted before any cap.
        #[serde(skip_serializing_if = "Option::is_none")]
        sent_bytes: Option<u64>,
        /// The full size of what came back, counted before any cap.
        #[serde(skip_serializing_if = "Option::is_none")]
        result_bytes: Option<u64>,
        /// Token counts the model provider reported.
        #[serde(skip_serializing_if = "Option::is_none")]
        usage: Option<ModelUsage>,
    },
    RunFinished {
        dispatched: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<ErrorKind>,
        wall_ms: u64,
        console_bytes: u64,
        /// This run's events the buffer dropped.
        events_dropped: u64,
    },
    /// What the run's caller received, as JSON.
    Returned {
        bytes: u64,
    },
    /// An MCP tool call on a session: documentation, package discovery, execute, files.
    ToolCall {
        tool: String,
        ok: bool,
        result_bytes: u64,
        wall_ms: u64,
    },
}

impl EventKind {
    /// Whether the event may use the buffer's reserve.
    fn reserved(&self) -> bool {
        !matches!(self, Self::CallStarted { .. } | Self::CallFinished { .. })
    }
}

/// What the buffer holds, and how much of it only reserved events may use.
#[derive(Debug, Clone, Copy)]
struct Limits {
    events: usize,
    reserved_events: usize,
    bytes: usize,
    reserved_bytes: usize,
}

const LIMITS: Limits = Limits {
    events: 4096,
    reserved_events: 512,
    bytes: 16 * 1024 * 1024,
    reserved_bytes: 2 * 1024 * 1024,
};

#[derive(Default)]
struct Buffer {
    queue: VecDeque<(SessionEvent, usize)>,
    bytes: usize,
    next_seq: u64,
    /// Events are queued that no delivery pass has taken yet, or one is taking them.
    delivery_pending: bool,
    /// The delivery task is running.
    delivery_task_running: bool,
    /// Events dropped per run still in progress. A run is listed from its start until its
    /// end reads the count, so drops for a run that is not listed are not counted.
    dropped: HashMap<String, u64>,
}

/// Buffers events and delivers them to the factory on a task of their own.
pub(crate) struct EventHub {
    factory: Arc<dyn RunRecorderFactory>,
    buffer: Mutex<Buffer>,
    limits: Limits,
    /// Wakes the delivery task.
    wake: Arc<tokio::sync::Notify>,
    /// Event ids are this, then the sequence number: unique without a random draw each.
    id_prefix: String,
}

impl EventHub {
    pub(crate) fn new(factory: Arc<dyn RunRecorderFactory>) -> Arc<Self> {
        Self::with_limits(factory, LIMITS)
    }

    fn with_limits(factory: Arc<dyn RunRecorderFactory>, limits: Limits) -> Arc<Self> {
        Arc::new(Self {
            factory,
            buffer: Mutex::new(Buffer::default()),
            limits,
            wake: Arc::new(tokio::sync::Notify::new()),
            id_prefix: Uuid::new_v4().to_string(),
        })
    }

    fn lock(&self) -> MutexGuard<'_, Buffer> {
        // Recovering is acceptable only because the buffer is observation-only: it never
        // feeds a decision or a response.
        self.buffer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Takes an event. Never blocks on the embedder.
    pub(crate) fn push(
        self: &Arc<Self>,
        session_id: Option<&str>,
        run_id: Option<&str>,
        tool_call_id: Option<&str>,
        kind: EventKind,
    ) {
        let size = event_size(&[session_id, run_id, tool_call_id], &kind);
        let mut buffer = self.lock();
        let Some(seq) = self.admit(&mut buffer, run_id, kind.reserved(), size) else {
            return;
        };
        let event = SessionEvent {
            schema: EVENT_SCHEMA,
            event_id: format!("{}-{seq}", self.id_prefix),
            seq,
            at_micros: now_micros(),
            session_id: session_id.map(str::to_owned),
            run_id: run_id.map(str::to_owned),
            tool_call_id: tool_call_id.map(str::to_owned),
            kind,
        };
        buffer.bytes = buffer.bytes.saturating_add(size);
        buffer.queue.push_back((event, size));
        self.schedule_delivery(buffer);
    }

    /// Numbers an event and makes room for it. `None` when it does not fit: it is dropped,
    /// counted against its run, and its number is skipped so a gap shows it.
    fn admit(
        &self,
        buffer: &mut Buffer,
        run_id: Option<&str>,
        reserved: bool,
        size: usize,
    ) -> Option<u64> {
        buffer.next_seq = buffer.next_seq.saturating_add(1);
        let seq = buffer.next_seq;
        let limits = self.limits;
        let (events, bytes) = if reserved {
            (limits.events, limits.bytes)
        } else {
            (
                limits.events.saturating_sub(limits.reserved_events),
                limits.bytes.saturating_sub(limits.reserved_bytes),
            )
        };
        let fits = buffer.queue.len() < events
            && buffer.bytes.saturating_add(size) <= bytes
            && buffer.queue.try_reserve(1).is_ok();
        if fits {
            return Some(seq);
        }
        // Only a run still in progress is counted, so a late event of a run that has
        // ended cannot leave an entry behind.
        if let Some(count) = run_id.and_then(|run_id| buffer.dropped.get_mut(run_id)) {
            *count = count.saturating_add(1);
        }
        None
    }

    /// Makes sure something delivers the queue: the running task, a new one, or this
    /// thread when there is no runtime to hand it to.
    fn schedule_delivery(self: &Arc<Self>, mut buffer: MutexGuard<'_, Buffer>) {
        if buffer.delivery_pending {
            return;
        }
        buffer.delivery_pending = true;
        if buffer.delivery_task_running {
            drop(buffer);
            self.wake.notify_one();
            return;
        }
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            drop(buffer);
            self.drain();
            return;
        };
        buffer.delivery_task_running = true;
        drop(buffer);
        // One long-lived task delivers everything; it ends with the hub, which wakes it
        // as it drops.
        let hub = Arc::downgrade(self);
        let wake = self.wake.clone();
        runtime.spawn(async move {
            loop {
                let Some(hub) = hub.upgrade() else {
                    return;
                };
                hub.drain();
                drop(hub);
                wake.notified().await;
            }
        });
    }

    /// Starts counting the events dropped for `run_id`.
    fn register_run(&self, run_id: &str) {
        self.lock().dropped.insert(run_id.to_owned(), 0);
    }

    #[cfg(test)]
    pub(super) fn tracked_runs(&self) -> usize {
        self.lock().dropped.len()
    }

    /// The events dropped for `run_id`, forgetting the run.
    fn take_dropped(&self, run_id: &str) -> u64 {
        self.lock().dropped.remove(run_id).unwrap_or(0)
    }

    fn drain(&self) {
        loop {
            let next = {
                let mut buffer = self.lock();
                let Some((event, size)) = buffer.queue.pop_front() else {
                    buffer.delivery_pending = false;
                    return;
                };
                buffer.bytes = buffer.bytes.saturating_sub(size);
                event
            };
            // An embedder that panics on one event must not end delivery of the rest.
            let delivered = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                self.factory.event(next);
            }));
            if delivered.is_err() {
                tracing::warn!("the run recorder panicked while handling a session event");
            }
        }
    }
}

impl Drop for EventHub {
    fn drop(&mut self) {
        // Wake the parked delivery task so it sees the hub is gone and ends.
        self.wake.notify_one();
    }
}

/// Bytes an event holds, for the buffer's budget: a flat amount for the event itself, the
/// owned strings it and its ids carry, and a decision's capped record, which dominates.
fn event_size(ids: &[Option<&str>], kind: &EventKind) -> usize {
    const BASE: usize = 512;
    let ids = ids.iter().flatten().map(|id| id.len());
    saturating_sum(ids.chain([BASE, kind_size(kind)]))
}

/// The owned strings, and for a decision the capped record, that `kind` holds.
fn kind_size(kind: &EventKind) -> usize {
    let text = |text: &Option<String>| text.as_ref().map_or(0, String::len);
    match kind {
        EventKind::RunStarted {
            label,
            entry,
            client,
            blueprint,
            blueprint_hash,
            code_hash,
        } => saturating_sum([
            label.len(),
            entry.len(),
            text(client),
            blueprint.len(),
            text(blueprint_hash),
            text(code_hash),
        ]),
        EventKind::CallStarted {
            caller, capability, ..
        } => saturating_sum([caller.len(), capability.len()]),
        EventKind::CallFinished { capability, .. } => capability.len(),
        EventKind::ToolCall { tool, .. } => tool.len(),
        EventKind::Decision { record } => saturating_sum(
            [
                record.caller.len(),
                record.capability.len(),
                record.source.len(),
                text(&record.reason),
                value_size(&record.context),
            ]
            .into_iter()
            .chain(record.near_misses.iter().map(near_miss_size)),
        ),
        EventKind::RunFinished { .. } | EventKind::Returned { .. } => 0,
    }
}

fn near_miss_size(miss: &interpreter::runtime::NearMissRecord) -> usize {
    saturating_sum(
        [
            miss.rule.caller.len(),
            miss.rule.name.as_ref().map_or(0, String::len),
            miss.filter.len(),
        ]
        .into_iter()
        .chain(miss.failures.iter().map(failure_size)),
    )
}

fn failure_size(failure: &interpreter::runtime::FailureRecord) -> usize {
    saturating_sum([
        failure.comparison.len(),
        failure.expected.as_ref().map_or(0, String::len),
        failure.actual.as_ref().map_or(0, value_size),
    ])
}

fn saturating_sum(sizes: impl IntoIterator<Item = usize>) -> usize {
    sizes.into_iter().fold(0, usize::saturating_add)
}

/// Roughly what a JSON value holds: its strings and keys, and a word per node.
fn value_size(value: &serde_json::Value) -> usize {
    use serde_json::Value;
    const NODE: usize = 8;
    match value {
        Value::String(text) => text.len().saturating_add(NODE),
        Value::Array(items) => saturating_sum(items.iter().map(value_size).chain([NODE])),
        Value::Object(fields) => saturating_sum(
            fields
                .iter()
                .map(|(key, item)| key.len().saturating_add(value_size(item)))
                .chain([NODE]),
        ),
        _ => NODE,
    }
}

fn now_micros() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| {
            u64::try_from(elapsed.as_micros()).unwrap_or(u64::MAX)
        })
}

/// One run's events: the observer its decision log reports to.
pub(crate) struct RunEvents {
    hub: Arc<EventHub>,
    run_id: String,
    session_id: Option<String>,
    tool_call_id: Option<String>,
}

impl RunEvents {
    pub(crate) fn start(hub: &Arc<EventHub>, run: &RunStart) -> Arc<Self> {
        hub.register_run(&run.execution_id);
        let events = Arc::new(Self {
            hub: hub.clone(),
            run_id: run.execution_id.clone(),
            session_id: run.session_id.clone(),
            tool_call_id: run.tool_call_id.clone(),
        });
        events.push(EventKind::RunStarted {
            label: run.label.clone(),
            entry: entry_name(&run.entry),
            client: run.client.clone(),
            blueprint: run.blueprint_name.clone(),
            blueprint_hash: run.blueprint_hash.clone(),
            code_hash: run
                .code
                .as_deref()
                .map(|code| crate::audit::hash(code.as_bytes())),
        });
        events
    }

    fn push(&self, kind: EventKind) {
        self.hub.push(
            self.session_id.as_deref(),
            Some(&self.run_id),
            self.tool_call_id.as_deref(),
            kind,
        );
    }

    pub(crate) fn finished(
        &self,
        dispatched: bool,
        error: Option<ErrorKind>,
        wall: std::time::Duration,
        console_bytes: u64,
    ) {
        let events_dropped = self.hub.take_dropped(&self.run_id);
        self.push(EventKind::RunFinished {
            dispatched,
            error,
            wall_ms: u64::try_from(wall.as_millis()).unwrap_or(u64::MAX),
            console_bytes,
            events_dropped,
        });
    }

    pub(crate) fn returned(&self, bytes: u64) {
        self.push(EventKind::Returned { bytes });
    }
}

fn entry_name(entry: &RunEntry) -> String {
    match entry {
        RunEntry::Http => "http".into(),
        RunEntry::Session => "session".into(),
        RunEntry::Mcp => "mcp".into(),
        RunEntry::McpFileTool { tool } => format!("mcp:{tool}"),
        RunEntry::Program => "program".into(),
        RunEntry::Test => "test".into(),
    }
}

impl RecordObserver for RunEvents {
    fn call_started(&self, call: &CallRecord) {
        self.push(EventKind::CallStarted {
            call_index: call.call_index,
            caller: call.caller.clone(),
            capability: call.capability.clone(),
            started_micros: call.started_micros,
            line: call.line,
        });
    }

    fn decision(&self, record: &DecisionRecord) {
        self.push(EventKind::Decision {
            record: Box::new(record.clone()),
        });
    }

    fn call_finished(&self, call: &CallRecord) {
        self.push(EventKind::CallFinished {
            call_index: call.call_index,
            capability: call.capability.clone(),
            started_micros: call.started_micros,
            ended_micros: call.ended_micros,
            outcome: call.outcome,
            sent_bytes: call.request.as_ref().map(|request| request.bytes),
            result_bytes: call.response.as_ref().map(|response| response.bytes),
            usage: call.usage,
        });
    }
}

/// Records an MCP tool call on a session.
pub(crate) fn tool_call(
    hub: &Arc<EventHub>,
    session_id: Option<&str>,
    tool_call_id: Option<&str>,
    tool: &str,
    ok: bool,
    result_bytes: u64,
    wall: std::time::Duration,
) {
    hub.push(
        session_id,
        None,
        tool_call_id,
        EventKind::ToolCall {
            tool: tool.to_owned(),
            ok,
            result_bytes,
            wall_ms: u64::try_from(wall.as_millis()).unwrap_or(u64::MAX),
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Collect(Mutex<Vec<SessionEvent>>);

    impl RunRecorderFactory for Collect {
        fn start(&self, _run: RunStart) -> Option<Arc<dyn super::super::RunRecorder>> {
            None
        }
        fn wants_events(&self) -> bool {
            true
        }
        fn event(&self, event: SessionEvent) {
            self.0.lock().unwrap().push(event);
        }
    }

    fn progress(index: u64) -> EventKind {
        EventKind::CallStarted {
            call_index: index,
            caller: "main".into(),
            capability: "http.get".into(),
            started_micros: 0,
            line: None,
        }
    }

    #[test]
    fn an_overflow_drops_progress_first_and_counts_the_runs_losses() {
        let collect = Arc::new(Collect::default());
        // Room for four events, one of them reserved.
        let hub = EventHub::with_limits(
            collect.clone(),
            Limits {
                events: 4,
                reserved_events: 1,
                bytes: usize::MAX,
                reserved_bytes: 0,
            },
        );
        hub.register_run("r");
        {
            // Hold delivery back so the buffer fills.
            let mut buffer = hub.lock();
            buffer.delivery_pending = true;
        }
        for index in 0..5 {
            hub.push(Some("s"), Some("r"), None, progress(index));
        }
        hub.push(Some("s"), Some("r"), None, EventKind::Returned { bytes: 1 });
        hub.push(Some("s"), Some("r"), None, EventKind::Returned { bytes: 2 });
        assert_eq!(
            hub.take_dropped("r"),
            3,
            "two progress events and one reserved"
        );
        hub.drain();
        let events = collect.0.lock().unwrap();
        let seqs: Vec<_> = events.iter().map(|event| event.seq).collect();
        assert_eq!(seqs, [1, 2, 3, 6], "a gap marks what was dropped");
        assert!(matches!(events[3].kind, EventKind::Returned { bytes: 1 }));
    }

    #[test]
    fn an_event_is_sized_by_the_strings_it_carries() {
        let small = event_size(&[None], &progress(0));
        let long = "x".repeat(10_000);
        let big = event_size(
            &[Some(&long), Some("run"), None],
            &EventKind::CallStarted {
                call_index: 0,
                caller: long.clone(),
                capability: long.clone(),
                started_micros: 0,
                line: None,
            },
        );
        assert!(big >= small + 29_000, "{small} vs {big}");
        let tool = EventKind::ToolCall {
            tool: long.clone(),
            ok: true,
            result_bytes: 0,
            wall_ms: 0,
        };
        assert!(event_size(&[None], &tool) >= long.len());
        assert_eq!(
            event_size(
                &[Some(&long)],
                &EventKind::Returned {
                    bytes: usize::MAX as u64
                }
            ),
            event_size(&[None], &EventKind::Returned { bytes: 0 }) + long.len()
        );
    }

    #[test]
    fn a_finished_runs_late_drops_leave_nothing_behind() {
        let collect = Arc::new(Collect::default());
        let hub = EventHub::with_limits(
            collect,
            Limits {
                events: 0,
                reserved_events: 0,
                bytes: 0,
                reserved_bytes: 0,
            },
        );
        hub.register_run("r");
        hub.push(None, Some("r"), None, progress(0));
        assert_eq!(hub.take_dropped("r"), 1);
        // Events that arrive after the run's end, and the end's own, are all dropped.
        hub.push(None, Some("r"), None, EventKind::Returned { bytes: 1 });
        hub.push(None, Some("r"), None, progress(1));
        hub.push(None, Some("never-started"), None, progress(2));
        assert!(hub.lock().dropped.is_empty());
    }

    #[test]
    fn a_panicking_embedder_does_not_stop_delivery() {
        struct Flaky(Mutex<Vec<u64>>);
        impl RunRecorderFactory for Flaky {
            fn start(&self, _run: RunStart) -> Option<Arc<dyn super::super::RunRecorder>> {
                None
            }
            fn wants_events(&self) -> bool {
                true
            }
            fn event(&self, event: SessionEvent) {
                assert!(event.seq != 1, "first event refused");
                self.0.lock().unwrap().push(event.seq);
            }
        }
        let flaky = Arc::new(Flaky(Mutex::default()));
        let hub = EventHub::new(flaky.clone());
        {
            let mut buffer = hub.lock();
            buffer.delivery_pending = true;
        }
        for index in 0..3 {
            hub.push(None, None, None, progress(index));
        }
        hub.drain();
        assert_eq!(*flaky.0.lock().unwrap(), [2, 3]);
    }

    #[test]
    fn events_serialize_with_their_schema_and_without_unknown_fields() {
        let collect = Arc::new(Collect::default());
        let hub = EventHub::new(collect.clone());
        hub.push(None, Some("run"), None, EventKind::Returned { bytes: 7 });
        let events = collect.0.lock().unwrap();
        let value = serde_json::to_value(&events[0]).unwrap();
        assert_eq!(value["schema"], EVENT_SCHEMA);
        assert_eq!(value["kind"], "returned");
        assert_eq!(value["run_id"], "run");
        assert!(
            value.get("session_id").is_none(),
            "absent, not null: {value}"
        );
        assert!(value.get("tool_call_id").is_none());
    }
}
