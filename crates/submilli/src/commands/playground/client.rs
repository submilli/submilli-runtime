//! How a command reaches a running playground: read the lock, check the instance
//! lock, and complete the nonce challenge before any credential is sent. Only a
//! listener that proves it knows the lock's start nonce ever sees the admin token.

use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use super::control_auth::{CHALLENGE_BYTES, challenge_response};
use super::host::Status;
use super::state::{Lock, StateDir, random_hex};

/// Long enough for a loaded playground to answer, short enough that a listener
/// that never answers is reported rather than hanging the command.
const CHALLENGE_TIMEOUT: Duration = Duration::from_secs(5);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

pub(crate) enum Probe {
    NotRunning,
    /// A lock no serving process holds: what it names is gone.
    Stale(Lock),
    /// A process serves this project but cannot be reached now: it is still
    /// starting, or it did not answer the challenge in time. Never replaced.
    Busy(Busy),
    Running(Running),
}

pub(crate) enum Busy {
    /// The instance lock is held but no lock names an instance yet.
    Starting,
    /// The lock's listener did not answer the challenge, or answered it wrong.
    NotAnswering { pid: u32 },
}

impl Busy {
    pub(crate) fn pid(&self) -> Option<u32> {
        match self {
            Self::Starting => None,
            Self::NotAnswering { pid } => Some(*pid),
        }
    }

    pub(crate) fn message(&self) -> String {
        match self {
            Self::Starting => "a Submilli playground is starting for this project; try again \
                               in a moment"
                .to_owned(),
            Self::NotAnswering { pid } => format!(
                "a Submilli playground (pid {pid}) serves this project but did not answer its \
                 control listener within {}s; it may be busy. Try again, or end the process \
                 if it is stuck",
                CHALLENGE_TIMEOUT.as_secs()
            ),
        }
    }
}

pub(crate) struct Running {
    pub(crate) lock: Lock,
    agent: ureq::Agent,
    admin: String,
}

/// What the lock in `state` names, checked. Only a process holding the instance
/// lock serves the project, so a lock without one is stale however its pid and
/// port look, and one with it is never stale, however slowly it answers.
pub(crate) fn probe(state: &StateDir) -> Result<Probe> {
    // The lock before the instance lock: an instance takes the instance lock before
    // it writes its lock, so a lock read here and no holder after means it is gone.
    let lock = state.read_lock()?;
    let held = state.instance_held()?;
    let Some(lock) = lock else {
        return Ok(if held {
            Probe::Busy(Busy::Starting)
        } else {
            Probe::NotRunning
        });
    };
    if !held {
        return Ok(Probe::Stale(lock));
    }
    let agent = agent(CHALLENGE_TIMEOUT);
    if !answers_challenge(&agent, &lock) {
        return Ok(Probe::Busy(Busy::NotAnswering { pid: lock.pid }));
    }
    // Read only now: a listener that failed the challenge never gets near it.
    let admin = state.admin_token()?;
    Ok(Probe::Running(Running {
        lock,
        agent: self::agent(REQUEST_TIMEOUT),
        admin,
    }))
}

fn answers_challenge(agent: &ureq::Agent, lock: &Lock) -> bool {
    let Ok(challenge) = random_hex(CHALLENGE_BYTES) else {
        return false;
    };
    let Some(expected) = challenge_response(&lock.nonce, &challenge) else {
        return false;
    };
    let url = format!("{}/api/challenge", base(lock));
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
    pub(crate) fn status(&self) -> Result<Status> {
        self.call("GET", "/api/status")
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
        let url = format!("http://127.0.0.1:{}/healthz", self.lock.server_port);
        self.agent
            .get(&url)
            .call()
            .is_ok_and(|response| response.status().is_success())
    }

    /// A control call's answer. A success whose body is not what the route returns
    /// is an error, never an empty answer.
    fn call<T: DeserializeOwned>(&self, method: &str, path: &str) -> Result<T> {
        let url = format!("{}{path}", base(&self.lock));
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

fn base(lock: &Lock) -> String {
    format!("http://127.0.0.1:{}", lock.control_port)
}

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .max_redirects(0)
        .http_status_as_error(false)
        .timeout_global(Some(timeout))
        .build()
        .into()
}
