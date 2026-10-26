//! The run recorder: every run the playground's host executes, whoever sent it, is
//! stored as it finishes, and its session's events as they arrive.
//!
//! [`Recorder`] is the host's [`RunRecorder`](host::RunRecorder). A run's events come
//! through a hub of the recorder's own.
//!
//! Redaction happens here, on the way to disk: a run and each event are serialized,
//! every known secret is cut out of the serialized form, and only that is written.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError, Weak};
use std::time::{Duration, Instant};

use interpreter::runtime::{CallRecord, DecisionLogConfig, DecisionRecord};

use super::super::host::record::events::{
    EVENT_SCHEMA, EventHub, EventKind, EventSink, RunEvents, SessionEvent,
};
use super::super::host::run as host;
use super::super::log::warn;
use super::events::{EventBody, Gap, Position, StoredEvent};
use super::run::{RunLink, StoredRun};
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
pub(super) const FINISHED_RUN_MEMORY: Duration = Duration::from_secs(60);

/// The playground's [`RunRecorderFactory`].
#[derive(Clone)]
pub(crate) struct Recorder {
    shared: Arc<Shared>,
}

struct Shared {
    store: Arc<Store>,
    secrets: KnownSecrets,
    /// Runs in progress or recently finished, by the host's execution id.
    tracks: Mutex<HashMap<String, Track>>,
    /// Runs the store could not take, by the host's execution id, with when their end
    /// event arrived: they run unrecorded, and their events are logged without a run id.
    untracked: Mutex<HashMap<String, Option<Instant>>>,
    /// The host-wide sequence number of the last event delivered; a jump past the next
    /// one means the host dropped events in between.
    last_seq: AtomicU64,
    /// The session of each recent MCP tool call its client named, by that name: how chat
    /// from a coding assistant finds the session its tool calls ran in.
    calls: Mutex<RecentCalls>,
    /// Delivers the host's run events, made on its first run.
    host_events: OnceLock<Arc<EventHub>>,
}

/// The most recent tool calls' sessions, by the call's id, oldest forgotten first.
#[derive(Default)]
struct RecentCalls {
    sessions: HashMap<String, String>,
    order: std::collections::VecDeque<String>,
}

/// How many tool calls' sessions are kept in memory; older ones are found on disk.
const RECENT_CALLS: usize = 4096;

impl RecentCalls {
    fn insert(&mut self, call: &str, session: &str) {
        if self.sessions.contains_key(call) {
            return;
        }
        if self.order.len() >= RECENT_CALLS
            && let Some(oldest) = self.order.pop_front()
        {
            self.sessions.remove(&oldest);
        }
        self.sessions.insert(call.to_owned(), session.to_owned());
        self.order.push_back(call.to_owned());
    }
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
    /// The run's end event arrived, with the count of its events the host dropped.
    end_event: Option<u64>,
    /// Decision events delivered for the run: the last one's number in the run, while
    /// none was dropped before it.
    decisions_delivered: u64,
    /// The host dropped an event while the run's events were still coming, so a
    /// decision's number can no longer be told from the count. The host's sequence
    /// shows that an event was dropped, not whose, so a drop marks every run whose end
    /// has not arrived, including runs that lost nothing: their later decisions go
    /// unnumbered. Numbers are never wrong, only missing.
    numbering_lost: bool,
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
                untracked: Mutex::new(HashMap::new()),
                last_seq: AtomicU64::new(0),
                calls: Mutex::new(RecentCalls::default()),
                host_events: OnceLock::new(),
            }),
        }
    }

    /// The hub the host's events go through, for the MCP endpoint's tool calls.
    pub(crate) fn events(&self) -> &Arc<EventHub> {
        self.shared.host_events()
    }

    pub(crate) fn store(&self) -> &Arc<Store> {
        &self.shared.store
    }

    /// The session one of the tool calls `ids` ran in: a recent call's from memory,
    /// else one in the event logs on disk.
    pub(crate) fn session_of_call<'a>(
        &self,
        ids: impl IntoIterator<Item = &'a str> + Clone,
    ) -> Option<String> {
        let recent = {
            let calls = self
                .shared
                .calls
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            ids.clone()
                .into_iter()
                .find_map(|id| calls.sessions.get(id).cloned())
        };
        recent.or_else(|| {
            let ids: HashSet<&str> = ids.into_iter().collect();
            if ids.is_empty() {
                return None;
            }
            self.shared
                .store
                .session_with_call(&ids)
                .unwrap_or_else(|error| {
                    warn(&format!(
                        "looking for a tool call's session failed: {error}"
                    ));
                    None
                })
        })
    }

    /// Appends a chat entry to `session`'s log at `position`, with known secrets masked.
    pub(crate) fn append_chat(
        &self,
        session: &str,
        event_id: String,
        position: Position,
        body: EventBody,
    ) {
        self.shared.append_stored(
            Some(session.to_owned()),
            StoredEvent {
                format: FORMAT,
                session_seq: 0,
                event_id,
                position,
                run: None,
                backfilled: false,
                decision: None,
                body,
            },
        );
    }

    /// The host's execution id of run `id` while it is in flight: started, and not yet
    /// finished. A run's id is handed out when it starts, so this is how a playground run
    /// id reaches the host's cancellation.
    /// The host execution ids of session `session`'s runs in flight.
    pub(crate) fn in_flight_in(&self, session: &str) -> Vec<String> {
        self.shared
            .tracks()
            .values()
            .filter(|track| {
                track.session.as_deref() == Some(session) && track.finished_at.is_none()
            })
            .map(|track| track.execution_id.clone())
            .collect()
    }

    pub(crate) fn in_flight(&self, id: u64) -> Option<String> {
        self.shared
            .tracks()
            .values()
            .find(|track| track.id == id && track.finished_at.is_none())
            .map(|track| track.execution_id.clone())
    }

    /// Whether the run with host execution id `execution_id` started under this
    /// recorder (recorded, or left unrecorded when the store could not take it) and is
    /// still followed: in flight, or finished less than [`FINISHED_RUN_MEMORY`] ago. A
    /// run a previous playground started is never followed.
    pub(crate) fn follows(&self, execution_id: &str) -> bool {
        self.follows_at(execution_id, Instant::now())
    }

    /// [`Self::follows`], as of `now`.
    pub(super) fn follows_at(&self, execution_id: &str, now: Instant) -> bool {
        let finished_at = match self.shared.tracks().get(execution_id) {
            Some(track) => track.finished_at,
            None => match self.shared.untracked().get(execution_id) {
                Some(finished_at) => *finished_at,
                None => return false,
            },
        };
        still_followed(finished_at, now)
    }
}

impl host::RunRecorder for Recorder {
    fn start(&self, run: host::RunStart) -> Option<Arc<dyn host::RecordedRun>> {
        let core = self.shared.begin(Begin {
            execution_id: &run.execution_id,
            label: &run.label,
            session: Some(&run.session_id),
            tool_call_id: run.tool_call_id.as_deref(),
            test_of: run.test_of.as_deref(),
            code: run.code.as_deref(),
            harness_secrets: run.harness_secrets.values(),
        })?;
        let events = RunEvents::start(self.shared.host_events(), &run);
        Some(Arc::new(HostRunRecording {
            core,
            start: run,
            events,
        }))
    }
}

#[cfg(test)]
impl Recorder {
    /// This recorder with its runs' own events left out: a test sends events with
    /// [`Self::event`], numbered as it chooses.
    pub(crate) fn with_events_sent_by_hand(self) -> Self {
        struct Ignored;
        impl EventSink for Ignored {
            fn event(&self, _event: SessionEvent) {}
        }
        let _ = self
            .shared
            .host_events
            .set(EventHub::new(Arc::new(Ignored) as Arc<dyn EventSink>));
        self
    }

    /// Takes `event` as if the host's hub delivered it.
    pub(crate) fn event(&self, event: SessionEvent) {
        self.shared.event(event);
    }
}

/// What the recorder takes from a run as it starts.
struct Begin<'a, S> {
    execution_id: &'a str,
    label: &'a str,
    session: Option<&'a str>,
    tool_call_id: Option<&'a str>,
    test_of: Option<&'a str>,
    code: Option<&'a str>,
    harness_secrets: S,
}

impl Shared {
    /// Follows a run that is starting, and lists it as running. `None`, with a
    /// warning, when the store cannot take it: it runs unrecorded.
    fn begin<'a, S: IntoIterator<Item = &'a String>>(
        self: &Arc<Self>,
        run: Begin<'a, S>,
    ) -> Option<Core> {
        self.secrets.add_all(run.harness_secrets);
        let id = match self.store.next_run_id() {
            Ok(id) => id,
            Err(error) => {
                warn(&format!("run not recorded: {error}"));
                let mut untracked = self.untracked();
                untracked.retain(|_, finished_at| still_followed(*finished_at, Instant::now()));
                untracked.insert(run.execution_id.to_owned(), None);
                return None;
            }
        };
        let started_at_micros = now_micros();
        let test_of = run.test_of.map(|execution_id| RunLink {
            run: self.run_id_of(execution_id),
            execution_id: execution_id.to_owned(),
        });
        let mut tracks = self.tracks();
        prune(&mut tracks);
        tracks.insert(
            run.execution_id.to_owned(),
            Track {
                id,
                execution_id: run.execution_id.to_owned(),
                session: run.session.map(str::to_owned),
                tool_call_id: run.tool_call_id.map(str::to_owned),
                started_at_micros,
                seen: HashSet::new(),
                end_event: None,
                decisions_delivered: 0,
                numbering_lost: false,
                record: None,
                backfilled: false,
                finished_at: None,
            },
        );
        drop(tracks);
        if let Err(error) =
            self.store
                .mark_running(id, run.label, run.session, started_at_micros, run.code)
        {
            warn(&format!("run {id} not listed as running: {error}"));
        }
        Some(Core {
            shared: Arc::clone(self),
            execution_id: run.execution_id.to_owned(),
            id,
            test_of,
            started_at_micros,
        })
    }

    /// The hub the host's run events go through, to [`Self::event`].
    fn host_events(self: &Arc<Self>) -> &Arc<EventHub> {
        self.host_events.get_or_init(|| {
            EventHub::new(Arc::new(HostEvents(Arc::downgrade(self))) as Arc<dyn EventSink>)
        })
    }

    fn tracks(&self) -> MutexGuard<'_, HashMap<String, Track>> {
        self.tracks.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Taken alone, or after [`Self::tracks`], never before it.
    fn untracked(&self) -> MutexGuard<'_, HashMap<String, Option<Instant>>> {
        self.untracked
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// The store's id for a host execution id: a run in memory, or one stored earlier.
    fn run_id_of(&self, execution_id: &str) -> Option<u64> {
        if let Some(track) = self.tracks().get(execution_id) {
            return Some(track.id);
        }
        self.store.run_id_of(execution_id).ok().flatten()
    }

    fn event(&self, event: SessionEvent) {
        if let (Some(call), Some(session)) = (&event.tool_call_id, &event.session_id) {
            self.calls
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .insert(call, session);
        }
        let mut tracks = self.tracks();
        let previous = self.last_seq.swap(event.seq, Ordering::Relaxed);
        if event.seq > previous.saturating_add(1) {
            // The dropped events may be any run's whose end has not arrived yet.
            for track in tracks
                .values_mut()
                .filter(|track| track.end_event.is_none())
            {
                track.numbering_lost = true;
            }
        }
        let track = event
            .run_id
            .as_ref()
            .and_then(|run_id| tracks.get_mut(run_id));
        let Some(track) = track else {
            if let (Some(run_id), EventKind::RunFinished { .. }) = (&event.run_id, &event.kind)
                && let Some(finished_at) = self.untracked().get_mut(run_id)
            {
                finished_at.get_or_insert_with(Instant::now);
            }
            let placement = Placement {
                position: Position {
                    at_micros: event.at_micros,
                    run: None,
                    call_index: None,
                    rank: 0,
                },
                run: None,
                decision: None,
                backfilled: false,
            };
            self.append(event.session_id.clone(), placement, event);
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
        // A run's log keeps each decision and reports it from the run's own thread, so its
        // decision events arrive in the order the record holds them.
        let decision = match &event.kind {
            EventKind::Decision { .. } => {
                track.decisions_delivered = track.decisions_delivered.saturating_add(1);
                (!track.numbering_lost).then_some(track.decisions_delivered)
            }
            _ => None,
        };
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
        let placement = Placement {
            position,
            run,
            decision,
            backfilled: false,
        };
        self.append(session, placement, event);
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

    /// The run's recording was dropped. A run dropped without finishing has no record and
    /// will deliver no more events, so it ends here: no longer in flight, and pruned in time
    /// like a finished run.
    fn dropped(&self, execution_id: &str) {
        if let Some(track) = self.tracks().get_mut(execution_id) {
            track.finished_at.get_or_insert_with(Instant::now);
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
            // Finished outside the host's runtime (a run dropped while unwinding):
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
        // Each with its decision's number in the run, which the record's order gives.
        let mut recovered: Vec<(Position, Option<u64>, EventKind)> = Vec::new();
        for (n, decision) in (1_u64..).zip(&record.decisions) {
            if track.seen.insert(EventKey::decision(decision)) {
                let kind = EventKind::Decision {
                    record: Box::new(decision.clone()),
                };
                recovered.push((position_of(&kind, 0, track), Some(n), kind));
            }
        }
        for call in record.calls.iter().filter(|call| call.outcome.is_some()) {
            if track.seen.insert(EventKey::CallFinished(call.call_index)) {
                let kind = call_finished(call);
                recovered.push((position_of(&kind, 0, track), None, kind));
            }
        }
        let end_missing = track.end_event.is_none();
        if end_missing {
            let at = now_micros();
            recovered.push((position_of(&record.end, at, track), None, record.end));
        }
        recovered.sort_by_key(|(position, _, _)| *position);
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
        self.append_stored(
            track.session.clone(),
            StoredEvent {
                format: FORMAT,
                session_seq: 0,
                event_id: format!("{execution_id}-gap"),
                position: gap_position,
                run: Some(track.id),
                backfilled: false,
                decision: None,
                body: EventBody::Gap(gap),
            },
        );
        for (n, (position, decision, kind)) in recovered.into_iter().enumerate() {
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
            let placement = Placement {
                position,
                run: Some(track.id),
                decision,
                backfilled: true,
            };
            self.append(track.session.clone(), placement, event);
        }
    }

    fn append(&self, session: Option<String>, placement: Placement, event: SessionEvent) {
        let stored = StoredEvent {
            format: FORMAT,
            session_seq: 0,
            event_id: event.event_id.clone(),
            position: placement.position,
            run: placement.run,
            backfilled: placement.backfilled,
            decision: placement.decision,
            body: EventBody::Event(Box::new(event)),
        };
        self.append_stored(session, stored);
    }

    /// Appends `event` to `session`'s log, which numbers it.
    fn append_stored(&self, session: Option<String>, event: StoredEvent) {
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

/// Where an event goes in its session's log, and what the log says of it.
struct Placement {
    position: Position,
    /// The store's id of its run, when the recorder follows the run.
    run: Option<u64>,
    /// Its number among its run's decisions, when it is a decision whose number is known.
    decision: Option<u64>,
    /// Recovered from the run's record rather than delivered.
    backfilled: bool,
}

/// Forgets runs that finished long enough ago that no more of their events can come.
fn prune(tracks: &mut HashMap<String, Track>) {
    let now = Instant::now();
    tracks.retain(|_, track| still_followed(track.finished_at, now));
}

/// Whether a run that finished at `finished_at`, if it did, may still deliver events at
/// `now`.
fn still_followed(finished_at: Option<Instant>, now: Instant) -> bool {
    finished_at.is_none_or(|finished| now.saturating_duration_since(finished) < FINISHED_RUN_MEMORY)
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
        // After the end of the call before it, and before the start of the next.
        EventKind::Console {
            written_micros,
            after_call,
            ..
        } => Position {
            at_micros: base.saturating_add(*written_micros),
            run: Some(track.id),
            call_index: *after_call,
            rank: 3,
        },
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

/// One run the recorder follows: stores it as it finishes, and however it ended,
/// finished or dropped, it is no longer in flight once this drops.
struct Core {
    shared: Arc<Shared>,
    execution_id: String,
    id: u64,
    test_of: Option<RunLink>,
    started_at_micros: u64,
}

impl Core {
    /// Stores the finished run, redacted, and hands its record to the event log.
    fn store(&self, stored: &StoredRun, console_bytes: u64) {
        let Some(stored) = self.redacted(stored) else {
            return;
        };
        if let Err(error) = self.shared.store.write_run(&stored) {
            warn(&format!("run {} not stored: {error}", self.id));
        }
        if let Err(error) = self.shared.store.unmark_running(self.id) {
            warn(&format!("run {} still listed as running: {error}", self.id));
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
                console_bytes,
                events_dropped: 0,
            },
            truncated: stored.recording.log_truncated,
        };
        self.shared.finished(&self.execution_id, backfill);
    }

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

impl Drop for Core {
    /// However the run ended, finished or dropped, it is no longer in flight.
    fn drop(&mut self) {
        if let Err(error) = self.shared.store.unmark_running(self.id) {
            warn(&format!("run {} still listed as running: {error}", self.id));
        }
        self.shared.dropped(&self.execution_id);
    }
}

/// Records one host run, with its events.
struct HostRunRecording {
    core: Core,
    start: host::RunStart,
    events: Arc<RunEvents>,
}

impl host::RecordedRun for HostRunRecording {
    fn log_config(&self) -> DecisionLogConfig {
        PLAYGROUND_LOG_CONFIG
    }

    fn event(&self, event: host::RunEvent<'_>) {
        self.events.observe(event);
    }

    fn finish(&self, run: host::FinishedRun) {
        let console_bytes = run.console.len() as u64;
        self.events.finished(
            run.dispatched,
            run.error.as_ref().map(|error| error.kind),
            run.wall,
            console_bytes,
        );
        let core = &self.core;
        let stored = StoredRun::from_parts(
            core.id,
            &self.start,
            &run,
            core.test_of.clone(),
            core.started_at_micros,
        );
        core.store(&stored, console_bytes);
    }

    fn returned(&self, bytes: u64) {
        self.events.returned(bytes);
    }
}

/// Takes the host's events into the recorder.
struct HostEvents(Weak<Shared>);

impl EventSink for HostEvents {
    fn event(&self, event: SessionEvent) {
        if let Some(shared) = self.0.upgrade() {
            shared.event(event);
        }
    }
}
