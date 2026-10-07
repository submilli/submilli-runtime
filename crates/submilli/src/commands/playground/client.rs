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
        let status = response.status();
        let body = response
            .body_mut()
            .with_config()
            .limit(1 << 20)
            .read_json::<Value>();
        if !status.is_success() {
            let message = body
                .ok()
                .and_then(|body| body["message"].as_str().map(str::to_owned))
                .unwrap_or_else(|| format!("HTTP {}", status.as_u16()));
            bail!("the playground refused {method} {path}: {message}");
        }
        let body = body
            .with_context(|| format!("the playground's answer to {method} {path} is not JSON"))?;
        serde_json::from_value(body)
            .with_context(|| format!("the playground's answer to {method} {path} is malformed"))
    }
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
