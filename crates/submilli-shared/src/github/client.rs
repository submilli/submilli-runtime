//! HTTP to GitHub, and the one place a token is attached to a request.
//!
//! The token goes only to the [`Endpoints`] the fetcher builds URLs from (in
//! production `https://api.github.com` and `https://codeload.github.com`). An
//! authenticated request follows a redirect only to the same origin, so the
//! token never leaves for another host; anonymous requests follow redirects as
//! usual.

use std::io::Read;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ureq::Body;
use ureq::http::{Response, StatusCode};

use super::auth::{GithubAuth, GithubToken};
use super::{GithubError, Result};

const USER_AGENT: &str = "submilli";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
/// How much of an error body is read to tell a secondary rate limit.
const RATE_LIMIT_BODY_BYTES: u64 = 4096;
/// Same-origin redirects an authenticated request follows (a renamed
/// repository redirects once).
const MAX_REDIRECTS: usize = 3;

/// The base URLs requests go to. Only tests point them elsewhere.
#[derive(Clone, Debug)]
pub(super) struct Endpoints {
    pub(super) api: String,
    pub(super) codeload: String,
}

impl Endpoints {
    pub(super) fn github() -> Self {
        Self {
            api: "https://api.github.com".into(),
            codeload: "https://codeload.github.com".into(),
        }
    }
}

/// What a request is for, so its failures can say so.
pub(super) struct Target<'a> {
    pub(super) org: &'a str,
    pub(super) repo: &'a str,
    /// The commit fetched, when the request names one.
    pub(super) commit: Option<&'a str>,
    /// For example "resolving acme/crm ref `main`".
    pub(super) action: &'a str,
    /// Wraps a transport failure or an unexpected status.
    pub(super) fail: fn(String) -> GithubError,
}

/// GET `url` with `auth`'s token, returning a 200 response. A token GitHub
/// rejects (401) is dropped for the rest of `auth`'s fetches and the request
/// retried without it, so an expired token doesn't break public installs.
pub(super) fn get(
    url: &str,
    accept: Option<&str>,
    auth: &GithubAuth,
    target: &Target<'_>,
) -> Result<Response<Body>> {
    let token = auth.active_token().map(GithubToken::secret);
    let mut response = send(url, accept, token, target.action, target.fail)?;
    let mut sent_token = token.is_some();
    if sent_token && response.status() == StatusCode::UNAUTHORIZED {
        auth.mark_rejected();
        response = send(url, accept, None, target.action, target.fail)?;
        sent_token = false;
    }
    check_status(response, sent_token, auth, target)
}

/// GET `url`, with `token` when given, returning whatever GitHub answered.
/// `action` and `fail` describe and wrap a transport failure.
pub(super) fn send(
    url: &str,
    accept: Option<&str>,
    token: Option<&str>,
    action: &str,
    fail: fn(String) -> GithubError,
) -> Result<Response<Body>> {
    let transport = |err: ureq::Error| fail(format!("{action}: {err}"));
    let Some(token) = token else {
        return request(&agent(None), url, accept).call().map_err(transport);
    };
    let agent = agent(Some(0));
    let bearer = format!("Bearer {token}");
    let mut url = url.to_string();
    for _ in 0..=MAX_REDIRECTS {
        let response = request(&agent, &url, accept)
            .header("Authorization", &bearer)
            .call()
            .map_err(transport)?;
        let status = response.status();
        if !status.is_redirection() {
            return Ok(response);
        }
        let Some(next) = redirect_location(&url, &response) else {
            return Err(fail(format!(
                "{action}: GitHub answered HTTP {} without a usable Location",
                status.as_u16()
            )));
        };
        if !url::Url::parse(&url).is_ok_and(|from| from.origin() == next.origin()) {
            return Err(fail(format!(
                "{action}: GitHub redirected to another host, and the token is only sent to the \
                 host it was meant for"
            )));
        }
        url = next.to_string();
    }
    Err(fail(format!("{action}: too many redirects")))
}

fn request(
    agent: &ureq::Agent,
    url: &str,
    accept: Option<&str>,
) -> ureq::RequestBuilder<ureq::typestate::WithoutBody> {
    let request = agent.get(url).header("User-Agent", USER_AGENT);
    match accept {
        Some(accept) => request.header("Accept", accept),
        None => request,
    }
}

/// `max_redirects` of `Some(0)` returns a 3xx response instead of following it.
fn agent(max_redirects: Option<u32>) -> ureq::Agent {
    let mut config = ureq::Agent::config_builder()
        .timeout_global(Some(REQUEST_TIMEOUT))
        .http_status_as_error(false);
    if let Some(max) = max_redirects {
        config = config.max_redirects(max);
    }
    config.build().into()
}

/// Where a redirect from `from` points, resolving a relative `Location`.
fn redirect_location(from: &str, response: &Response<Body>) -> Option<url::Url> {
    let location = response.headers().get("location")?.to_str().ok()?;
    url::Url::parse(from).ok()?.join(location).ok()
}

fn check_status(
    response: Response<Body>,
    sent_token: bool,
    auth: &GithubAuth,
    target: &Target<'_>,
) -> Result<Response<Body>> {
    let status = response.status();
    if status == StatusCode::OK {
        return Ok(response);
    }
    let (org, repo) = (target.org, target.repo);
    if status == StatusCode::NOT_FOUND {
        if !sent_token {
            auth.record_unauthenticated_not_found(org);
        }
        return Err(GithubError::Access(auth.not_found_message(
            org,
            repo,
            target.commit,
            sent_token,
        )));
    }
    if status == StatusCode::FORBIDDEN
        && let Some(sso) = header(&response, "x-github-sso")
    {
        let url = sso
            .split(';')
            .find_map(|part| part.trim().strip_prefix("url="));
        return Err(GithubError::Access(auth.sso_message(org, repo, url)));
    }
    let hint = if sent_token {
        String::new()
    } else {
        auth.raise_limit_hint()
            .map(|hint| format!("; {hint}"))
            .unwrap_or_default()
    };
    if let Some(limited) = rate_limited(response, target.action, &hint) {
        return Err(limited);
    }
    // GitHub answers a ref it can't find in a repository it can see with 422:
    // the caller's spec names nothing, like any other invalid spec.
    if status == StatusCode::UNPROCESSABLE_ENTITY {
        return Err(GithubError::InvalidSpec(format!(
            "{}: GitHub found no such branch, tag, or commit in `{org}/{repo}`",
            target.action
        )));
    }
    // A plain 403 to a token: it lacks Contents access to the repository, or
    // an organization's policy refuses it (approval, a forbidden kind).
    if status == StatusCode::FORBIDDEN && sent_token {
        return Err(GithubError::Access(auth.forbidden_message(org, repo)));
    }
    Err((target.fail)(format!(
        "{}: GitHub returned HTTP {}",
        target.action,
        status.as_u16()
    )))
}

/// A response header as text.
pub(super) fn header<'a>(response: &'a Response<Body>, name: &str) -> Option<&'a str> {
    response
        .headers()
        .get(name)
        .and_then(|value| value.to_str().ok())
}

/// A used-up rate limit, with when to try again and `hint` appended; `None`
/// for any other non-200 answer. GitHub's secondary limits answer 403 or 429,
/// with `retry-after` while quota is left, or with no header at all and only
/// the body saying so. Consumes the response, since that may mean reading it.
pub(super) fn rate_limited(
    response: Response<Body>,
    action: &str,
    hint: &str,
) -> Option<GithubError> {
    let status = response.status();
    if status != StatusCode::FORBIDDEN && status != StatusCode::TOO_MANY_REQUESTS {
        return None;
    }
    // Headers first: reading the body consumes the response.
    let retry_after = header(&response, "retry-after");
    let by_header = status == StatusCode::TOO_MANY_REQUESTS
        || header(&response, "x-ratelimit-remaining") == Some("0")
        || retry_after.is_some();
    let minutes = retry_after
        .and_then(|seconds| seconds.trim().parse::<u64>().ok())
        .map(|seconds| seconds.div_ceil(60).max(1))
        .or_else(|| header(&response, "x-ratelimit-reset").and_then(minutes_until));
    if !by_header && !body_mentions_rate_limit(response) {
        return None;
    }
    // GitHub's advice when a secondary limit names no time: wait a minute.
    let when = format!("in about {} min", minutes.unwrap_or(1));
    Some(GithubError::RateLimited(format!(
        "{action}: GitHub's rate limit is used up; try again {when}{hint}"
    )))
}

/// Whether the start of the body says "rate limit", as a secondary limit's
/// message does when it carries no header.
fn body_mentions_rate_limit(response: Response<Body>) -> bool {
    let mut start = Vec::new();
    let read = response
        .into_body()
        .into_reader()
        .take(RATE_LIMIT_BODY_BYTES)
        .read_to_end(&mut start);
    read.is_ok()
        && String::from_utf8_lossy(&start)
            .to_ascii_lowercase()
            .contains("rate limit")
}

/// Whole minutes (at least 1) from now until the Unix time in `reset`.
fn minutes_until(reset: &str) -> Option<u64> {
    let reset = reset.trim().parse::<u64>().ok()?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs();
    Some(reset.saturating_sub(now).div_ceil(60).max(1))
}
