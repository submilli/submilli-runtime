//! How a command reaches a running playground: read the lock, check the pid,
//! and complete the nonce challenge before any credential is sent. Only a
//! listener that proves it knows the lock's start nonce ever sees the admin token.

use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

use super::control_auth::{CHALLENGE_BYTES, challenge_response};
use super::state::{Lock, StateDir, random_hex};

/// Long enough for a loaded playground to answer, short enough that a listener
/// that never answers reads as not running rather than hanging the command.
const CHALLENGE_TIMEOUT: Duration = Duration::from_secs(5);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

pub(crate) enum Probe {
    NotRunning,
    /// A lock whose pid is gone or whose listener does not answer the challenge.
    Stale,
    Running(Running),
}

pub(crate) struct Running {
    pub(crate) lock: Lock,
    agent: ureq::Agent,
    admin: String,
}

/// What the lock in `state` names, checked.
pub(crate) fn probe(state: &StateDir) -> Result<Probe> {
    let Some(lock) = state.read_lock()? else {
        return Ok(Probe::NotRunning);
    };
    if !pid_alive(lock.pid) {
        return Ok(Probe::Stale);
    }
    let agent = agent(CHALLENGE_TIMEOUT);
    if !answers_challenge(&agent, &lock) {
        return Ok(Probe::Stale);
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
    pub(crate) fn status(&self) -> Result<Value> {
        self.call("GET", "/api/status")
    }

    pub(crate) fn mint_login_code(&self) -> Result<String> {
        let body = self.call("POST", "/api/login-codes")?;
        body["code"]
            .as_str()
            .map(str::to_owned)
            .context("the playground returned no login code")
    }

    pub(crate) fn stop(&self) -> Result<()> {
        self.call("POST", "/api/stop").map(|_| ())
    }

    /// Whether the server listener answers its health probe, which needs no token.
    pub(crate) fn server_healthy(&self) -> bool {
        let url = format!("http://127.0.0.1:{}/healthz", self.lock.server_port);
        self.agent
            .get(&url)
            .call()
            .is_ok_and(|response| response.status().is_success())
    }

    fn call(&self, method: &str, path: &str) -> Result<Value> {
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
            .read_json::<Value>()
            .unwrap_or(Value::Null);
        if !status.is_success() {
            let message = body["message"]
                .as_str()
                .map_or_else(|| format!("HTTP {}", status.as_u16()), str::to_owned);
            bail!("the playground refused {method} {path}: {message}");
        }
        Ok(body)
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

/// Whether a process with this pid exists. A pid this user may not signal still
/// exists, so it counts as alive; the nonce challenge tells whether it is ours.
pub(crate) fn pid_alive(pid: u32) -> bool {
    let Ok(pid) = libc::pid_t::try_from(pid) else {
        return false;
    };
    if pid <= 0 {
        return false;
    }
    // SAFETY: kill with signal 0 only checks for the process; it takes scalar
    // arguments and sends nothing.
    let result = unsafe { libc::kill(pid, 0) };
    result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}
