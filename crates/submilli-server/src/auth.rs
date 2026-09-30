//! Inbound authentication for the HTTP API.
//!
//! A caller presents an opaque bearer token; the server keeps only its SHA-256
//! digest. Each token carries a [`Role`] and each route declares the
//! [`Access`] it needs when it is registered (see `app::Routes`), so a route
//! cannot exist without the guard knowing what to require of it.
//!
//! The token authenticates the *application*, not its end user: the
//! application binds the end user itself, through `Submilli-Variables`.

use std::fmt;
use std::net::IpAddr;
use std::sync::Arc;

use axum::Json;
use axum::extract::{Request, State};
use axum::http::header::{AUTHORIZATION, WWW_AUTHENTICATE};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

/// Shortest token the server accepts. A token is compared by digest, so its
/// strength is its entropy; 32 characters of base64 or hex is the floor below
/// which a guessed token stops being implausible.
pub const MIN_TOKEN_LEN: usize = 32;

/// What a token may do. The split exists because the blueprint is the security
/// boundary: a credential that can run code must not also be able to rewrite
/// the policy constraining that code, and the `User` token is the one that ends
/// up in every agent process.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// Run code against registered blueprints and read what they expose.
    User,
    /// Everything, including blueprint, secret, and package management.
    Admin,
}

impl Role {
    fn satisfies(self, access: Access) -> bool {
        match (self, access) {
            (_, Access::Public) | (Role::Admin, _) | (Role::User, Access::User) => true,
            (Role::User, Access::Admin) => false,
        }
    }
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Role::User => "user",
            Role::Admin => "admin",
        })
    }
}

/// What a route requires of its caller.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    /// No token. Reserved for endpoints that reveal nothing, such as the
    /// health probe a kubelet calls without credentials.
    Public,
    User,
    Admin,
}

/// One configured token. Holds the digest rather than the token, so neither a
/// debug print nor a core dump of this value yields a usable credential.
#[derive(Clone)]
pub struct ApiToken {
    name: String,
    role: Role,
    digest: [u8; 32],
}

impl ApiToken {
    /// `token` is the exact string a caller will present. It is checked against
    /// the RFC 6750 `b64token` alphabet so that a token which could never
    /// survive an `Authorization` header is refused at boot, not at first use.
    pub fn new(name: impl Into<String>, role: Role, token: &str) -> Result<Self, TokenError> {
        if !is_b64token(token) {
            return Err(TokenError::InvalidCharacter);
        }
        // Padding carries no entropy, so it does not count toward the floor.
        let len = token.trim_end_matches('=').len();
        if len < MIN_TOKEN_LEN {
            return Err(TokenError::TooShort { len });
        }
        Ok(Self {
            name: name.into(),
            role,
            digest: digest(token),
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn role(&self) -> Role {
        self.role
    }

    /// Whether two entries hold the same token. Two names for one token would
    /// make the role of a request depend on table order.
    pub fn same_token(&self, other: &Self) -> bool {
        bool::from(self.digest.ct_eq(&other.digest))
    }
}

impl fmt::Debug for ApiToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ApiToken")
            .field("name", &self.name)
            .field("role", &self.role)
            .finish_non_exhaustive()
    }
}

/// Why a token was refused. Never carries the token.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenError {
    TooShort { len: usize },
    InvalidCharacter,
}

impl fmt::Display for TokenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TokenError::TooShort { len } => write!(
                f,
                "is {len} characters long, not counting `=` padding; use at least \
                 {MIN_TOKEN_LEN} (for example `openssl rand -hex 32`)"
            ),
            TokenError::InvalidCharacter => f.write_str(
                "contains a character that cannot be sent as a bearer token; use only letters, \
                 digits, and `-._~+/`, with optional trailing `=`",
            ),
        }
    }
}

impl std::error::Error for TokenError {}

/// Whether the server checks callers at all.
///
/// The default is `Disabled`, like the rest of [`crate::ServerConfig`]'s
/// defaults: permissive, for embedders and tests that build a config in code.
/// The `submilli-server` binary does not inherit it — it refuses to start
/// without tokens unless the operator opts out explicitly.
#[derive(Clone, Debug, Default)]
pub enum AuthConfig {
    #[default]
    Disabled,
    Tokens(Vec<ApiToken>),
}

/// Say at startup what stands between the network and this server.
pub fn log_auth_posture(addr: IpAddr, auth: &AuthConfig) {
    match auth {
        AuthConfig::Tokens(tokens) => {
            let tokens = tokens
                .iter()
                .map(|token| format!("{} ({})", token.name, token.role))
                .collect::<Vec<_>>()
                .join(", ");
            tracing::info!(tokens, "inbound authentication enabled");
        }
        AuthConfig::Disabled if addr.is_loopback() => tracing::warn!(
            "inbound authentication is disabled: every process on this machine can run code, \
             manage blueprints, and stop the server. Set `SUBMILLI_SERVER_TOKEN` to require a \
             token"
        ),
        AuthConfig::Disabled => tracing::warn!(
            %addr,
            "inbound authentication is disabled and the server is bound outside loopback: \
             anything that can reach this port can run code, manage blueprints, and stop the \
             server. Set `SUBMILLI_SERVER_TOKEN` to require a token \
             (https://submilli.ai/docs/deploying/)"
        ),
    }
}

/// The per-route guard: the server's tokens plus what this route requires.
#[derive(Clone)]
pub(crate) struct Guard {
    auth: Arc<AuthConfig>,
    access: Access,
}

impl Guard {
    pub(crate) fn new(auth: Arc<AuthConfig>, access: Access) -> Self {
        Self { auth, access }
    }
}

pub(crate) async fn require(
    State(guard): State<Guard>,
    mut request: Request,
    next: Next,
) -> Response {
    let AuthConfig::Tokens(tokens) = guard.auth.as_ref() else {
        return next.run(request).await;
    };
    let Some(role) =
        bearer_token(request.headers()).and_then(|presented| role_of(tokens, presented))
    else {
        return unauthorized();
    };
    if !role.satisfies(guard.access) {
        return forbidden(role);
    }
    // The MCP transport copies the whole request head into each MCP request's
    // extensions, which would carry the token into the session layer and
    // anything that later records it.
    request.headers_mut().remove(AUTHORIZATION);
    next.run(request).await
}

/// The role of the token a request presents, if it presents a known one.
///
/// Every entry is compared, with no early exit, so the time taken says nothing
/// about which entry matched or whether any did.
fn role_of(tokens: &[ApiToken], presented: &str) -> Option<Role> {
    let presented = digest(presented);
    let mut role = None;
    for token in tokens {
        if bool::from(token.digest.ct_eq(&presented)) {
            role = Some(token.role);
        }
    }
    role
}

/// The token from `Authorization: Bearer <token>`. Anything else — a second
/// `Authorization` header, another scheme, an empty or non-ASCII token — is no
/// token at all, rather than a best-effort guess at which part the caller meant.
fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    let mut values = headers.get_all(AUTHORIZATION).iter();
    let value = values.next()?;
    if values.next().is_some() {
        return None;
    }
    let (scheme, token) = value.to_str().ok()?.split_once(' ')?;
    (scheme.eq_ignore_ascii_case("bearer") && !token.is_empty()).then_some(token)
}

#[derive(Serialize)]
struct ErrorBody {
    error: &'static str,
    message: String,
}

/// A bare `Bearer` challenge, with no `resource_metadata`: an MCP client reads
/// that parameter as an invitation to start OAuth discovery, and there is no
/// authorization server to discover. It should report the failure instead.
fn unauthorized() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        [(WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"))],
        Json(ErrorBody {
            error: "unauthorized",
            message: "missing or unrecognised API token; send `Authorization: Bearer <token>` \
                      with a token this server was started with"
                .into(),
        }),
    )
        .into_response()
}

/// Names `admin` outright: a known token is only ever refused for being a
/// `user` token on an admin route (see [`Role::satisfies`]).
fn forbidden(role: Role) -> Response {
    (
        StatusCode::FORBIDDEN,
        Json(ErrorBody {
            error: "forbidden",
            message: format!(
                "this endpoint needs a token with the `admin` role; the token sent has the \
                 `{role}` role"
            ),
        }),
    )
        .into_response()
}

fn digest(token: &str) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
}

/// RFC 6750 `b64token`: `1*( ALPHA / DIGIT / "-" / "." / "_" / "~" / "+" / "/" ) *"="`.
fn is_b64token(token: &str) -> bool {
    let body = token.trim_end_matches('=');
    !body.is_empty()
        && body
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-._~+/".contains(&b))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ADMIN: &str = "admin-token-0123456789abcdef0123456789";
    const USER: &str = "user-token-0123456789abcdef01234567890";

    fn tokens() -> Vec<ApiToken> {
        vec![
            ApiToken::new("ops", Role::Admin, ADMIN).unwrap(),
            ApiToken::new("app", Role::User, USER).unwrap(),
        ]
    }

    fn headers(values: &[&str]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for value in values {
            headers.append(AUTHORIZATION, HeaderValue::from_str(value).unwrap());
        }
        headers
    }

    #[test]
    fn a_known_token_resolves_to_its_role() {
        let tokens = tokens();
        assert_eq!(role_of(&tokens, ADMIN), Some(Role::Admin));
        assert_eq!(role_of(&tokens, USER), Some(Role::User));
        assert_eq!(role_of(&tokens, "not-a-configured-token"), None);
        assert_eq!(role_of(&tokens, ""), None);
    }

    #[test]
    fn roles_cover_the_access_levels_they_should() {
        assert!(Role::Admin.satisfies(Access::Admin));
        assert!(Role::Admin.satisfies(Access::User));
        assert!(Role::User.satisfies(Access::User));
        assert!(!Role::User.satisfies(Access::Admin));
        assert!(Role::User.satisfies(Access::Public));
    }

    #[test]
    fn only_a_single_well_formed_bearer_header_yields_a_token() {
        assert_eq!(bearer_token(&headers(&["Bearer abc"])), Some("abc"));
        assert_eq!(bearer_token(&headers(&["bearer abc"])), Some("abc"));
        assert_eq!(bearer_token(&headers(&[])), None);
        assert_eq!(bearer_token(&headers(&["Bearer"])), None);
        assert_eq!(bearer_token(&headers(&["Bearer "])), None);
        assert_eq!(bearer_token(&headers(&["Basic abc"])), None);
        assert_eq!(bearer_token(&headers(&["abc"])), None);
        assert_eq!(bearer_token(&headers(&["Bearer abc", "Bearer abc"])), None);
    }

    #[test]
    fn a_token_is_refused_when_short_or_unsendable() {
        assert_eq!(
            ApiToken::new("t", Role::User, "short").unwrap_err(),
            TokenError::TooShort { len: 5 }
        );
        for bad in [
            "has a space 0123456789abcdef0123456789",
            "has\ttab-0123456789abcdef0123456789abc",
            "pad=inside-0123456789abcdef0123456789",
            "================================",
            "has,comma-0123456789abcdef0123456789abc",
            "non-ascii-é-0123456789abcdef0123456789",
        ] {
            assert_eq!(
                ApiToken::new("t", Role::User, bad).unwrap_err(),
                TokenError::InvalidCharacter,
                "{bad:?}"
            );
        }
        assert!(ApiToken::new("t", Role::User, "abcDEF123-._~+/0123456789abcdef0==").is_ok());
        let padded = format!("a{}", "=".repeat(40));
        assert_eq!(
            ApiToken::new("t", Role::User, &padded).unwrap_err(),
            TokenError::TooShort { len: 1 }
        );
    }

    #[test]
    fn debug_and_errors_never_show_the_token() {
        let token = ApiToken::new("ops", Role::Admin, ADMIN).unwrap();
        let config = AuthConfig::Tokens(vec![token.clone()]);
        let printed = format!("{token:?} {config:?}");
        assert!(printed.contains("ops"));
        assert!(!printed.contains(ADMIN));
        let short = "secret-but-short";
        let err = ApiToken::new("t", Role::User, short)
            .unwrap_err()
            .to_string();
        assert!(!err.contains(short), "{err}");
    }

    #[tokio::test]
    async fn the_token_is_removed_before_the_handler_runs() {
        use axum::body::Body;
        use axum::routing::get;
        use tower::ServiceExt;

        let guard = Guard::new(Arc::new(AuthConfig::Tokens(tokens())), Access::User);
        let router: axum::Router = axum::Router::new().route(
            "/",
            get(
                |headers: HeaderMap| async move { headers.contains_key(AUTHORIZATION).to_string() },
            )
            .layer(axum::middleware::from_fn_with_state(guard, require)),
        );
        let request = Request::get("/")
            .header(AUTHORIZATION, format!("Bearer {USER}"))
            .body(Body::empty())
            .unwrap();
        let response = router.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 64)
            .await
            .unwrap();
        assert_eq!(&body[..], b"false");
    }

    #[test]
    fn the_same_token_under_two_names_is_detected() {
        let a = ApiToken::new("a", Role::Admin, ADMIN).unwrap();
        let b = ApiToken::new("b", Role::User, ADMIN).unwrap();
        let c = ApiToken::new("c", Role::User, USER).unwrap();
        assert!(a.same_token(&b));
        assert!(!a.same_token(&c));
    }
}
