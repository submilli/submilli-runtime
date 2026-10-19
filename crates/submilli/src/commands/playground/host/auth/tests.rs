use axum::http::header::AUTHORIZATION;
use axum::http::{HeaderMap, HeaderValue};

use super::*;

const ADMIN: &str = "admin-token-0123456789abcdef0123456789";
const USER: &str = "user-token-0123456789abcdef01234567890";

fn tokens() -> Vec<ApiToken> {
    vec![
        ApiToken::new("admin", Role::Admin, ADMIN).unwrap(),
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
fn each_roles_token_is_accepted() {
    let tokens = tokens();
    let admin = authenticate(&tokens, &headers(&[&format!("Bearer {ADMIN}")])).unwrap();
    assert_eq!((admin.name(), admin.role()), ("admin", Role::Admin));
    let app = authenticate(&tokens, &headers(&[&format!("bearer {USER}")])).unwrap();
    assert_eq!((app.name(), app.role()), ("app", Role::User));
}

#[test]
fn a_missing_or_wrong_token_is_refused() {
    let tokens = tokens();
    assert!(authenticate(&tokens, &headers(&[])).is_none());
    assert!(authenticate(&tokens, &headers(&["Bearer "])).is_none());
    assert!(authenticate(&tokens, &headers(&[&format!("Basic {ADMIN}")])).is_none());
    assert!(
        authenticate(
            &tokens,
            &headers(&[&format!("Bearer {ADMIN}"), &format!("Bearer {ADMIN}")])
        )
        .is_none(),
        "a second Authorization header is no token at all"
    );
}

#[test]
fn a_wrong_token_is_refused_by_digest_whatever_its_length() {
    let tokens = tokens();
    // Same length as a configured token, differing in the last character.
    let same_length = format!("{}x", &ADMIN[..ADMIN.len() - 1]);
    assert_eq!(same_length.len(), ADMIN.len());
    assert!(authenticate(&tokens, &headers(&[&format!("Bearer {same_length}")])).is_none());
    assert!(authenticate(&tokens, &headers(&["Bearer short"])).is_none());
    // Both reach the digest compare: a presented token is hashed before any
    // comparison, so its length never decides the path it takes.
    assert_eq!(digest(&same_length).len(), digest("short").len());
}

#[test]
fn a_token_too_short_or_unsendable_is_refused_at_start() {
    assert_eq!(
        ApiToken::new("app", Role::User, "short").unwrap_err(),
        TokenError::TooShort { len: 5 }
    );
    assert_eq!(
        ApiToken::new("app", Role::User, &format!("{USER} ")).unwrap_err(),
        TokenError::InvalidCharacter
    );
}

#[test]
fn a_debug_print_never_shows_the_token() {
    let printed = format!("{:?}", tokens());
    assert!(
        !printed.contains(ADMIN) && !printed.contains(USER),
        "{printed}"
    );
}
