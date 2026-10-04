//! Whom a token belongs to, as GitHub reports it: how `submilli github
//! authenticate` checks a token before storing it, and how `auth-status`
//! describes the one in use.

use ureq::http::StatusCode;

use super::client::{self, Endpoints};
use super::{GithubError, GithubToken, Result};

const ACTION: &str = "checking the GitHub token";

/// Whom a token belongs to, as GitHub reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TokenIdentity {
    pub login: String,
    /// When the token expires, as GitHub writes it (`2026-10-08 21:42:29
    /// UTC`); `None` for a token that never does.
    pub expires: Option<String>,
}

impl TokenIdentity {
    /// "expires 2026-10-08 21:42:29 UTC", or "never expires".
    pub fn expiry(&self) -> String {
        self.expires
            .as_ref()
            .map_or_else(|| "never expires".to_string(), |at| format!("expires {at}"))
    }
}

/// Ask GitHub whose token this is, which also checks that it is valid. It
/// can't check repository access: a fine-grained token's repositories aren't
/// listed anywhere it can read.
///
/// `None` is a token GitHub won't identify (403), as for a GitHub App's,
/// including an Actions run's `GITHUB_TOKEN`: it may still read
/// repositories. A rejected token is an [`Access`](GithubError::Access)
/// error, a used-up rate limit a [`RateLimited`](GithubError::RateLimited)
/// one, and anything else that goes wrong a [`Resolve`](GithubError::Resolve)
/// one.
pub fn token_identity(token: &GithubToken) -> Result<Option<TokenIdentity>> {
    token_identity_at(&Endpoints::github(), token)
}

pub(super) fn token_identity_at(
    endpoints: &Endpoints,
    token: &GithubToken,
) -> Result<Option<TokenIdentity>> {
    let url = format!("{}/user", endpoints.api);
    let response = client::send(
        &url,
        None,
        Some(token.secret()),
        ACTION,
        GithubError::Resolve,
    )?;
    let status = response.status();
    if status == StatusCode::UNAUTHORIZED {
        return Err(GithubError::Access(
            "GitHub rejected the token: it is mistyped, expired, or revoked".into(),
        ));
    }
    if status != StatusCode::OK {
        // A rate limit first; then a 403 is a token GitHub won't identify.
        return match client::rate_limited(response, ACTION, "") {
            Some(limited) => Err(limited),
            None if status == StatusCode::FORBIDDEN => Ok(None),
            None => Err(GithubError::Resolve(format!(
                "{ACTION}: GitHub returned HTTP {}",
                status.as_u16()
            ))),
        };
    }
    let expires =
        client::header(&response, "github-authentication-token-expiration").map(str::to_string);
    let body: serde_json::Value = response
        .into_body()
        .read_json()
        .map_err(|err| GithubError::Resolve(format!("{ACTION}: {err}")))?;
    let login = body
        .get("login")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("an unknown account")
        .to_string();
    Ok(Some(TokenIdentity { login, expires }))
}
