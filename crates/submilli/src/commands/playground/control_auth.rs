//! Who may use the control listener, and the checks that need no credential.
//!
//! - The CLI presents the admin token, checked with the server's own digest
//!   comparison ([`submilli_server::auth::authenticate`]).
//! - The page presents a browser session token, which it got by exchanging a
//!   single-use login code from a link. Both live only in this process's memory,
//!   as digests, and a stop drops them.
//! - The nonce challenge proves the listener belongs to the instance the lock
//!   names before any command sends a credential to it.

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use axum::http::HeaderMap;
use axum::http::header::{HOST, ORIGIN};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use submilli_server::ApiToken;

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
}

pub(crate) struct ControlAuth {
    tokens: Vec<ApiToken>,
    // Poison means a panic interrupted an update of the code or session tables;
    // AGENTS.md permits the poisoned-lock panic rather than trusting partial state.
    secrets: Mutex<Secrets>,
}

#[derive(Default)]
struct Secrets {
    /// Digest of an unused login code, and when it expires, oldest first.
    codes: VecDeque<([u8; 32], Instant)>,
    /// Digest of a browser session token, and when it expires.
    sessions: HashMap<[u8; 32], Instant>,
    failures: VecDeque<Instant>,
}

impl ControlAuth {
    pub(crate) fn new(tokens: Vec<ApiToken>) -> Self {
        Self {
            tokens,
            secrets: Mutex::new(Secrets::default()),
        }
    }

    /// A new single-use login code.
    pub(crate) fn mint_login_code(&self, now: Instant) -> anyhow::Result<String> {
        let code = random_hex(SECRET_BYTES)?;
        let expires = now
            .checked_add(LOGIN_CODE_TTL)
            .ok_or_else(|| anyhow::anyhow!("the clock cannot represent a login code's expiry"))?;
        let mut secrets = self.lock();
        secrets.codes.retain(|(_, expiry)| *expiry > now);
        while secrets.codes.len() >= MAX_CODES {
            secrets.codes.pop_front();
        }
        secrets.codes.push_back((digest(&code), expires));
        Ok(code)
    }

    /// Trade a login code for a browser session token. The code is spent whether
    /// or not the caller reads the answer.
    pub(crate) fn exchange(&self, code: &str, now: Instant) -> Result<String, LoginRefusal> {
        let mut secrets = self.lock();
        while secrets
            .failures
            .front()
            .is_some_and(|failed| now.saturating_duration_since(*failed) >= FAILURE_WINDOW)
        {
            secrets.failures.pop_front();
        }
        if secrets.failures.len() >= MAX_FAILURES {
            return Err(LoginRefusal::RateLimited);
        }
        let presented = digest(code);
        let found = secrets
            .codes
            .iter()
            .position(|(code, expiry)| *code == presented && *expiry > now);
        let Some(index) = found else {
            secrets.failures.push_back(now);
            return Err(LoginRefusal::Invalid);
        };
        secrets.codes.remove(index);
        let token = random_hex(SECRET_BYTES).map_err(|_| LoginRefusal::Invalid)?;
        let expires = now.checked_add(SESSION_TTL).ok_or(LoginRefusal::Invalid)?;
        secrets.sessions.retain(|_, expiry| *expiry > now);
        secrets.sessions.insert(digest(&token), expires);
        Ok(token)
    }

    /// Who presents `headers`' bearer token, if anyone recognized.
    pub(crate) fn caller(&self, headers: &HeaderMap, now: Instant) -> Option<Caller> {
        if let Some(token) = submilli_server::auth::authenticate(&self.tokens, headers) {
            return Some(if token.name() == ADMIN_TOKEN {
                Caller::Admin
            } else {
                Caller::Token(token.name().to_owned())
            });
        }
        let presented = submilli_server::auth::bearer_token(headers)?;
        let secrets = self.lock();
        secrets
            .sessions
            .get(&digest(presented))
            .is_some_and(|expiry| *expiry > now)
            .then_some(Caller::Browser)
    }

    pub(crate) fn browser_sessions(&self, now: Instant) -> usize {
        self.lock()
            .sessions
            .values()
            .filter(|expiry| **expiry > now)
            .count()
    }

    /// Forget every login code and browser session, as a stop does.
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
    use axum::http::HeaderValue;
    use axum::http::header::AUTHORIZATION;
    use submilli_server::Role;

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

    #[test]
    fn a_login_code_works_once_and_not_after_it_expires() {
        let auth = auth();
        let now = Instant::now();
        let code = auth.mint_login_code(now).unwrap();
        assert!(code.len() * 4 >= 128);
        let session = auth.exchange(&code, now).unwrap();
        assert_eq!(auth.exchange(&code, now), Err(LoginRefusal::Invalid));
        assert_eq!(auth.caller(&bearer(&session), now), Some(Caller::Browser));

        let late = auth.mint_login_code(now).unwrap();
        let expired = now + LOGIN_CODE_TTL + Duration::from_secs(1);
        assert_eq!(auth.exchange(&late, expired), Err(LoginRefusal::Invalid));
        let just_in_time = auth.mint_login_code(now).unwrap();
        assert!(
            auth.exchange(&just_in_time, now + LOGIN_CODE_TTL - Duration::from_secs(1))
                .is_ok()
        );
    }

    #[test]
    fn sessions_expire_and_a_stop_drops_them() {
        let auth = auth();
        let now = Instant::now();
        let session = auth
            .exchange(&auth.mint_login_code(now).unwrap(), now)
            .unwrap();
        let later = now + SESSION_TTL + Duration::from_secs(1);
        assert_eq!(auth.caller(&bearer(&session), later), None);
        let session = auth
            .exchange(&auth.mint_login_code(now).unwrap(), now)
            .unwrap();
        auth.drop_sessions();
        assert_eq!(auth.caller(&bearer(&session), now), None);
    }

    #[test]
    fn failed_exchanges_are_rate_limited() {
        let auth = auth();
        let now = Instant::now();
        let code = auth.mint_login_code(now).unwrap();
        for _ in 0..MAX_FAILURES {
            assert_eq!(auth.exchange("wrong", now), Err(LoginRefusal::Invalid));
        }
        assert_eq!(auth.exchange(&code, now), Err(LoginRefusal::RateLimited));
        assert!(auth.exchange(&code, now + FAILURE_WINDOW).is_ok());
    }

    #[test]
    fn tokens_are_told_apart() {
        let auth = auth();
        let now = Instant::now();
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
    }
}
