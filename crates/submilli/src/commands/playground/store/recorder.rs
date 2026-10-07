//! The server's run recorder: every run the playground's server executes, whoever sent
//! it, is stored as it finishes, and its session's events as they arrive.
//!
//! Redaction happens here, on the way to disk: a run and each event are serialized,
//! every known secret is cut out of the serialized form, and only that is written.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use interpreter::runtime::{CallRecord, DecisionLogConfig, DecisionRecord};
use submilli_server::record::{
    EVENT_SCHEMA, EventKind, FinishedRun, RetryLink, RunRecorder, RunRecorderFactory, RunStart,
    SessionEvent,
};

use super::super::log::warn;
use super::events::{EventBody, Gap, Position, StoredEvent};
use super::run::{RetryRecord, RunLink, StoredRun};
use super::{FORMAT, KnownSecrets, Store, json_line, now_micros};

/// The recording caps for every run the playground serves, raised from the server's
/// defaults (10,000 decisions and calls, 1 KiB context strings, 1 MiB body copies, a
/// 16 MiB recorder budget) so fewer test runs stop with "recording incomplete":
///
/// - Body copies of 4 MiB: a test run can answer a call only from a body the recording
///   kept whole, and 4 MiB covers the API responses a program typically works through
///   (paged listings, documents) where 1 MiB cut a few.
/// - A 64 MiB recorder budget: room for a dozen full-size bodies besides the decisions,
///   and held only while a run executes, outside the run's own memory limit. A local
///   machine affords it; a deployed server keeps its own default.
/// - 50,000 decisions and calls: long loops over a listing stay whole.
/// - 4 KiB context strings: URLs with long query strings and request fields stay
///   readable in `explain`, still capped with a marker.
/// - Four times the line-capture frame budget, for the larger decision cap.
pub(crate) const PLAYGROUND_LOG_CONFIG: DecisionLogConfig = DecisionLogConfig {
    max_decisions: 50_000,
    max_context_value_bytes: 4 * 1024,
    max_recorder_bytes: 64 * 1024 * 1024,
    max_line_capture_frames: 4_000_000,
    max_calls: 50_000,
    max_payload_bytes: 4 * 1024 * 1024,
};

/// How long after a run finishes its end event may still be on its way. A run whose end
/// event has not arrived by then lost it to an overflow, and is backfilled without it.
const END_EVENT_GRACE: Duration = Duration::from_secs(2);

/// How long a finished run's ids stay known, for its late events (what its caller
/// received arrives after its end).
const FINISHED_RUN_MEMORY: Duration = Duration::from_secs(60);

/// The playground's [`RunRecorderFactory`].
#[derive(Clone)]
pub(crate) struct Recorder {
    shared: Arc<Shared>,
}

struct Shared {
    store: Arc<Store>,
    secrets: KnownSecrets,
    /// Runs in progress or recently finished, by the server's execution id.
    tracks: Mutex<HashMap<String, Track>>,
}

/// What the recorder knows of one run while its events arrive.
struct Track {
    id: u64,
    execution_id: String,
    session: Option<String>,
    tool_call_id: Option<String>,
    started_at_micros: u64,
    /// Decisions and finished calls whose events are in the log.
    seen: HashSet<EventKey>,
    /// The run's end event arrived, with the count of its events the server dropped.
    end_event: Option<u64>,
    /// The run's record, once it finished; what a backfill reads.
    record: Option<Backfill>,
    backfilled: bool,
    finished_at: Option<Instant>,
}

/// What a backfill takes from a finished run's record.
struct Backfill {
    decisions: Vec<DecisionRecord>,
    calls: Vec<CallRecord>,
    end: EventKind,
    truncated: bool,
}

/// Identifies an event a backfill could duplicate.
#[derive(Clone, PartialEq, Eq, Hash)]
enum EventKey {
    Decision {
        call_index: u64,
        seq: u64,
        at_micros: u64,
        capability: String,
    },
    CallFinished(u64),
}

impl EventKey {
    fn of(kind: &EventKind) -> Option<Self> {
        match kind {
            EventKind::Decision { record } => Some(Self::decision(record)),
            EventKind::CallFinished { call_index, .. } => Some(Self::CallFinished(*call_index)),
            _ => None,
        }
    }

    fn decision(record: &DecisionRecord) -> Self {
        Self::Decision {
            call_index: record.call_index,
            seq: record.seq,
            at_micros: record.at_micros,
            capability: record.capability.clone(),
        }
    }
}

impl Recorder {
    pub(crate) fn new(store: Arc<Store>, secrets: KnownSecrets) -> Self {
        Self {
            shared: Arc::new(Shared {
                store,
                secrets,
                tracks: Mutex::new(HashMap::new()),
            }),
        }
    }

    pub(crate) fn store(&self) -> &Arc<Store> {
        &self.shared.store
    }
}

impl RunRecorderFactory for Recorder {
    fn start(&self, run: RunStart) -> Option<Arc<dyn RunRecorder>> {
        let shared = &self.shared;
        shared.secrets.add_all(run.harness_secrets.values());
        let id = match shared.store.next_run_id() {
            Ok(id) => id,
            Err(error) => {
                warn(&format!("run not recorded: {error}"));
                return None;
            }
        };
        let started_at_micros = now_micros();
        let test_of = run.test_of.as_ref().map(|execution_id| RunLink {
            run: shared.run_id_of(execution_id),
            execution_id: execution_id.clone(),
        });
        let mut tracks = shared.tracks();
        prune(&mut tracks);
        tracks.insert(
            run.execution_id.clone(),
            Track {
                id,
                execution_id: run.execution_id.clone(),
                session: run.session_id.clone(),
                tool_call_id: run.tool_call_id.clone(),
                started_at_micros,
                seen: HashSet::new(),
                end_event: None,
                record: None,
                backfilled: false,
                finished_at: None,
            },
        );
        drop(tracks);
        Some(Arc::new(RunRecording {
            shared: Arc::clone(shared),
            start: run,
            id,
            test_of,
            started_at_micros,
        }))
    }

    fn retried(&self, retry: RetryLink) {
        let original = retry.original_execution_id.map(|execution_id| RunLink {
            run: self.shared.run_id_of(&execution_id),
            execution_id,
        });
        let record = RetryRecord {
            format: FORMAT,
            at_micros: now_micros(),
            session_id: retry.session_id,
            idempotency_key: retry.idempotency_key,
            original,
        };
        if let Err(error) = self.shared.store.record_retry(&record) {
            warn(&format!("retry not recorded: {error}"));
        }
    }

    fn wants_events(&self) -> bool {
        true
    }

    fn event(&self, event: SessionEvent) {
        self.shared.event(event);
    }
}

impl Shared {
    fn tracks(&self) -> MutexGuard<'_, HashMap<String, Track>> {
        self.tracks.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The store's id for a server execution id: a run in memory, or one stored earlier.
    fn run_id_of(&self, execution_id: &str) -> Option<u64> {
        if let Some(track) = self.tracks().get(execution_id) {
            return Some(track.id);
        }
        self.store.run_id_of(execution_id).ok().flatten()
    }

    fn event(&self, event: SessionEvent) {
        let mut tracks = self.tracks();
        let track = event
            .run_id
            .as_ref()
            .and_then(|run_id| tracks.get_mut(run_id));
        let Some(track) = track else {
            let position = Position {
                at_micros: event.at_micros,
                run: None,
                call_index: None,
                rank: 0,
            };
            self.append(event.session_id.clone(), position, None, false, event);
            return;
        };
        let key = EventKey::of(&event.kind);
        if let Some(key) = &key {
            if track.backfilled && track.seen.contains(key) {
                return;
            }
            track.seen.insert(key.clone());
        }
        let position = position_of(&event.kind, event.at_micros, track);
        let run = Some(track.id);
        let session = track.session.clone();
        let end = match &event.kind {
            EventKind::RunFinished { events_dropped, .. } => Some(*events_dropped),
            _ => None,
        };
        if end.is_some() && track.backfilled {
            // The backfill already wrote this run's end from its record.
            return;
        }
        self.append(session, position, run, false, event);
        if let Some(dropped) = end {
            track.end_event = Some(dropped);
            if dropped > 0 && track.record.is_some() {
                self.backfill(track);
            }
        }
    }

    /// The run finished: its record is what a backfill reads. When its end event already
    /// reported drops, backfill now; when the end event has not arrived, give it a moment
    /// and backfill if it never comes.
    fn finished(self: &Arc<Self>, execution_id: &str, record: Backfill) {
        let mut tracks = self.tracks();
        let Some(track) = tracks.get_mut(execution_id) else {
            return;
        };
        track.record = Some(record);
        track.finished_at = Some(Instant::now());
        match track.end_event {
            Some(dropped) if dropped > 0 => self.backfill(track),
            Some(_) => track.seen = HashSet::new(),
            None => {
                drop(tracks);
                self.backfill_later(execution_id.to_owned());
            }
        }
    }

    fn backfill_later(self: &Arc<Self>, execution_id: String) {
        let shared = Arc::clone(self);
        let check = move || {
            let mut tracks = shared.tracks();
            if let Some(track) = tracks.get_mut(&execution_id)
                && track.end_event.is_none()
                && !track.backfilled
            {
                shared.backfill(track);
            }
        };
        match tokio::runtime::Handle::try_current() {
            Ok(runtime) => {
                runtime.spawn(async move {
                    tokio::time::sleep(END_EVENT_GRACE).await;
                    check();
                });
            }
            // Finished outside the server's runtime (a run dropped while unwinding):
            // nothing will deliver its end now.
            Err(_) => check(),
        }
    }

    /// Appends a gap entry for the run, then every decision, finished call, and the end
    /// its record has and the log lacks, each at its causal position.
    fn backfill(&self, track: &mut Track) {
        let Some(record) = track.record.take() else {
            return;
        };
        track.backfilled = true;
        let mut recovered: Vec<(Position, EventKind)> = Vec::new();
        for decision in &record.decisions {
            if track.seen.insert(EventKey::decision(decision)) {
                let kind = EventKind::Decision {
                    record: Box::new(decision.clone()),
                };
                recovered.push((position_of(&kind, 0, track), kind));
            }
        }
        for call in record.calls.iter().filter(|call| call.outcome.is_some()) {
            if track.seen.insert(EventKey::CallFinished(call.call_index)) {
                let kind = call_finished(call);
                recovered.push((position_of(&kind, 0, track), kind));
            }
        }
        let end_missing = track.end_event.is_none();
        if end_missing {
            let at = now_micros();
            recovered.push((position_of(&record.end, at, track), record.end));
        }
        recovered.sort_by_key(|(position, _)| *position);
        let execution_id = track.execution_id.clone();
        let gap = Gap {
            run_id: execution_id.clone(),
            dropped: track.end_event,
            recovered: recovered.len() as u64,
            lost: record.truncated,
        };
        let gap_position = Position {
            at_micros: now_micros(),
            run: Some(track.id),
            call_index: None,
            rank: u8::MAX,
        };
        self.append_body(
            track.session.clone(),
            format!("{execution_id}-gap"),
            gap_position,
            Some(track.id),
            false,
            EventBody::Gap(gap),
        );
        for (n, (position, kind)) in recovered.into_iter().enumerate() {
            let event = SessionEvent {
                schema: EVENT_SCHEMA,
                event_id: format!("{execution_id}-backfill-{n}"),
                seq: 0,
                at_micros: position.at_micros,
                session_id: track.session.clone(),
                run_id: Some(execution_id.clone()),
                tool_call_id: track.tool_call_id.clone(),
                kind,
            };
            self.append(track.session.clone(), position, Some(track.id), true, event);
        }
    }

    fn append(
        &self,
        session: Option<String>,
        position: Position,
        run: Option<u64>,
        backfilled: bool,
        event: SessionEvent,
    ) {
        let event_id = event.event_id.clone();
        self.append_body(
            session,
            event_id,
            position,
            run,
            backfilled,
            EventBody::Event(Box::new(event)),
        );
    }

    fn append_body(
        &self,
        session: Option<String>,
        event_id: String,
        position: Position,
        run: Option<u64>,
        backfilled: bool,
        body: EventBody,
    ) {
        let event = StoredEvent {
            format: FORMAT,
            session_seq: 0,
            event_id,
            position,
            run,
            backfilled,
            body,
        };
        let appended = self
            .store
            .events
            .append(session.as_deref(), event, |event| {
                // Read back like a run, so a line that would not parse is never written;
                // the event is then left out of the log, with a warning.
                match self.secrets.redact_record(event) {
                    Ok(redacted) => Some(json_line(&redacted.value)),
                    Err(error) => {
                        warn(&format!("event {} not recorded: {error}", event.event_id));
                        None
                    }
                }
            });
        if let Err(error) = appended {
            warn(&format!("event not recorded: {error}"));
        }
    }
}

/// Forgets runs that finished long enough ago that no more of their events can come.
fn prune(tracks: &mut HashMap<String, Track>) {
    tracks.retain(|_, track| {
        track
            .finished_at
            .is_none_or(|finished| finished.elapsed() < FINISHED_RUN_MEMORY)
    });
}

/// Where an event of `track`'s run happened. Call events are placed by the run's own
/// clock (microseconds since its recorder started) from the run's start, the same way
/// whether the event arrived or was recovered, so the two sort consistently.
fn position_of(kind: &EventKind, arrived_micros: u64, track: &Track) -> Position {
    let base = track.started_at_micros;
    let in_run = |micros: u64, call_index: u64, rank: u8| Position {
        at_micros: base.saturating_add(micros),
        run: Some(track.id),
        call_index: Some(call_index),
        rank,
    };
    match kind {
        EventKind::RunStarted { .. } => Position {
            at_micros: base,
            run: Some(track.id),
            call_index: None,
            rank: 0,
        },
        EventKind::CallStarted {
            call_index,
            started_micros,
            ..
        } => in_run(*started_micros, *call_index, 0),
        EventKind::Decision { record } => in_run(record.at_micros, record.call_index, 1),
        EventKind::CallFinished {
            call_index,
            started_micros,
            ended_micros,
            ..
        } => in_run(ended_micros.unwrap_or(*started_micros), *call_index, 2),
        EventKind::RunFinished { .. } | EventKind::Returned { .. } | EventKind::ToolCall { .. } => {
            Position {
                at_micros: arrived_micros,
                run: Some(track.id),
                call_index: None,
                rank: 3,
            }
        }
    }
}

fn call_finished(call: &CallRecord) -> EventKind {
    EventKind::CallFinished {
        call_index: call.call_index,
        capability: call.capability.clone(),
        started_micros: call.started_micros,
        ended_micros: call.ended_micros,
        outcome: call.outcome,
        sent_bytes: call.request.as_ref().map(|request| request.bytes),
        result_bytes: call.response.as_ref().map(|response| response.bytes),
        usage: call.usage,
    }
}

/// Records one run.
struct RunRecording {
    shared: Arc<Shared>,
    start: RunStart,
    id: u64,
    test_of: Option<RunLink>,
    started_at_micros: u64,
}

impl RunRecorder for RunRecording {
    fn log_config(&self) -> DecisionLogConfig {
        PLAYGROUND_LOG_CONFIG
    }

    fn finish(&self, run: FinishedRun) {
        let stored = StoredRun::from_parts(
            self.id,
            &self.start,
            &run,
            self.test_of.clone(),
            self.started_at_micros,
        );
        let Some(stored) = self.redacted(&stored) else {
            return;
        };
        if let Err(error) = self.shared.store.write_run(&stored) {
            warn(&format!("run {} not stored: {error}", self.id));
        }
        let backfill = Backfill {
            decisions: stored.recording.decisions.clone(),
            calls: stored
                .recording
                .calls
                .iter()
                .map(CallRecord::without_bodies)
                .collect(),
            end: EventKind::RunFinished {
                dispatched: stored.dispatched,
                error: stored.error.as_ref().map(|error| error.kind),
                wall_ms: stored.wall_ms,
                console_bytes: run.console.len() as u64,
                events_dropped: 0,
            },
            truncated: stored.recording.log_truncated,
        };
        self.shared.finished(&self.start.execution_id, backfill);
    }
}

impl RunRecording {
    /// The run with every known secret cut out. `None`, with a warning, when the
    /// redacted form does not read back: the run is not stored rather than stored
    /// unredacted.
    fn redacted(&self, run: &StoredRun) -> Option<StoredRun> {
        match self.shared.secrets.redact_record(run) {
            Ok(redacted) => Some(redacted.record),
            Err(error) => {
                warn(&format!("run {} not stored: {error}", self.id));
                None
            }
        }
    }
}
