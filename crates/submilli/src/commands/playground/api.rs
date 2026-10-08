//! The control API: every route the control listener serves, in one table the router is
//! built from, by who may call it and the CLI subcommand that does the same.
//!
//! The actions answer with an [`Answer`]: the result the CLI prints with `--json`, its
//! text form, and the exit status, so the CLI and the page see one result. The reads
//! answer with the read result itself, for the page; the CLI reads the store directly.
//! The event feed serves a session's event log as server-sent events.

use std::collections::{HashMap, VecDeque};
use std::convert::Infallible;
use std::sync::Arc;

use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection, QueryRejection};
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{MethodRouter, get, post};
use serde::Deserialize;
use serde_json::json;
use submilli_server::record::EventKind;

use super::controls::act::{
    ActError, Actions, Answer, BindRequest, DraftRequest, ExecRequest, RunRequest,
    SessionEndRequest, SessionStartRequest, TestRequest,
};
use super::controls::read::{self, AuditQuery, RunsQuery, Since};
use super::controls::render::EXIT_FAILURE;
use super::controls::{ReadError, Reader};
use super::host::{self, ControlState};
use super::store::events::{EventBody, StoredEvent};
use super::store::run::DecisionRef;

/// Who may call a route.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Group {
    /// No credential: the page, the login-code exchange, and the nonce challenge.
    Public,
    /// The admin token only: the CLI.
    Admin,
    /// The admin token or a browser session: the CLI and the page.
    Viewer,
    /// The `stand-in` token only: the bridge.
    StandIn,
}

/// One control route.
#[cfg_attr(
    not(test),
    allow(dead_code, reason = "the parity test reads the method and the command")
)]
pub(crate) struct Route {
    pub(crate) method: &'static str,
    pub(crate) path: &'static str,
    pub(crate) group: Group,
    /// The CLI subcommand that does what the route does; `None` for what only the page
    /// or the bridge use (the page itself, signing in, the nonce challenge, the bridge's
    /// binding).
    pub(crate) command: Option<&'static str>,
    pub(crate) handler: fn() -> MethodRouter<ControlState>,
}

/// Every route the control listener serves.
pub(crate) const ROUTES: &[Route] = &[
    Route {
        method: "GET",
        path: "/",
        group: Group::Public,
        command: None,
        handler: || get(host::page),
    },
    Route {
        method: "POST",
        path: "/api/challenge",
        group: Group::Public,
        command: None,
        handler: || post(host::challenge),
    },
    Route {
        method: "POST",
        path: "/api/login",
        group: Group::Public,
        command: None,
        handler: || post(host::login),
    },
    Route {
        method: "POST",
        path: "/api/login-codes",
        group: Group::Admin,
        command: Some("open"),
        handler: || post(host::mint_login_code),
    },
    Route {
        method: "POST",
        path: "/api/stop",
        group: Group::Admin,
        command: Some("stop"),
        handler: || post(host::stop),
    },
    Route {
        method: "POST",
        path: "/api/exec",
        group: Group::Admin,
        command: Some("exec"),
        handler: || post(exec),
    },
    Route {
        method: "GET",
        path: "/api/binding",
        group: Group::Admin,
        command: Some("bind"),
        handler: || get(binding),
    },
    Route {
        method: "POST",
        path: "/api/bind",
        group: Group::Admin,
        command: Some("bind"),
        handler: || post(bind),
    },
    Route {
        method: "POST",
        path: "/api/sessions/start",
        group: Group::Admin,
        command: Some("session"),
        handler: || post(session_start),
    },
    Route {
        method: "POST",
        path: "/api/sessions/end",
        group: Group::Admin,
        command: Some("session"),
        handler: || post(session_end),
    },
    Route {
        method: "POST",
        path: "/api/recheck",
        group: Group::Admin,
        command: Some("recheck"),
        handler: || post(recheck),
    },
    Route {
        method: "POST",
        path: "/api/test",
        group: Group::Admin,
        command: Some("test"),
        handler: || post(test),
    },
    Route {
        method: "POST",
        path: "/api/rerun",
        group: Group::Admin,
        command: Some("rerun"),
        handler: || post(rerun),
    },
    Route {
        method: "POST",
        path: "/api/clear",
        group: Group::Admin,
        command: Some("clear"),
        handler: || post(clear),
    },
    Route {
        method: "POST",
        path: "/api/cancel",
        group: Group::Admin,
        command: Some("cancel"),
        handler: || post(cancel),
    },
    Route {
        method: "GET",
        path: "/api/status",
        group: Group::Viewer,
        command: Some("status"),
        handler: || get(host::status),
    },
    Route {
        method: "POST",
        path: "/api/draft-rule/write",
        group: Group::Admin,
        command: Some("draft-rule"),
        handler: || post(draft_rule_write),
    },
    Route {
        method: "POST",
        path: "/api/draft-rule",
        group: Group::Viewer,
        command: Some("draft-rule"),
        handler: || post(draft_rule),
    },
    Route {
        method: "GET",
        path: "/api/runs",
        group: Group::Viewer,
        command: Some("runs"),
        handler: || get(runs),
    },
    Route {
        method: "GET",
        path: "/api/runs/{run}",
        group: Group::Viewer,
        command: Some("show"),
        handler: || get(show),
    },
    Route {
        method: "GET",
        path: "/api/decisions/{decision}",
        group: Group::Viewer,
        command: Some("explain"),
        handler: || get(explain),
    },
    Route {
        method: "GET",
        path: "/api/compare/{a}/{b}",
        group: Group::Viewer,
        command: Some("compare"),
        handler: || get(compare),
    },
    Route {
        method: "GET",
        path: "/api/audit",
        group: Group::Viewer,
        command: Some("audit"),
        handler: || get(audit),
    },
    Route {
        method: "GET",
        path: "/api/changes",
        group: Group::Viewer,
        command: Some("changes"),
        handler: || get(changes),
    },
    Route {
        method: "GET",
        path: "/api/sessions",
        group: Group::Viewer,
        command: Some("sessions"),
        handler: || get(sessions),
    },
    Route {
        method: "GET",
        path: "/api/sessions/{session}/events",
        group: Group::Viewer,
        command: Some("watch"),
        handler: || get(events),
    },
    Route {
        method: "GET",
        path: "/api/bridge/binding",
        group: Group::StandIn,
        command: None,
        handler: || get(bridge_binding),
    },
];

/// The largest request body the control API takes: room for any program a person or an
/// assistant writes by hand, and a bound on what one request makes the playground hold.
pub(crate) const MAX_BODY_BYTES: usize = 2 << 20;

/// What an extractor took from the request (a JSON body, a path segment, a query), or a
/// 400 in the control API's error shape that says what was wrong with it.
fn extracted<T, E: std::fmt::Display>(value: Result<T, E>) -> Result<T, Box<Response>> {
    value.map_err(|rejection| {
        Box::new(host::error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            &rejection.to_string(),
        ))
    })
}

fn answered(answer: Answer) -> Response {
    Json(answer).into_response()
}

/// An action that takes a JSON body: its answer, or the 400 for a body it does not take.
async fn act_with<T, F>(
    state: &ControlState,
    request: Result<Json<T>, JsonRejection>,
    act: impl FnOnce(Arc<Actions>, T) -> F,
) -> Response
where
    F: std::future::Future<Output = Answer>,
{
    match extracted(request) {
        Ok(Json(request)) => answered(act(Arc::clone(state.actions_arc()), request).await),
        Err(refused) => *refused,
    }
}

// ---- actions -------------------------------------------------------------------------------

async fn exec(
    State(state): State<ControlState>,
    request: Result<Json<ExecRequest>, JsonRejection>,
) -> Response {
    act_with(&state, request, |actions, request| async move {
        actions.exec(request).await
    })
    .await
}

async fn binding(State(state): State<ControlState>) -> Response {
    answered(state.actions().bind(None).await)
}

async fn bind(
    State(state): State<ControlState>,
    request: Result<Json<BindRequest>, JsonRejection>,
) -> Response {
    act_with(&state, request, |actions, request| async move {
        actions.bind(Some(request)).await
    })
    .await
}

async fn session_start(
    State(state): State<ControlState>,
    request: Result<Json<SessionStartRequest>, JsonRejection>,
) -> Response {
    act_with(&state, request, |actions, request| async move {
        actions.session_start(request).await
    })
    .await
}

async fn session_end(
    State(state): State<ControlState>,
    request: Result<Json<SessionEndRequest>, JsonRejection>,
) -> Response {
    act_with(&state, request, |actions, request| async move {
        actions.session_end(request).await
    })
    .await
}

async fn recheck(
    State(state): State<ControlState>,
    request: Result<Json<RunRequest>, JsonRejection>,
) -> Response {
    act_with(&state, request, |actions, request| async move {
        actions.recheck(request).await
    })
    .await
}

async fn test(
    State(state): State<ControlState>,
    request: Result<Json<TestRequest>, JsonRejection>,
) -> Response {
    act_with(&state, request, |actions, request| async move {
        actions.test(request).await
    })
    .await
}

async fn rerun(
    State(state): State<ControlState>,
    request: Result<Json<RunRequest>, JsonRejection>,
) -> Response {
    act_with(&state, request, |actions, request| async move {
        actions.rerun(request).await
    })
    .await
}

/// `POST /api/draft-rule`: drafts and prints a rule, for the page and the CLI alike.
/// Writing it widens the blueprint, which takes the admin token: `/api/draft-rule/write`.
async fn draft_rule(
    State(state): State<ControlState>,
    request: Result<Json<DraftRequest>, JsonRejection>,
) -> Response {
    let Json(request) = match extracted(request) {
        Ok(request) => request,
        Err(refused) => return *refused,
    };
    if request.write {
        return host::error(
            StatusCode::FORBIDDEN,
            "forbidden",
            "writing a drafted rule into the blueprint takes the admin token: POST \
             /api/draft-rule/write",
        );
    }
    answered(draft(Arc::clone(state.actions_arc()), request).await)
}

/// `POST /api/draft-rule/write`: drafts a rule and writes it into the blueprint file.
async fn draft_rule_write(
    State(state): State<ControlState>,
    request: Result<Json<DraftRequest>, JsonRejection>,
) -> Response {
    act_with(&state, request, |actions, request| async move {
        draft(
            actions,
            DraftRequest {
                write: true,
                ..request
            },
        )
        .await
    })
    .await
}

/// Drafts off the async runtime: it reads the store and the blueprint file.
async fn draft(actions: Arc<Actions>, request: DraftRequest) -> Answer {
    match tokio::task::spawn_blocking(move || actions.draft(&request)).await {
        Ok(answer) => answer,
        Err(error) => ActError::failure(format!("drafting failed: {error}")).into(),
    }
}

async fn clear(State(state): State<ControlState>) -> Response {
    answered(state.actions().clear())
}

async fn cancel(
    State(state): State<ControlState>,
    request: Result<Json<RunRequest>, JsonRejection>,
) -> Response {
    act_with(&state, request, |actions, request| async move {
        actions.cancel(&request).await
    })
    .await
}

async fn bridge_binding(State(state): State<ControlState>) -> Response {
    Json(state.actions().bridge_binding()).into_response()
}

// ---- reads, for the page -------------------------------------------------------------------

/// A read control's result, or its error as the CLI reports it, with a status to match.
async fn read_with<T: serde::Serialize + Send + 'static>(
    state: &ControlState,
    read: impl FnOnce(&Reader) -> Result<T, ReadError> + Send + 'static,
) -> Response {
    let actions = Arc::clone(state.actions_arc());
    let answered = tokio::task::spawn_blocking(move || {
        let reader = actions
            .reader()
            .map_err(|error| (error.exit, error.message))?;
        read(&reader).map_err(|error| (error.exit(), error.to_string()))
    })
    .await;
    match answered {
        Ok(Ok(result)) => Json(result).into_response(),
        Ok(Err((exit, message))) => {
            let status = if exit == EXIT_FAILURE {
                StatusCode::INTERNAL_SERVER_ERROR
            } else {
                StatusCode::NOT_FOUND
            };
            host::error(status, "read_failed", &message)
        }
        Err(error) => host::error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            &error.to_string(),
        ),
    }
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RunsParams {
    source: Option<String>,
    since: Option<String>,
    session: Option<String>,
    limit: Option<usize>,
}

async fn runs(
    State(state): State<ControlState>,
    params: Result<Query<RunsParams>, QueryRejection>,
) -> Response {
    let Query(params) = match extracted(params) {
        Ok(params) => params,
        Err(refused) => return *refused,
    };
    let since = match params.since.as_deref().map(str::parse::<Since>).transpose() {
        Ok(since) => since,
        Err(message) => return host::error(StatusCode::BAD_REQUEST, "invalid_request", &message),
    };
    let query = RunsQuery {
        source: params.source,
        since,
        session: params.session,
        limit: params.limit.unwrap_or(20),
        now_micros: super::now_micros(),
    };
    read_with(&state, move |reader| read::runs(reader, &query)).await
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct ShowParams {
    include_payloads: bool,
}

async fn show(
    State(state): State<ControlState>,
    run: Result<Path<u64>, PathRejection>,
    params: Result<Query<ShowParams>, QueryRejection>,
) -> Response {
    let (Path(run), Query(params)) = match (extracted(run), extracted(params)) {
        (Ok(run), Ok(params)) => (run, params),
        (Err(refused), _) | (_, Err(refused)) => return *refused,
    };
    read_with(&state, move |reader| {
        read::show(reader, run, params.include_payloads)
    })
    .await
}

async fn explain(
    State(state): State<ControlState>,
    decision: Result<Path<String>, PathRejection>,
) -> Response {
    let decision = match extracted(decision)
        .and_then(|Path(decision)| extracted(decision.parse::<DecisionRef>()))
    {
        Ok(decision) => decision,
        Err(refused) => return *refused,
    };
    read_with(&state, move |reader| read::explain(reader, decision)).await
}

async fn compare(
    State(state): State<ControlState>,
    runs: Result<Path<(u64, u64)>, PathRejection>,
) -> Response {
    let Path((a, b)) = match extracted(runs) {
        Ok(runs) => runs,
        Err(refused) => return *refused,
    };
    read_with(&state, move |reader| read::compare(reader, a, b)).await
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct AuditParams {
    default_only: bool,
    packages_only: bool,
}

async fn audit(
    State(state): State<ControlState>,
    params: Result<Query<AuditParams>, QueryRejection>,
) -> Response {
    let Query(params) = match extracted(params) {
        Ok(params) => params,
        Err(refused) => return *refused,
    };
    let query = AuditQuery {
        default_only: params.default_only,
        packages_only: params.packages_only,
    };
    read_with(&state, move |reader| read::audit(reader, query)).await
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct ChangesParams {
    version: Option<u64>,
}

async fn changes(
    State(state): State<ControlState>,
    params: Result<Query<ChangesParams>, QueryRejection>,
) -> Response {
    let Query(params) = match extracted(params) {
        Ok(params) => params,
        Err(refused) => return *refused,
    };
    read_with(&state, move |reader| read::changes(reader, params.version)).await
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct SessionsParams {
    limit: Option<usize>,
}

async fn sessions(
    State(state): State<ControlState>,
    params: Result<Query<SessionsParams>, QueryRejection>,
) -> Response {
    let Query(params) = match extracted(params) {
        Ok(params) => params,
        Err(refused) => return *refused,
    };
    let limit = params.limit.unwrap_or(20);
    let now = super::now_micros();
    read_with(&state, move |reader| read::sessions(reader, limit, now)).await
}

// ---- the event feed ------------------------------------------------------------------------

/// The feed's own events, besides the session's stored ones (`event` and `gap`).
pub(crate) mod feed {
    /// A run of the session finished and none is in flight.
    pub(crate) const RUN_IDLE: &str = "run-idle";
    /// The session ended and its log is read to the end; the stream closes.
    pub(crate) const SESSION_ENDED: &str = "session-ended";
    /// The session's log could not be read; the stream closes. Its data says why.
    pub(crate) const FEED_ERROR: &str = "feed-error";
}

/// What one feed follows, between reads of the log.
struct Feed {
    state: ControlState,
    session: String,
    /// Where the next read of the log starts, in bytes.
    offset: u64,
    /// The last sequence number read; events after it are next.
    cursor: u64,
    /// Events read before `resume_after` were sent on an earlier connection.
    resume_after: u64,
    /// Runs of the session that started and have not finished, by execution id, with
    /// the sequence number of their start.
    in_flight: HashMap<String, u64>,
    /// Runs that finished and whose `returned` event, appended just after, has not been
    /// read yet, with when the finish was read and its sequence number.
    returning: HashMap<String, (std::time::Instant, u64)>,
    /// Ready to send.
    pending: VecDeque<Event>,
    /// The stream ends once `pending` is sent.
    ended: bool,
    appended: tokio::sync::watch::Receiver<u64>,
    stopping: tokio::sync::watch::Receiver<bool>,
}

/// How often a quiet feed looks at whether its session is still open.
const SESSION_CHECK: std::time::Duration = std::time::Duration::from_secs(1);
/// How long a finished run's `returned` event is waited for before the session counts as
/// idle without it: a caller that went away gets none.
const RETURN_GRACE: std::time::Duration = std::time::Duration::from_millis(500);

/// `GET /api/sessions/{session}/events`: the session's event log as server-sent events,
/// `id:` its sequence number, resuming after `Last-Event-ID`. Woken by each append.
///
/// A `run-idle` event follows a batch in which a run of the session finished and
/// returned (or did not return within a moment) and none is left in flight; on a resumed
/// connection, only for a finish after the resume point. A run a crashed playground left
/// unfinished does not count as in flight. Once the session has ended (or is gone), its
/// runs are done, and its log is read to the end, a `session-ended` event closes the
/// stream; a log that cannot be read closes it with a `feed-error` event. It also ends
/// when the playground stops. A session the playground knows nothing of is a 404.
async fn events(
    State(state): State<ControlState>,
    session: Result<Path<String>, PathRejection>,
    headers: HeaderMap,
) -> Response {
    let Path(session) = match extracted(session) {
        Ok(session) => session,
        Err(refused) => return *refused,
    };
    if !session_known(&state, &session).await {
        return host::error(
            StatusCode::NOT_FOUND,
            "unknown_session",
            "no session by that id has run here or been started; list sessions with \
             `submilli playground sessions`",
        );
    }
    let resume_after = headers
        .get("last-event-id")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<u64>().ok())
        .unwrap_or(0);
    let feed = Feed {
        appended: state.actions().store.events.subscribe(),
        stopping: state.stopping(),
        state,
        session,
        offset: 0,
        cursor: 0,
        resume_after,
        in_flight: HashMap::new(),
        returning: HashMap::new(),
        pending: VecDeque::new(),
        ended: false,
    };
    let stream = futures::stream::unfold(feed, |mut feed| async move {
        let event = feed.next_event().await?;
        Some((Ok::<Event, Infallible>(event), feed))
    });
    Sse::new(stream)
        .keep_alive(KeepAlive::default())
        .into_response()
}

/// Whether the playground knows `session`: open on the server, started here, or with
/// events in the store.
async fn session_known(state: &ControlState, session: &str) -> bool {
    if matches!(
        submilli_server::record::session_variables(&state.actions().app, session).await,
        Ok(Some(_)) | Err(_)
    ) {
        return true;
    }
    let store = Arc::clone(&state.actions().store);
    let session = session.to_owned();
    tokio::task::spawn_blocking(move || {
        store.events_path(Some(&session)).exists()
            || store.session_log().is_ok_and(|log| {
                log.iter()
                    .any(|line| line.entry.session_id() == session.as_str())
            })
    })
    .await
    .unwrap_or(true)
}

impl Feed {
    /// The next event to send, or `None` when the stream ends.
    async fn next_event(&mut self) -> Option<Event> {
        loop {
            if let Some(event) = self.pending.pop_front() {
                return Some(event);
            }
            if self.ended || *self.stopping.borrow_and_update() {
                return None;
            }
            // Marked seen before the read, so an append during it wakes the wait below.
            self.appended.borrow_and_update();
            self.read().await;
            if !self.pending.is_empty() {
                continue;
            }
            if self.in_flight.is_empty() && self.returning.is_empty() && !self.session_open().await
            {
                // Whatever the session's last run appended before it ended goes first.
                self.read().await;
                if !self.ended {
                    self.pending
                        .push_back(
                            Event::default().event(feed::SESSION_ENDED).data(
                                json!({ "kind": feed::SESSION_ENDED, "session": self.session })
                                    .to_string(),
                            ),
                        );
                    self.ended = true;
                }
                continue;
            }
            // A run that finished is idle once its `returned` event is read or its grace
            // runs out, so wake for the grace.
            let wait = if self.returning.is_empty() {
                SESSION_CHECK
            } else {
                RETURN_GRACE
            };
            tokio::select! {
                () = tokio::time::sleep(wait) => {}
                changed = self.appended.changed() => {
                    if changed.is_err() {
                        return None;
                    }
                }
                changed = self.stopping.changed() => {
                    if changed.is_err() {
                        return None;
                    }
                }
            }
        }
    }

    /// Whether the session is still open. A read that fails says nothing either way, so
    /// the feed keeps following.
    async fn session_open(&self) -> bool {
        !matches!(
            submilli_server::record::session_variables(&self.state.actions().app, &self.session)
                .await,
            Ok(None)
        )
    }

    /// Reads the lines appended since the last read and queues what this client has not
    /// had, then `run-idle` when a run finished and none is left in flight. A log that
    /// cannot be read queues a `feed-error` and ends the stream.
    async fn read(&mut self) {
        let store = Arc::clone(&self.state.actions().store);
        let session = self.session.clone();
        let offset = self.offset;
        let read =
            tokio::task::spawn_blocking(move || store.read_events_from(Some(&session), offset))
                .await;
        let events = match read {
            Ok(Ok((events, offset))) => {
                self.offset = offset;
                events
            }
            Ok(Err(error)) => return self.fail(&error.to_string()),
            Err(error) => return self.fail(&error.to_string()),
        };
        let mut finished = false;
        for event in events {
            if event.session_seq <= self.cursor {
                continue;
            }
            self.cursor = event.session_seq;
            finished |= self.track(&event);
            if event.session_seq > self.resume_after {
                self.pending.push_back(sse_event(&event));
            }
        }
        finished |= self.settle();
        if finished && self.in_flight.is_empty() && self.returning.is_empty() {
            self.pending.push_back(
                Event::default()
                    .event(feed::RUN_IDLE)
                    .data(json!({ "kind": feed::RUN_IDLE, "session": self.session }).to_string()),
            );
        }
    }

    /// Follows `event`'s run from its start to its return; whether it finished a run
    /// after the resume point.
    fn track(&mut self, event: &StoredEvent) -> bool {
        let EventBody::Event(session_event) = &event.body else {
            return false;
        };
        let Some(run) = &session_event.run_id else {
            return false;
        };
        let seq = event.session_seq;
        match session_event.kind {
            EventKind::RunStarted { .. } => {
                self.in_flight.insert(run.clone(), seq);
            }
            EventKind::RunFinished { .. } => {
                self.in_flight.remove(run);
                self.returning
                    .insert(run.clone(), (std::time::Instant::now(), seq));
            }
            EventKind::Returned { .. } => {
                return self.returning.remove(run).is_some() && seq > self.resume_after;
            }
            _ => {}
        }
        false
    }

    /// Lets go of the runs that will not finish as the log tells it: a finished run whose
    /// `returned` event its grace has waited for, and a run this playground's recorder
    /// never started, which a playground that crashed left unfinished. Whether one of
    /// them finished after the resume point.
    fn settle(&mut self) -> bool {
        let resume_after = self.resume_after;
        let mut finished = false;
        self.returning.retain(|_, (at, seq)| {
            let waiting = at.elapsed() < RETURN_GRACE;
            finished |= !waiting && *seq > resume_after;
            waiting
        });
        let recorder = &self.state.actions().recorder;
        self.in_flight.retain(|run, seq| {
            let running = recorder.knows(run);
            finished |= !running && *seq > resume_after;
            running
        });
        finished
    }

    fn fail(&mut self, message: &str) {
        self.pending.push_back(
            Event::default().event(feed::FEED_ERROR).data(
                json!({ "kind": feed::FEED_ERROR, "session": self.session, "message": message })
                    .to_string(),
            ),
        );
        self.ended = true;
    }
}

fn sse_event(event: &StoredEvent) -> Event {
    let name = match event.body {
        EventBody::Event(_) => "event",
        EventBody::Gap(_) => "gap",
    };
    Event::default()
        .id(event.session_seq.to_string())
        .event(name)
        .data(serde_json::to_string(event).unwrap_or_else(|_| "null".to_owned()))
}

/// The routes by group, for the parity test and the router.
pub(crate) fn by_group(group: Group) -> impl Iterator<Item = &'static Route> {
    ROUTES.iter().filter(move |route| route.group == group)
}

/// What each CLI subcommand reaches, for the parity test: every command a route maps to.
#[cfg(test)]
pub(crate) fn commands() -> std::collections::BTreeMap<&'static str, Vec<&'static str>> {
    let mut commands: std::collections::BTreeMap<&'static str, Vec<&'static str>> =
        std::collections::BTreeMap::new();
    for route in ROUTES {
        if let Some(command) = route.command {
            commands.entry(command).or_default().push(route.path);
        }
    }
    commands
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    /// The subcommands `submilli playground` has.
    fn subcommands() -> Vec<String> {
        use clap::{Args as _, Command};
        super::super::Args::augment_args(Command::new("playground"))
            .get_subcommands()
            .map(|command| command.get_name().to_owned())
            .collect()
    }

    #[test]
    fn every_action_and_read_route_has_a_cli_subcommand() {
        let subcommands = subcommands();
        for route in ROUTES {
            match route.group {
                // The page, signing in, the nonce challenge, and the bridge's binding:
                // the CLI's counterpart to the bridge's route is `playground mcp` itself.
                Group::Public | Group::StandIn => {
                    assert!(
                        route.command.is_none(),
                        "{} {} names a command",
                        route.method,
                        route.path
                    );
                }
                Group::Admin | Group::Viewer => {
                    let command = route.command.unwrap_or_else(|| {
                        panic!("{} {} has no CLI subcommand", route.method, route.path)
                    });
                    assert!(
                        subcommands.iter().any(|name| name == command),
                        "{} {} maps to `{command}`, which `submilli playground` lacks",
                        route.method,
                        route.path
                    );
                }
            }
        }
        let mapped = commands();
        assert_eq!(mapped["watch"], ["/api/sessions/{session}/events"]);
        assert_eq!(mapped["status"], ["/api/status"]);
        assert_eq!(mapped["stop"], ["/api/stop"]);
        assert_eq!(mapped["open"], ["/api/login-codes"]);
    }

    #[test]
    fn routes_are_unique_and_the_bridge_route_is_the_stand_ins_alone() {
        let mut seen = HashSet::new();
        for route in ROUTES {
            assert!(
                seen.insert((route.method, route.path)),
                "{} {} twice",
                route.method,
                route.path
            );
        }
        let bridge: Vec<&str> = by_group(Group::StandIn).map(|route| route.path).collect();
        assert_eq!(bridge, ["/api/bridge/binding"]);
    }
}
