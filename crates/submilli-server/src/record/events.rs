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
//! a run never changes how much memory it may use (the plan's R19; the same choice the
//! decision recorder makes).

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

/// Events the buffer holds at most, and how many of those only reserved events may use.
const MAX_EVENTS: usize = 4096;
const RESERVED_EVENTS: usize = 512;
/// Bytes the buffer holds at most, and how many of those only reserved events may use.
const MAX_BYTES: usize = 16 * 1024 * 1024;
const RESERVED_BYTES: usize = 2 * 1024 * 1024;

#[derive(Default)]
struct Buffer {
    queue: VecDeque<(SessionEvent, usize)>,
    bytes: usize,
    next_seq: u64,
    /// Delivery has been asked for and has not yet emptied the queue.
    draining: bool,
    /// The delivery task is running.
    deliverer: bool,
    /// Events dropped per run still in progress.
    dropped: HashMap<String, u64>,
}

/// Buffers events and delivers them to the factory on a task of their own.
pub(crate) struct EventHub {
    factory: Arc<dyn RunRecorderFactory>,
    buffer: Mutex<Buffer>,
    limits: (usize, usize, usize, usize),
    /// Wakes the delivery task.
    wake: Arc<tokio::sync::Notify>,
    /// Event ids are this, then the sequence number: unique without a random draw each.
    id_prefix: String,
}

impl EventHub {
    pub(crate) fn new(factory: Arc<dyn RunRecorderFactory>) -> Arc<Self> {
        Self::with_limits(
            factory,
            (MAX_EVENTS, RESERVED_EVENTS, MAX_BYTES, RESERVED_BYTES),
        )
    }

    fn with_limits(
        factory: Arc<dyn RunRecorderFactory>,
        limits: (usize, usize, usize, usize),
    ) -> Arc<Self> {
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
        let reserved = kind.reserved();
        let size = event_size(&kind);
        let (max_events, reserved_events, max_bytes, reserved_bytes) = self.limits;
        let mut buffer = self.lock();
        buffer.next_seq = buffer.next_seq.saturating_add(1);
        let seq = buffer.next_seq;
        let (events, bytes) = if reserved {
            (max_events, max_bytes)
        } else {
            (
                max_events.saturating_sub(reserved_events),
                max_bytes.saturating_sub(reserved_bytes),
            )
        };
        let fits = buffer.queue.len() < events
            && buffer.bytes.saturating_add(size) <= bytes
            && buffer.queue.try_reserve(1).is_ok();
        if !fits {
            if let Some(run_id) = run_id {
                let count = buffer.dropped.entry(run_id.to_owned()).or_default();
                *count = count.saturating_add(1);
            }
            return;
        }
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
        if buffer.draining {
            return;
        }
        buffer.draining = true;
        if buffer.deliverer {
            drop(buffer);
            self.wake.notify_one();
            return;
        }
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            // No runtime to hand delivery to: deliver here.
            drop(buffer);
            self.drain();
            return;
        };
        buffer.deliverer = true;
        drop(buffer);
        // One long-lived task delivers everything; it ends with the hub.
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

    /// The events dropped for `run_id`, forgetting the count.
    fn take_dropped(&self, run_id: &str) -> u64 {
        self.lock().dropped.remove(run_id).unwrap_or(0)
    }

    fn drain(&self) {
        loop {
            let next = {
                let mut buffer = self.lock();
                let Some((event, size)) = buffer.queue.pop_front() else {
                    buffer.draining = false;
                    return;
                };
                buffer.bytes = buffer.bytes.saturating_sub(size);
                event
            };
            self.factory.event(next);
        }
    }
}

/// Bytes an event holds, for the buffer's budget. A decision's capped record dominates;
/// the rest are small and charged a flat amount.
fn event_size(kind: &EventKind) -> usize {
    const BASE: usize = 512;
    match kind {
        EventKind::Decision { record } => {
            let misses: usize = record
                .near_misses
                .iter()
                .flat_map(|miss| &miss.failures)
                .map(|failure| {
                    failure.comparison.len()
                        + failure.expected.as_ref().map_or(0, String::len)
                        + failure.actual.as_ref().map_or(0, value_size)
                })
                .sum();
            BASE + value_size(&record.context) + misses
        }
        _ => BASE,
    }
}

/// Roughly what a JSON value holds: its strings and keys, and a word per node.
fn value_size(value: &serde_json::Value) -> usize {
    use serde_json::Value;
    match value {
        Value::String(text) => text.len() + 8,
        Value::Array(items) => items.iter().map(value_size).sum::<usize>() + 8,
        Value::Object(fields) => {
            fields
                .iter()
                .map(|(key, item)| key.len() + value_size(item))
                .sum::<usize>()
                + 8
        }
        _ => 8,
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

/// Records an MCP tool call on a session, when events are wanted.
pub(crate) fn tool_call(
    hub: Option<&Arc<EventHub>>,
    session_id: Option<&str>,
    tool_call_id: Option<&str>,
    tool: &str,
    ok: bool,
    result_bytes: u64,
    wall: std::time::Duration,
) {
    if let Some(hub) = hub {
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
        let hub = EventHub::with_limits(collect.clone(), (4, 1, usize::MAX, 0));
        {
            // Hold delivery back so the buffer fills.
            let mut buffer = hub.lock();
            buffer.draining = true;
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
