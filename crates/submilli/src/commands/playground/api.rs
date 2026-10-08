//! The control API: every route the control listener serves, in one table the router is
//! built from, by who may call it and the CLI subcommand that does the same.
//!
//! The actions answer with an [`Answer`]: the result the CLI prints with `--json`, its
//! text form, and the exit status, so the CLI and the page see one result. The reads
//! answer with the read result itself, for the page; the CLI reads the store directly.
//! The event feed serves a session's event log as server-sent events.

use std::collections::{HashSet, VecDeque};
use std::convert::Infallible;
use std::sync::Arc;

use axum::Json;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{MethodRouter, get, post};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::json;
use submilli_server::record::EventKind;

use super::controls::act::{
    Answer, BindRequest, DraftRequest, ExecRequest, RunRequest, SessionEndRequest,
    SessionStartRequest, TestRequest,
};
use super::controls::read::{self, AuditQuery, RunsQuery, Since};
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

/// A JSON body, or a 400 that says what was wrong with it (an unknown field, a wrong
/// type), in the control API's error shape.
fn body<T: DeserializeOwned>(body: Result<Json<T>, JsonRejection>) -> Result<T, Box<Response>> {
    body.map(|Json(body)| body).map_err(|rejection| {
        Box::new(host::error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            &rejection.body_text(),
        ))
    })
}

fn answered(answer: Answer) -> Response {
    Json(answer).into_response()
}

// ---- actions -------------------------------------------------------------------------------

async fn exec(
    State(state): State<ControlState>,
    request: Result<Json<ExecRequest>, JsonRejection>,
) -> Response {
    match body(request) {
        Ok(request) => answered(state.actions().exec(request).await),
        Err(refused) => *refused,
    }
}

async fn binding(State(state): State<ControlState>) -> Response {
    answered(state.actions().bind(None).await)
}

async fn bind(
    State(state): State<ControlState>,
    request: Result<Json<BindRequest>, JsonRejection>,
) -> Response {
    match body(request) {
        Ok(request) => answered(state.actions().bind(Some(request)).await),
        Err(refused) => *refused,
    }
}

async fn session_start(
    State(state): State<ControlState>,
    request: Result<Json<SessionStartRequest>, JsonRejection>,
) -> Response {
    match body(request) {
        Ok(request) => answered(state.actions().session_start(request).await),
        Err(refused) => *refused,
    }
}

async fn session_end(
    State(state): State<ControlState>,
    request: Result<Json<SessionEndRequest>, JsonRejection>,
) -> Response {
    match body(request) {
        Ok(request) => answered(state.actions().session_end(request).await),
        Err(refused) => *refused,
    }
}

async fn recheck(
    State(state): State<ControlState>,
    request: Result<Json<RunRequest>, JsonRejection>,
) -> Response {
    match body(request) {
        Ok(request) => answered(state.actions().recheck(request).await),
        Err(refused) => *refused,
    }
}

async fn test(
    State(state): State<ControlState>,
    request: Result<Json<TestRequest>, JsonRejection>,
) -> Response {
    match body(request) {
        Ok(request) => answered(state.actions().test(request).await),
        Err(refused) => *refused,
    }
}

async fn rerun(
    State(state): State<ControlState>,
    request: Result<Json<RunRequest>, JsonRejection>,
) -> Response {
    match body(request) {
        Ok(request) => answered(state.actions().rerun(request).await),
        Err(refused) => *refused,
    }
}

async fn draft_rule(
    State(state): State<ControlState>,
    request: Result<Json<DraftRequest>, JsonRejection>,
) -> Response {
    let request = match body(request) {
        Ok(request) => request,
        Err(refused) => return *refused,
    };
    let actions = Arc::clone(state.actions_arc());
    match tokio::task::spawn_blocking(move || actions.draft(&request)).await {
        Ok(answer) => answered(answer),
        Err(error) => host::error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            &error.to_string(),
        ),
    }
}

async fn clear(State(state): State<ControlState>) -> Response {
    answered(state.actions().clear())
}

async fn cancel(
    State(state): State<ControlState>,
    request: Result<Json<RunRequest>, JsonRejection>,
) -> Response {
    match body(request) {
        Ok(request) => answered(state.actions().cancel(&request)),
        Err(refused) => *refused,
    }
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
            let status = if exit == 1 {
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

async fn runs(State(state): State<ControlState>, Query(params): Query<RunsParams>) -> Response {
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
    Path(run): Path<u64>,
    Query(params): Query<ShowParams>,
) -> Response {
    read_with(&state, move |reader| {
        read::show(reader, run, params.include_payloads)
    })
    .await
}

async fn explain(State(state): State<ControlState>, Path(decision): Path<String>) -> Response {
    let decision = match decision.parse::<DecisionRef>() {
        Ok(decision) => decision,
        Err(message) => return host::error(StatusCode::BAD_REQUEST, "invalid_request", &message),
    };
    read_with(&state, move |reader| read::explain(reader, decision)).await
}

async fn compare(State(state): State<ControlState>, Path((a, b)): Path<(u64, u64)>) -> Response {
    read_with(&state, move |reader| read::compare(reader, a, b)).await
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct AuditParams {
    default_only: bool,
    packages_only: bool,
}

async fn audit(State(state): State<ControlState>, Query(params): Query<AuditParams>) -> Response {
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
    Query(params): Query<ChangesParams>,
) -> Response {
    read_with(&state, move |reader| read::changes(reader, params.version)).await
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct SessionsParams {
    limit: Option<usize>,
}

async fn sessions(
    State(state): State<ControlState>,
    Query(params): Query<SessionsParams>,
) -> Response {
    let limit = params.limit.unwrap_or(20);
    read_with(&state, move |reader| read::sessions(reader, limit)).await
}

// ---- the event feed ------------------------------------------------------------------------

/// What one feed follows, between reads of the log.
struct Feed {
    state: ControlState,
    session: String,
    /// The last sequence number read; events after it are next.
    cursor: u64,
    /// Events read before `resume_after` were sent on an earlier connection.
    resume_after: u64,
    /// Runs of the session that started and have not finished, by execution id.
    in_flight: HashSet<String>,
    /// Ready to send.
    pending: VecDeque<Event>,
    appended: tokio::sync::watch::Receiver<u64>,
    stopping: tokio::sync::watch::Receiver<bool>,
}

/// `GET /api/sessions/{session}/events`: the session's event log as server-sent events,
/// `id:` its sequence number, resuming after `Last-Event-ID`. Woken by each append; a
/// `run-idle` event follows a batch in which a run of the session finished and none is
/// left in flight. The stream ends when the playground stops.
async fn events(
    State(state): State<ControlState>,
    Path(session): Path<String>,
    headers: HeaderMap,
) -> Response {
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
        cursor: 0,
        resume_after,
        in_flight: HashSet::new(),
        pending: VecDeque::new(),
    };
    let stream = futures::stream::unfold(feed, |mut feed| async move {
        loop {
            if let Some(event) = feed.pending.pop_front() {
                return Some((Ok::<Event, Infallible>(event), feed));
            }
            if *feed.stopping.borrow_and_update() {
                return None;
            }
            // Marked seen before the read, so an append during it wakes the wait below.
            feed.appended.borrow_and_update();
            if !feed.read().await {
                return None;
            }
            if !feed.pending.is_empty() {
                continue;
            }
            tokio::select! {
                changed = feed.appended.changed() => {
                    if changed.is_err() {
                        return None;
                    }
                }
                changed = feed.stopping.changed() => {
                    if changed.is_err() {
                        return None;
                    }
                }
            }
        }
    });
    Sse::new(stream)
        .keep_alive(KeepAlive::default())
        .into_response()
}

impl Feed {
    /// Reads the events appended since the cursor and queues what this client has not
    /// had. `false` when the log cannot be read.
    async fn read(&mut self) -> bool {
        let store = Arc::clone(&self.state.actions().store);
        let session = self.session.clone();
        let after = self.cursor;
        let read =
            tokio::task::spawn_blocking(move || store.read_events_after(Some(&session), after))
                .await;
        let Ok(Ok(events)) = read else {
            return false;
        };
        let mut finished = false;
        for event in events {
            self.cursor = self.cursor.max(event.session_seq);
            if let EventBody::Event(session_event) = &event.body
                && let Some(run) = &session_event.run_id
            {
                match session_event.kind {
                    EventKind::RunStarted { .. } => {
                        self.in_flight.insert(run.clone());
                    }
                    EventKind::RunFinished { .. } => {
                        self.in_flight.remove(run);
                        finished = true;
                    }
                    _ => {}
                }
            }
            if event.session_seq > self.resume_after {
                self.pending.push_back(sse_event(&event));
            }
        }
        if finished && self.in_flight.is_empty() {
            self.pending.push_back(
                Event::default()
                    .event("run-idle")
                    .data(json!({ "kind": "run-idle", "session": self.session }).to_string()),
            );
        }
        true
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
