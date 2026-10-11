//! How a command reaches a running playground: read the instance record, check the
//! instance lock, and complete the nonce challenge before any credential is sent.
//! Only a listener that proves it knows the record's start nonce ever sees the
//! admin token.

use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use super::control_auth::{CHALLENGE_BYTES, challenge_response};
use super::host::Status;
use super::state::{Holder, InstanceRecord, StateDir, random_hex};

const NOT_A_STATUS: &str = "the playground's answer to GET /api/status is not a playground status";

/// Long enough for a loaded playground to answer, short enough that a listener
/// that never answers is reported rather than hanging the command.
const CHALLENGE_TIMEOUT: Duration = Duration::from_secs(5);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

pub(crate) enum Probe {
    NotRunning,
    /// A record no serving process holds the instance lock for: what it names is gone.
    Stale(InstanceRecord),
    /// A process serves this project but cannot be reached now: it is still
    /// starting, or it did not answer the challenge in time. Never replaced.
    Busy(Busy),
    Running(Running),
}

pub(crate) enum Busy {
    /// The instance lock is held but no record names an instance yet.
    Starting(Holder),
    /// The instance is draining and will exit; a start says so and exits 1.
    Stopping { pid: Option<u32> },
    /// The record's listener did not answer the challenge, or answered it wrong.
    NotAnswering { pid: u32 },
}

impl Busy {
    /// A running instance that answered that it is draining.
    pub(crate) fn stopping(pid: u32) -> Self {
        Self::Stopping { pid: Some(pid) }
    }

    pub(crate) fn pid(&self) -> Option<u32> {
        match self {
            Self::Starting(holder) => holder.pid(),
            Self::Stopping { pid } => *pid,
            Self::NotAnswering { pid } => Some(*pid),
        }
    }

    pub(crate) fn is_stopping(&self) -> bool {
        matches!(self, Self::Stopping { .. })
    }

    pub(crate) fn message(&self) -> String {
        let pid = self
            .pid()
            .map_or_else(String::new, |pid| format!(" (pid {pid})"));
        match self {
            Self::Starting(holder) if holder.pid_from_kernel.is_none() => format!(
                "a Submilli playground{pid} is starting for this project; the system does not \
                 say which process holds its instance lock (another PID namespace or a network \
                 filesystem), so `submilli playground stop` sends it no signal. Try again in a \
                 moment, or end the process yourself"
            ),
            Self::Starting(_) => format!(
                "a Submilli playground{pid} is starting for this project; try again in a \
                 moment, or end it with `submilli playground stop`"
            ),
            Self::Stopping { .. } => format!(
                "a Submilli playground{pid} is stopping for this project; try again in a \
                 moment"
            ),
            Self::NotAnswering { .. } => format!(
                "a Submilli playground{pid} serves this project but did not answer its \
                 control listener within {}s; it may be busy. Try again, or end the process \
                 if it is stuck",
                CHALLENGE_TIMEOUT.as_secs()
            ),
        }
    }
}

/// Defaults like [`Status`], so a body without a page is refused as no status.
#[derive(Default, serde::Deserialize)]
#[serde(default)]
pub(crate) struct Page {
    pub(crate) url: String,
    pub(crate) stopping: bool,
}

pub(crate) struct Running {
    pub(crate) record: InstanceRecord,
    agent: ureq::Agent,
    admin: String,
}

/// The playground answered a control request with an error status.
#[derive(Debug)]
pub(crate) struct Refused {
    pub(crate) status: u16,
    /// The playground's own message, or the status when it gave none.
    pub(crate) message: String,
    request: String,
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "the playground refused {}: {}",
            self.request, self.message
        )
    }
}

impl std::error::Error for Refused {}

/// A control request that got no answer: the connection failed or broke, as it does when
/// the playground stops.
#[derive(Debug)]
pub(crate) struct Unanswered {
    url: String,
    source: ureq::Error,
}

impl std::fmt::Display for Unanswered {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "calling the playground at {}: {}", self.url, self.source)
    }
}

impl std::error::Error for Unanswered {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

/// One server-sent event of a control feed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct FeedEvent {
    /// `message` when the event names none.
    pub(crate) name: String,
    pub(crate) id: Option<String>,
    pub(crate) data: String,
}

/// What the instance record in `state` names, checked. Only a process holding the
/// instance lock serves the project, so a record without one is stale however its
/// pid and port look, and one with it is never stale, however slowly it answers.
pub(crate) fn probe(state: &StateDir) -> Result<Probe> {
    // The record before the instance lock: an instance takes the instance lock
    // before it writes its record, so a record read here and no holder after means
    // it is gone.
    let record = state.read_record()?;
    let Some(holder) = state.instance_holder()? else {
        return Ok(record.map_or(Probe::NotRunning, Probe::Stale));
    };
    if holder.stopping {
        return Ok(Probe::Busy(Busy::Stopping { pid: holder.pid() }));
    }
    let Some(record) = record else {
        return Ok(Probe::Busy(Busy::Starting(holder)));
    };
    let agent = agent(CHALLENGE_TIMEOUT);
    if !answers_challenge(&agent, &record) {
        return Ok(Probe::Busy(Busy::NotAnswering { pid: record.pid }));
    }
    // Read only now: a listener that failed the challenge never gets near it.
    let admin = state.admin_token()?;
    Ok(Probe::Running(Running {
        record,
        agent: self::agent(REQUEST_TIMEOUT),
        admin,
    }))
}

fn answers_challenge(agent: &ureq::Agent, record: &InstanceRecord) -> bool {
    let Ok(challenge) = random_hex(CHALLENGE_BYTES) else {
        return false;
    };
    let Some(expected) = challenge_response(&record.nonce, &challenge) else {
        return false;
    };
    let url = format!("{}/api/challenge", base(record));
    let Ok(mut response) = agent
        .post(&url)
        .send_json(json!({ "challenge": challenge }))
    else {
        return false;
    };
    if !response.status().is_success() {
        return false;
    }
    response
        .body_mut()
        .with_config()
        .limit(64 * 1024)
        .read_json::<Value>()
        .ok()
        .and_then(|body| body["response"].as_str().map(str::to_owned))
        .is_some_and(|answer| answer == expected)
}

impl Running {
    /// The instance's status. Every field defaults when absent, so a success whose
    /// JSON names no page or another process is refused here, not reported.
    pub(crate) fn status(&self) -> Result<Status> {
        let status: Status = self.call("GET", "/api/status")?;
        if status.description.url.is_empty() || status.description.pid != self.record.pid {
            bail!(NOT_A_STATUS);
        }
        Ok(status)
    }

    /// Only the page's address, and whether the instance is stopping: what `open`
    /// needs, from an instance of any version.
    pub(crate) fn page(&self) -> Result<Page> {
        let page: Page = self.call("GET", "/api/status")?;
        if page.url.is_empty() {
            bail!(NOT_A_STATUS);
        }
        Ok(page)
    }

    pub(crate) fn mint_login_code(&self) -> Result<String> {
        #[derive(serde::Deserialize)]
        struct Minted {
            code: String,
        }
        let minted: Minted = self.call("POST", "/api/login-codes")?;
        Ok(minted.code)
    }

    pub(crate) fn stop(&self) -> Result<()> {
        self.call::<Value>("POST", "/api/stop").map(|_| ())
    }

    /// Whether the server listener answers its health probe, which needs no token.
    pub(crate) fn server_healthy(&self) -> bool {
        let url = format!("http://127.0.0.1:{}/healthz", self.record.server_port);
        self.agent
            .get(&url)
            .call()
            .is_ok_and(|response| response.status().is_success())
    }

    /// An action's answer. Actions run programs, so the request has no overall deadline
    /// (a run ends by its own limits); only connecting is bounded.
    pub(crate) fn send<T: DeserializeOwned>(
        &self,
        method: &str,
        path: &str,
        body: Option<&Value>,
    ) -> Result<T> {
        let url = format!("{}{path}", base(&self.record));
        let builder = ureq::http::Request::builder()
            .method(method)
            .uri(&url)
            .header("authorization", format!("Bearer {}", self.admin))
            .header("content-type", "application/json");
        let request = builder
            .body(body.map_or_else(String::new, Value::to_string))
            .context("building a control request")?;
        let mut response = long_agent().run(request).map_err(|source| Unanswered {
            url: url.clone(),
            source,
        })?;
        decode(method, path, url, &mut response)
    }

    /// Follows a server-sent event feed, handing each event to `each` until it returns
    /// `false` or the feed ends (the playground stopped). `last_event_id` resumes after
    /// that event.
    pub(crate) fn follow(
        &self,
        path: &str,
        last_event_id: Option<&str>,
        mut each: impl FnMut(FeedEvent) -> bool,
    ) -> Result<()> {
        use std::io::BufRead as _;
        let url = format!("{}{path}", base(&self.record));
        let mut builder = ureq::http::Request::builder()
            .method("GET")
            .uri(&url)
            .header("authorization", format!("Bearer {}", self.admin))
            .header("accept", "text/event-stream");
        if let Some(id) = last_event_id {
            builder = builder.header("last-event-id", id);
        }
        let request = builder.body(()).context("building a control request")?;
        let mut response = long_agent().run(request).map_err(|source| Unanswered {
            url: url.clone(),
            source,
        })?;
        if !response.status().is_success() {
            return decode::<Value>("GET", path, url, &mut response).map(drop);
        }
        let reader =
            std::io::BufReader::new(response.body_mut().with_config().limit(u64::MAX).reader());
        let mut event = FeedEvent::default();
        for line in reader.lines() {
            // An error here is the playground closing the stream as it stopped.
            let Ok(line) = line else {
                return Ok(());
            };
            if line.is_empty() {
                let done = std::mem::take(&mut event);
                if done.data.is_empty() && done.name.is_empty() {
                    continue;
                }
                let name = if done.name.is_empty() {
                    "message".to_owned()
                } else {
                    done.name.clone()
                };
                if !each(FeedEvent { name, ..done }) {
                    return Ok(());
                }
                continue;
            }
            if line.starts_with(':') {
                continue;
            }
            let (field, value) = line.split_once(':').unwrap_or((line.as_str(), ""));
            let value = value.strip_prefix(' ').unwrap_or(value);
            match field {
                "event" => event.name = value.to_owned(),
                "id" => event.id = Some(value.to_owned()),
                "data" => {
                    if !event.data.is_empty() {
                        event.data.push('\n');
                    }
                    event.data.push_str(value);
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// A control call's answer. A success whose body is not what the route returns
    /// is an error, never an empty answer.
    fn call<T: DeserializeOwned>(&self, method: &str, path: &str) -> Result<T> {
        let url = format!("{}{path}", base(&self.record));
        let request = ureq::http::Request::builder()
            .method(method)
            .uri(&url)
            .header("authorization", format!("Bearer {}", self.admin))
            .body(())
            .context("building a control request")?;
        let mut response = self
            .agent
            .run(request)
            .with_context(|| format!("calling the playground at {url}"))?;
        decode(method, path, url, &mut response)
    }
}

/// A control response's JSON. A refusal is a [`Refused`] error with the playground's
/// message.
fn decode<T: DeserializeOwned>(
    method: &str,
    path: &str,
    url: String,
    response: &mut ureq::http::Response<ureq::Body>,
) -> Result<T> {
    let status = response.status();
    let body = response
        .body_mut()
        .with_config()
        .limit(16 << 20)
        .read_json::<Value>();
    if !status.is_success() {
        let message = body
            .ok()
            .and_then(|body| body["message"].as_str().map(str::to_owned))
            .unwrap_or_else(|| format!("HTTP {}", status.as_u16()));
        return Err(Refused {
            status: status.as_u16(),
            message,
            request: format!("{method} {path}"),
        }
        .into());
    }
    let body = match body {
        Ok(body) => body,
        Err(ureq::Error::Json(error)) if error.is_syntax() || error.is_data() => {
            return Err(anyhow::Error::new(error).context(format!(
                "the playground's answer to {method} {path} is not JSON"
            )));
        }
        // The answer stopped short: the connection broke, as it does when the playground
        // stops while answering.
        Err(source) => return Err(Unanswered { url, source }.into()),
    };
    serde_json::from_value(body)
        .with_context(|| format!("the playground's answer to {method} {path} is malformed"))
}

/// An agent for actions and feeds: they last as long as a run does.
fn long_agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .max_redirects(0)
        .http_status_as_error(false)
        .timeout_connect(Some(CHALLENGE_TIMEOUT))
        .build()
        .into()
}

fn base(record: &InstanceRecord) -> String {
    format!("http://127.0.0.1:{}", record.control_port)
}

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .max_redirects(0)
        .http_status_as_error(false)
        .timeout_global(Some(timeout))
        .build()
        .into()
}

#[cfg(test)]
mod tests {
    use std::io::{Read as _, Write as _};

    use super::*;

    /// A playground stand-in on loopback that answers one request with `response`, then
    /// closes the connection.
    fn answering(response: &'static str) -> Running {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut head = Vec::new();
            let mut byte = [0_u8; 1];
            while !head.ends_with(b"\r\n\r\n") && stream.read(&mut byte).unwrap_or(0) == 1 {
                head.push(byte[0]);
            }
            let _ = stream.write_all(response.as_bytes());
        });
        Running {
            record: InstanceRecord {
                pid: 0,
                control_port: port,
                server_port: 0,
                nonce: String::new(),
            },
            agent: long_agent(),
            admin: "token".into(),
        }
    }

    #[test]
    fn an_answer_cut_off_mid_body_is_a_lost_connection() {
        let running =
            answering("HTTP/1.1 200 OK\r\ncontent-length: 100\r\n\r\n{\"kind\": \"show\"");
        let error = running
            .send::<Value>("POST", "/api/exec", None)
            .unwrap_err();
        assert!(error.downcast_ref::<Unanswered>().is_some(), "{error:#}");
    }

    #[test]
    fn a_whole_answer_that_is_not_json_says_so() {
        let running = answering("HTTP/1.1 200 OK\r\ncontent-length: 5\r\n\r\nhello");
        let error = running
            .send::<Value>("POST", "/api/exec", None)
            .unwrap_err();
        assert!(error.downcast_ref::<Unanswered>().is_none(), "{error:#}");
        assert!(format!("{error:#}").contains("is not JSON"), "{error:#}");
    }
}
