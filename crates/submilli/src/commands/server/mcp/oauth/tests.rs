use super::*;

#[test]
fn s256_matches_rfc7636_vector() {
    // RFC 7636 Appendix B.
    assert_eq!(
        s256_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
        "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
    );
}

#[test]
fn authorize_url_carries_pkce_and_scope() {
    let scopes = vec!["api".to_string(), "refresh token".to_string()];
    let url = authorize_url(
        "https://idp/authorize",
        "cid",
        &scopes,
        "http://127.0.0.1:9/callback",
        "CHAL",
        "STATE",
    );
    assert!(url.starts_with("https://idp/authorize?"), "{url}");
    assert!(url.contains("response_type=code"));
    assert!(url.contains("client_id=cid"));
    assert!(url.contains("code_challenge=CHAL"));
    assert!(url.contains("code_challenge_method=S256"));
    assert!(url.contains("state=STATE"));
    // Space-bearing scope is percent-encoded.
    assert!(url.contains("scope=api%20refresh%20token"), "{url}");
    assert!(
        url.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A9%2Fcallback"),
        "{url}"
    );
}

#[test]
fn authorize_url_appends_with_amp_when_endpoint_has_query() {
    let url = authorize_url("https://idp/authorize?foo=1", "cid", &[], "uri", "C", "S");
    assert!(url.starts_with("https://idp/authorize?foo=1&"), "{url}");
}

#[test]
fn parse_query_decodes_pairs() {
    let q = parse_query("code=a%20b&state=xyz&empty=");
    assert_eq!(q["code"], "a b");
    assert_eq!(q["state"], "xyz");
    assert_eq!(q["empty"], "");
}
