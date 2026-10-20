//! The playground's tokens and the check a request's bearer token gets.
//!
//! Copied from `submilli-server`'s `auth.rs` (`ApiToken`, `bearer_token` and the
//! digest compare): a token is kept only as its SHA-256 digest, and a presented
//! token is hashed and compared against every entry in constant time.

use std::fmt;

use axum::http::HeaderMap;
use axum::http::header::AUTHORIZATION;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

/// Shortest token accepted. A token is compared by digest, so its strength is its
/// entropy; 32 characters of base64 or hex is the floor.
pub(crate) const MIN_TOKEN_LEN: usize = 32;

/// What a token may do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Role {
    /// Run code against the blueprint and read what it exposes.
    User,
    /// Everything, including the controls.
    Admin,
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Role::User => "user",
            Role::Admin => "admin",
        })
    }
}

/// One token. Holds the digest rather than the token, so neither a debug print
/// nor a core dump of this value yields a usable credential.
#[derive(Clone)]
pub(crate) struct ApiToken {
    name: String,
    role: Role,
    digest: [u8; 32],
}

impl ApiToken {
    /// `token` is the exact string a caller will present, checked against the
    /// RFC 6750 `b64token` alphabet so a token that could never survive an
    /// `Authorization` header is refused at start, not at first use.
    pub(crate) fn new(
        name: impl Into<String>,
        role: Role,
        token: &str,
    ) -> Result<Self, TokenError> {
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

    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    #[cfg(test)]
    pub(crate) fn role(&self) -> Role {
        self.role
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
pub(crate) enum TokenError {
    TooShort { len: usize },
    InvalidCharacter,
}

impl fmt::Display for TokenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TokenError::TooShort { len } => write!(
                f,
                "is {len} characters long, not counting `=` padding; use at least {MIN_TOKEN_LEN}"
            ),
            TokenError::InvalidCharacter => f.write_str(
                "contains a character that cannot be sent as a bearer token; use only letters, \
                 digits, and `-._~+/`, with optional trailing `=`",
            ),
        }
    }
}

impl std::error::Error for TokenError {}

/// The token a request presents in `Authorization: Bearer`, if it is one of `tokens`.
pub(crate) fn authenticate<'a>(
    tokens: &'a [ApiToken],
    headers: &HeaderMap,
) -> Option<&'a ApiToken> {
    let presented = digest(bearer_token(headers)?);
    // Every entry is compared, with no early exit, so the time taken says nothing
    // about which entry matched or whether any did.
    let mut found = None;
    for token in tokens {
        if bool::from(token.digest.ct_eq(&presented)) {
            found = Some(token);
        }
    }
    found
}

/// The token from `Authorization: Bearer <token>`. Anything else (a second
/// `Authorization` header, another scheme, an empty or non-ASCII token) is no
/// token at all, rather than a guess at which part the caller meant.
pub(crate) fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    let mut values = headers.get_all(AUTHORIZATION).iter();
    let value = values.next()?;
    if values.next().is_some() {
        return None;
    }
    let (scheme, token) = value.to_str().ok()?.split_once(' ')?;
    (scheme.eq_ignore_ascii_case("bearer") && !token.is_empty()).then_some(token)
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
mod tests;
