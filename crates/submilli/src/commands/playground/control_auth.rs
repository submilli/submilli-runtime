//! Who may use the control listener, and the checks that need no credential.
//!
//! - The CLI presents the admin token, checked with the server's own digest
//!   comparison ([`authenticate`](super::host::auth::authenticate)).
//! - The page presents a browser session token, which it got by exchanging a
//!   single-use login code from a link. Session digests and absolute expiries survive
//!   a restart in private project state; login codes remain in memory. An explicit
//!   stop revokes saved sessions.
//! - The nonce challenge proves the listener belongs to the instance the lock
//!   names before any command sends a credential to it.
//!
//! Expiry is checked against both clocks ([`Now`]): the monotonic one stops while
//! the machine sleeps, so a code minted before a night's sleep would otherwise
//! still work in the morning.

use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

use super::host::auth::{self, ApiToken};
use axum::http::HeaderMap;
use axum::http::header::{HOST, ORIGIN};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

use super::state::{ADMIN_TOKEN, hex, random_hex};

/// How long a login code from a link stays usable.
pub(crate) const LOGIN_CODE_TTL: Duration = Duration::from_secs(5 * 60);
/// How long a browser session lasts without a new login.
pub(crate) const SESSION_TTL: Duration = Duration::from_secs(12 * 60 * 60);
/// Failed login-code exchanges allowed per [`FAILURE_WINDOW`] before every
/// exchange is refused until the window moves on.
const MAX_FAILURES: usize = 20;
const FAILURE_WINDOW: Duration = Duration::from_secs(60);
/// Outstanding login codes kept at once; minting another drops the oldest.
const MAX_CODES: usize = 32;
/// Random bytes in a login code and a session token: 256 bits, above KTD8's floor.
const SECRET_BYTES: usize = 32;
/// The challenge a client sends is this many random bytes, hex-encoded.
pub(crate) const CHALLENGE_BYTES: usize = 32;
const CHALLENGE_CONTEXT: &[u8] = b"submilli-playground-challenge/1\0";

/// Who a control request comes from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Caller {
    Admin,
    Browser,
    /// One of the playground's other tokens, which no control route accepts yet.
    Token(String),
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum LoginRefusal {
    Invalid,
    RateLimited,
    Persistence,
}

pub(crate) struct ControlAuth {
    tokens: Vec<ApiToken>,
    session_file: Option<PathBuf>,
    // Poison means a panic interrupted an update of the code or session tables;
    // AGENTS.md permits the poisoned-lock panic rather than trusting partial state.
    secrets: Mutex<Secrets>,
}

/// The current time on both clocks.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Now {
    monotonic: Instant,
    wall: SystemTime,
}

impl Now {
    pub(crate) fn current() -> Self {
        Self {
            monotonic: Instant::now(),
            wall: SystemTime::now(),
        }
    }

    /// `ttl` from now on both clocks; `None` when either cannot represent it.
    fn plus(self, ttl: Duration) -> Option<Deadline> {
        Some(Deadline {
            monotonic: self.monotonic.checked_add(ttl)?,
            wall: self.wall.checked_add(ttl)?,
        })
    }

    #[cfg(test)]
    fn later(self, monotonic: Duration, wall: Duration) -> Self {
        Self {
            monotonic: self.monotonic + monotonic,
            wall: self.wall + wall,
        }
    }
}

/// When a code or session expires: as soon as either clock says so.
#[derive(Clone, Copy, Debug)]
struct Deadline {
    monotonic: Instant,
    wall: SystemTime,
}

impl Deadline {
    fn live_at(self, now: Now) -> bool {
        now.monotonic < self.monotonic && now.wall < self.wall
    }
}

/// Sync the directory entry too: a successful revoke must survive a machine crash.
fn write_sessions(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    super::state::write_private(path, bytes)?;
    let parent = path
        .parent()
        .context("browser sessions file has no parent")?;
    std::fs::File::open(parent)?.sync_all()?;
    Ok(())
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedSessions {
    version: u32,
    sessions: Vec<SavedSession>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedSession {
    digest: [u8; 32],
    expires_at: SystemTime,
}

#[derive(Default)]
struct Secrets {
    revoked: bool,
    /// Digest of an unused login code, and when it expires, oldest first.
    codes: VecDeque<([u8; 32], Deadline)>,
    /// Digest of a browser session token, and when it expires.
    sessions: HashMap<[u8; 32], Deadline>,
    /// When recent exchanges failed, for the rate limit. Monotonic only: a sleep
    /// that stretches the window only makes the limit stricter.
    failures: VecDeque<Instant>,
}

impl ControlAuth {
    pub(crate) fn new(tokens: Vec<ApiToken>) -> Self {
        Self {
            tokens,
            session_file: None,
            secrets: Mutex::new(Secrets::default()),
        }
    }

    /// Restore only this project's browser sessions. Login codes never survive startup.
    pub(crate) fn open(tokens: Vec<ApiToken>, path: PathBuf, now: Now) -> anyhow::Result<Self> {
        let mut auth = Self::new(tokens);
        if let Some(text) = super::state::read_private_bounded(&path, 1024 * 1024)? {
            let saved: SavedSessions =
                serde_json::from_str(&text).context("saved browser sessions are invalid")?;
            if saved.version != 1 {
                bail!("unsupported saved browser sessions version");
            }
            let mut secrets = auth.lock();
            for session in saved.sessions {
                let Ok(remaining) = session.expires_at.duration_since(now.wall) else {
                    continue;
                };
                // A backwards wall-clock jump cannot grant more than the original lifetime.
                if remaining.is_zero() || remaining > SESSION_TTL {
                    continue;
                }
                if let Some(monotonic) = now.monotonic.checked_add(remaining) {
                    secrets.sessions.insert(
                        session.digest,
                        Deadline {
                            monotonic,
                            wall: session.expires_at,
                        },
                    );
                }
            }
        }
        auth.session_file = Some(path);
        Ok(auth)
    }

    fn save_sessions(&self, sessions: &HashMap<[u8; 32], Deadline>) -> anyhow::Result<()> {
        let Some(path) = &self.session_file else {
            return Ok(());
        };
        let saved = SavedSessions {
            version: 1,
            sessions: sessions
                .iter()
                .map(|(digest, deadline)| SavedSession {
                    digest: *digest,
                    expires_at: deadline.wall,
                })
                .collect(),
        };
        let bytes = serde_json::to_vec(&saved)?;
        if bytes.len() > 1024 * 1024 {
            bail!("saved browser sessions exceed the size limit");
        }
        write_sessions(path, &bytes)
    }

    /// Revoke persisted sessions before acknowledging an explicit stop.
    pub(crate) fn revoke_sessions(&self) -> anyhow::Result<()> {
        let mut secrets = self.lock();
        self.save_sessions(&HashMap::new())?;
        secrets.revoked = true;
        secrets.codes.clear();
        secrets.sessions.clear();
        Ok(())
    }

    /// Also let an explicit stop revoke sessions after a crash left no process running.
    pub(crate) fn revoke_saved(path: &Path) -> anyhow::Result<()> {
        if super::state::read_private_bounded(path, 1024 * 1024)?.is_some() {
            write_sessions(
                path,
                &serde_json::to_vec(&SavedSessions {
                    version: 1,
                    sessions: Vec::new(),
                })?,
            )?;
        }
        Ok(())
    }

    /// A new single-use login code.
    pub(crate) fn mint_login_code(&self, now: Now) -> anyhow::Result<String> {
        let code = random_hex(SECRET_BYTES)?;
        let expires = now
            .plus(LOGIN_CODE_TTL)
            .ok_or_else(|| anyhow::anyhow!("the clock cannot represent a login code's expiry"))?;
        let mut secrets = self.lock();
        if secrets.revoked {
            bail!("the playground is stopping");
        }
        secrets.codes.retain(|(_, expiry)| expiry.live_at(now));
        while secrets.codes.len() >= MAX_CODES {
            secrets.codes.pop_front();
        }
        secrets.codes.push_back((digest(&code), expires));
        Ok(code)
    }

    /// Trade a login code for a browser session token. The code is spent whether
    /// or not the caller reads the answer.
    pub(crate) fn exchange(&self, code: &str, now: Now) -> Result<String, LoginRefusal> {
        let mut secrets = self.lock();
        if secrets.revoked {
            return Err(LoginRefusal::Invalid);
        }
        while secrets.failures.front().is_some_and(|failed| {
            now.monotonic.saturating_duration_since(*failed) >= FAILURE_WINDOW
        }) {
            secrets.failures.pop_front();
        }
        if secrets.failures.len() >= MAX_FAILURES {
            return Err(LoginRefusal::RateLimited);
        }
        let presented = digest(code);
        let found = secrets
            .codes
            .iter()
            .position(|(code, expiry)| *code == presented && expiry.live_at(now));
        let Some(index) = found else {
            secrets.failures.push_back(now.monotonic);
            return Err(LoginRefusal::Invalid);
        };
        secrets.codes.remove(index);
        let token = random_hex(SECRET_BYTES).map_err(|_| LoginRefusal::Invalid)?;
        let expires = now.plus(SESSION_TTL).ok_or(LoginRefusal::Invalid)?;
        secrets.sessions.retain(|_, expiry| expiry.live_at(now));
        secrets.sessions.insert(digest(&token), expires);
        if self.save_sessions(&secrets.sessions).is_err() {
            secrets.sessions.remove(&digest(&token));
            return Err(LoginRefusal::Persistence);
        }
        Ok(token)
    }

    /// Who presents `headers`' bearer token, if anyone recognized.
    pub(crate) fn caller(&self, headers: &HeaderMap, now: Now) -> Option<Caller> {
        if let Some(token) = auth::authenticate(&self.tokens, headers) {
            return Some(if token.name() == ADMIN_TOKEN {
                Caller::Admin
            } else {
                Caller::Token(token.name().to_owned())
            });
        }
        let presented = auth::bearer_token(headers)?;
        let secrets = self.lock();
        secrets
            .sessions
            .get(&digest(presented))
            .is_some_and(|expiry| expiry.live_at(now))
            .then_some(Caller::Browser)
    }

    pub(crate) fn browser_sessions(&self, now: Now) -> usize {
        self.lock()
            .sessions
            .values()
            .filter(|expiry| expiry.live_at(now))
            .count()
    }

    /// Forget this process's credentials during shutdown; saved sessions remain for restart.
    pub(crate) fn drop_sessions(&self) {
        let mut secrets = self.lock();
        secrets.codes.clear();
        secrets.sessions.clear();
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Secrets> {
        self.secrets
            .lock()
            .expect("control secrets lock poisoned by an earlier panic")
    }
}

/// The answer to a nonce challenge: an HMAC of the client's random challenge
/// keyed by the start nonce, which only the lock's owner and the instance know.
/// `None` when the challenge is not the expected length of hex.
pub(crate) fn challenge_response(nonce: &str, challenge: &str) -> Option<String> {
    let well_formed = challenge.len() == CHALLENGE_BYTES * 2
        && challenge.bytes().all(|byte| byte.is_ascii_hexdigit());
    if !well_formed {
        return None;
    }
    // HMAC accepts a key of any length, so construction cannot fail.
    let mut mac = Hmac::<Sha256>::new_from_slice(nonce.as_bytes())
        .expect("HMAC-SHA256 accepts keys of any length");
    mac.update(CHALLENGE_CONTEXT);
    mac.update(challenge.to_ascii_lowercase().as_bytes());
    Some(hex(&mac.finalize().into_bytes()))
}

/// Whether a control request names this listener in `Host` and, when it sends
/// one, in `Origin`: a page on another origin, or a DNS name rebound to loopback,
/// gets nothing.
pub(crate) fn same_origin(headers: &HeaderMap, port: u16) -> bool {
    let hosts = [
        format!("127.0.0.1:{port}"),
        format!("localhost:{port}"),
        format!("[::1]:{port}"),
    ];
    let host_ok = headers
        .get(HOST)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|host| {
            hosts
                .iter()
                .any(|allowed| host.eq_ignore_ascii_case(allowed))
        });
    if !host_ok {
        return false;
    }
    let mut origins = headers.get_all(ORIGIN).iter();
    let Some(origin) = origins.next() else {
        return true;
    };
    if origins.next().is_some() {
        return false;
    }
    origin.to_str().ok().is_some_and(|origin| {
        hosts
            .iter()
            .any(|allowed| origin.eq_ignore_ascii_case(&format!("http://{allowed}")))
    })
}

fn digest(secret: &str) -> [u8; 32] {
    Sha256::digest(secret.as_bytes()).into()
}

#[cfg(test)]
mod tests {
    use super::super::host::auth::Role;
    use axum::http::HeaderValue;
    use axum::http::header::AUTHORIZATION;

    use super::*;

    const ADMIN: &str = "0123456789abcdef0123456789abcdef-admin";
    const APP: &str = "0123456789abcdef0123456789abcdef-app";

    fn auth() -> ControlAuth {
        ControlAuth::new(vec![
            ApiToken::new(ADMIN_TOKEN, Role::Admin, ADMIN).unwrap(),
            ApiToken::new("app", Role::User, APP).unwrap(),
        ])
    }

    fn bearer(token: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {token}")).unwrap(),
        );
        headers
    }

    fn session_path(dir: &tempfile::TempDir) -> PathBuf {
        let state = super::super::state::StateDir::for_project(dir.path());
        state.create().unwrap();
        state.browser_sessions_path()
    }

    #[test]
    fn saved_sessions_survive_restart_with_original_expiry_and_project_scope() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = session_path(&dir);
        let now = Now::current();
        let first = ControlAuth::open(vec![], path.clone(), now).unwrap();
        let code = first.mint_login_code(now).unwrap();
        let token = first.exchange(&code, now).unwrap();
        let unused = first.mint_login_code(now).unwrap();
        let saved = std::fs::read_to_string(&path).unwrap();
        assert!(!saved.contains(&token));
        assert!(!saved.contains(&code));
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        drop(first);
        let hour = Duration::from_secs(3600);
        let restarted = ControlAuth::open(vec![], path.clone(), now.later(hour, hour)).unwrap();
        assert_eq!(
            restarted.caller(&bearer(&token), now.later(hour, hour)),
            Some(Caller::Browser)
        );
        assert_eq!(restarted.exchange(&code, now), Err(LoginRefusal::Invalid));
        assert_eq!(restarted.exchange(&unused, now), Err(LoginRefusal::Invalid));
        let end = now.later(SESSION_TTL, SESSION_TTL);
        assert_eq!(restarted.caller(&bearer(&token), end), None);
        let expired = ControlAuth::open(vec![], path.clone(), end).unwrap();
        assert_eq!(expired.caller(&bearer(&token), end), None);
        let other = tempfile::tempdir().unwrap();
        let separate = ControlAuth::open(vec![], session_path(&other), now).unwrap();
        assert_eq!(separate.caller(&bearer(&token), now), None);
        restarted.revoke_sessions().unwrap();
        assert!(restarted.mint_login_code(now).is_err());
        let revoked = ControlAuth::open(vec![], path, now).unwrap();
        assert_eq!(revoked.caller(&bearer(&token), now), None);
    }

    #[test]
    fn invalid_saved_sessions_fail_closed_and_failed_writes_issue_no_token() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().unwrap();
        let path = session_path(&dir);
        for contents in [
            "broken".to_owned(),
            r#"{"version":2,"sessions":[]}"#.to_owned(),
            "x".repeat(1024 * 1024 + 1),
        ] {
            std::fs::write(&path, contents).unwrap();
            assert!(ControlAuth::open(vec![], path.clone(), Now::current()).is_err());
        }
        std::fs::remove_file(&path).unwrap();
        let target = dir.path().join("target");
        std::fs::write(&target, r#"{"version":1,"sessions":[]}"#).unwrap();
        symlink(&target, &path).unwrap();
        assert!(ControlAuth::open(vec![], path, Now::current()).is_err());
        let now = Now::current();
        let auth =
            ControlAuth::open(vec![], dir.path().join("missing/sessions.json"), now).unwrap();
        let code = auth.mint_login_code(now).unwrap();
        assert_eq!(auth.exchange(&code, now), Err(LoginRefusal::Persistence));
        assert_eq!(auth.browser_sessions(now), 0);
        assert_eq!(auth.exchange(&code, now), Err(LoginRefusal::Invalid));
    }

    #[test]
    fn a_login_code_works_once_and_not_after_it_expires() {
        let auth = auth();
        let now = Now::current();
        let code = auth.mint_login_code(now).unwrap();
        assert!(code.len() * 4 >= 128);
        let session = auth.exchange(&code, now).unwrap();
        assert_eq!(auth.exchange(&code, now), Err(LoginRefusal::Invalid));
        assert_eq!(auth.caller(&bearer(&session), now), Some(Caller::Browser));

        let late = auth.mint_login_code(now).unwrap();
        let past = LOGIN_CODE_TTL + Duration::from_secs(1);
        let expired = now.later(past, past);
        assert_eq!(auth.exchange(&late, expired), Err(LoginRefusal::Invalid));
        let just_in_time = auth.mint_login_code(now).unwrap();
        let almost = LOGIN_CODE_TTL - Duration::from_secs(1);
        assert!(
            auth.exchange(&just_in_time, now.later(almost, almost))
                .is_ok()
        );
    }

    /// The monotonic clock stops while the machine sleeps; the wall clock does not,
    /// and either one passing the expiry ends a code or a session.
    #[test]
    fn a_code_and_a_session_expire_across_a_sleep() {
        let auth = auth();
        let now = Now::current();
        let code = auth.mint_login_code(now).unwrap();
        let slept = now.later(
            Duration::from_secs(1),
            LOGIN_CODE_TTL + Duration::from_secs(1),
        );
        assert_eq!(auth.exchange(&code, slept), Err(LoginRefusal::Invalid));

        let session = auth
            .exchange(&auth.mint_login_code(now).unwrap(), now)
            .unwrap();
        let slept = now.later(Duration::from_secs(1), SESSION_TTL + Duration::from_secs(1));
        assert_eq!(auth.caller(&bearer(&session), slept), None);
        assert_eq!(auth.browser_sessions(slept), 0);
        // A wall clock set back does not revive anything either.
        let rewound = Now {
            monotonic: now.monotonic + SESSION_TTL + Duration::from_secs(1),
            wall: now.wall,
        };
        assert_eq!(auth.caller(&bearer(&session), rewound), None);
    }

    #[test]
    fn sessions_expire_and_a_stop_drops_them() {
        let auth = auth();
        let now = Now::current();
        let session = auth
            .exchange(&auth.mint_login_code(now).unwrap(), now)
            .unwrap();
        let past = SESSION_TTL + Duration::from_secs(1);
        assert_eq!(auth.caller(&bearer(&session), now.later(past, past)), None);
        let session = auth
            .exchange(&auth.mint_login_code(now).unwrap(), now)
            .unwrap();
        auth.drop_sessions();
        assert_eq!(auth.caller(&bearer(&session), now), None);
    }

    #[test]
    fn failed_exchanges_are_rate_limited() {
        let auth = auth();
        let now = Now::current();
        let code = auth.mint_login_code(now).unwrap();
        for _ in 0..MAX_FAILURES {
            assert_eq!(auth.exchange("wrong", now), Err(LoginRefusal::Invalid));
        }
        assert_eq!(auth.exchange(&code, now), Err(LoginRefusal::RateLimited));
        assert!(
            auth.exchange(&code, now.later(FAILURE_WINDOW, FAILURE_WINDOW))
                .is_ok()
        );
    }

    #[test]
    fn tokens_are_told_apart() {
        let auth = auth();
        let now = Now::current();
        assert_eq!(auth.caller(&bearer(ADMIN), now), Some(Caller::Admin));
        assert_eq!(
            auth.caller(&bearer(APP), now),
            Some(Caller::Token("app".into()))
        );
        assert_eq!(auth.caller(&bearer("unknown-token"), now), None);
        assert_eq!(auth.caller(&HeaderMap::new(), now), None);
    }

    #[test]
    fn the_challenge_answer_depends_on_the_nonce_and_the_challenge() {
        let challenge = "ab".repeat(CHALLENGE_BYTES);
        let one = challenge_response("nonce-one", &challenge).unwrap();
        assert_eq!(one, challenge_response("nonce-one", &challenge).unwrap());
        assert_ne!(one, challenge_response("nonce-two", &challenge).unwrap());
        assert_ne!(
            one,
            challenge_response("nonce-one", &"cd".repeat(CHALLENGE_BYTES)).unwrap()
        );
        assert_eq!(challenge_response("nonce-one", "short"), None);
        assert_eq!(
            challenge_response("nonce-one", &"zz".repeat(CHALLENGE_BYTES)),
            None
        );
    }

    #[test]
    fn only_this_listener_s_host_and_origin_pass() {
        let headers = |host: &str, origin: Option<&str>| {
            let mut headers = HeaderMap::new();
            headers.insert(HOST, HeaderValue::from_str(host).unwrap());
            if let Some(origin) = origin {
                headers.insert(ORIGIN, HeaderValue::from_str(origin).unwrap());
            }
            headers
        };
        assert!(same_origin(&headers("127.0.0.1:4000", None), 4000));
        assert!(same_origin(
            &headers("localhost:4000", Some("http://localhost:4000")),
            4000
        ));
        assert!(!same_origin(&headers("evil.example:4000", None), 4000));
        assert!(!same_origin(&headers("127.0.0.1:4001", None), 4000));
        assert!(!same_origin(
            &headers("127.0.0.1:4000", Some("http://evil.example")),
            4000
        ));
        assert!(!same_origin(&headers("127.0.0.1:4000", Some("null")), 4000));
        assert!(!same_origin(
            &headers("127.0.0.1:4000", Some("http://127.0.0.1:4001")),
            4000
        ));
        assert!(!same_origin(&HeaderMap::new(), 4000));
        // Every loopback spelling of this listener, with its port; none without one.
        assert!(same_origin(&headers("localhost:4000", None), 4000));
        assert!(same_origin(&headers("LOCALHOST:4000", None), 4000));
        assert!(same_origin(
            &headers("[::1]:4000", Some("http://[::1]:4000")),
            4000
        ));
        for host in [
            "127.0.0.1",
            "localhost",
            "[::1]",
            "127.0.0.1:",
            "127.0.0.1:04000",
        ] {
            assert!(!same_origin(&headers(host, None), 4000), "{host}");
        }
    }

    #[test]
    fn a_garbage_or_huge_authorization_header_is_no_caller() {
        let auth = auth();
        let now = Now::current();
        let header = |value: &[u8]| {
            let mut headers = HeaderMap::new();
            headers.insert(AUTHORIZATION, HeaderValue::from_bytes(value).unwrap());
            headers
        };
        let huge = format!("Bearer {}", "a".repeat(1 << 20));
        let with_admin_prefix = format!("Bearer {ADMIN}{}", "0".repeat(4096));
        for value in [
            b"garbage".as_slice(),
            b"Bearer",
            b"Bearer ",
            b"Basic YWRtaW46YWRtaW4=",
            b"Bearer \xff\xfe\xfd",
            huge.as_bytes(),
            with_admin_prefix.as_bytes(),
        ] {
            assert_eq!(
                auth.caller(&header(value), now),
                None,
                "{}",
                String::from_utf8_lossy(&value[..value.len().min(40)])
            );
        }
    }
}
