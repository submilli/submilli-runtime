//! The fetcher against a loopback `httpmock` GitHub: which requests carry the
//! token, and what each GitHub answer turns into.

use std::io::Write;

use httpmock::prelude::*;
use submilli_build::RepoFetcher;

use super::identity::token_identity_at;
use super::*;

const SHA: &str = "0123456789abcdef0123456789abcdef01234567";
const TOKEN: &str = "ghp_mocktoken";

fn endpoints(server: &MockServer) -> Endpoints {
    Endpoints {
        api: server.base_url(),
        codeload: server.base_url(),
    }
}

fn token_auth(source: TokenSource) -> GithubAuth {
    GithubAuth::new(Some(GithubToken::parse(TOKEN).unwrap()), source)
}

fn spec(git_ref: &str) -> GithubSpec {
    GithubSpec {
        org: "acme".into(),
        repo: "crm".into(),
        git_ref: Some(git_ref.into()),
    }
}

fn resolved() -> ResolvedRepo {
    ResolvedRepo {
        org: "acme".into(),
        repo: "crm".into(),
        sha: SHA.into(),
    }
}

/// A codeload-shaped `.tar.gz`: one `README.md` under `crm-<sha>/`.
fn gzipped_repo() -> Vec<u8> {
    let mut builder = tar::Builder::new(Vec::new());
    let mut header = tar::Header::new_gnu();
    header.set_size(6);
    header.set_mode(0o644);
    header.set_cksum();
    builder
        .append_data(&mut header, format!("crm-{SHA}/README.md"), &b"# crm\n"[..])
        .unwrap();
    let tar = builder.into_inner().unwrap();
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    gz.write_all(&tar).unwrap();
    gz.finish().unwrap()
}

fn has_no_authorization(req: &HttpMockRequest) -> bool {
    req.headers.as_ref().is_none_or(|headers| {
        !headers
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case("authorization"))
    })
}

#[test]
#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
fn a_token_reaches_resolve_and_download_and_keeps_the_hash() {
    let server = MockServer::start();
    let resolve = server.mock(|when, then| {
        when.method(GET)
            .path("/repos/acme/crm/commits/main")
            .header("authorization", format!("Bearer {TOKEN}"))
            .header("accept", "application/vnd.github.sha");
        then.status(200).body(SHA);
    });
    let body = gzipped_repo();
    let download = server.mock(|when, then| {
        when.method(GET)
            .path(format!("/acme/crm/tar.gz/{SHA}"))
            .header("authorization", format!("Bearer {TOKEN}"));
        then.status(200).body(body.clone());
    });

    let auth = token_auth(TokenSource::Stored);
    let resolved = spec("main").resolve_at(&endpoints(&server), &auth).unwrap();
    assert_eq!(resolved.sha, SHA);
    let (dir, hash) = resolved.download_at(&endpoints(&server), &auth).unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.path().join("README.md")).unwrap(),
        "# crm\n"
    );
    assert_eq!(hash, sha256_hex(&body));
    resolve.assert();
    download.assert();
    assert!(!auth.token_rejected());
}

#[test]
#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
fn no_token_sends_no_authorization_header() {
    let server = MockServer::start();
    let with_auth = server.mock(|when, then| {
        when.header_exists("authorization");
        then.status(418);
    });
    let anonymous = server.mock(|when, then| {
        when.method(GET).path("/repos/acme/crm/commits/main");
        then.status(200).body(SHA);
    });
    spec("main")
        .resolve_at(&endpoints(&server), &GithubAuth::anonymous())
        .unwrap();
    assert_eq!(with_auth.hits(), 0);
    anonymous.assert();
}

#[test]
#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
fn not_found_without_a_token_says_it_may_be_private() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.any_request();
        then.status(404);
    });
    let auth = GithubAuth::anonymous();
    let err = spec("main")
        .resolve_at(&endpoints(&server), &auth)
        .unwrap_err();
    let GithubError::Access(message) = err else {
        panic!("expected an access error, got {err:?}");
    };
    assert!(
        message.contains("if the repository is private"),
        "{message}"
    );
    assert!(message.contains("contents=read"), "{message}");
    assert_eq!(auth.unauthenticated_not_found(), Some("acme"));
}

#[test]
#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
fn not_found_with_a_token_names_the_token_and_never_shows_it() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.any_request();
        then.status(404);
    });
    let auth = token_auth(TokenSource::Env("GH_TOKEN"));
    let err = resolved()
        .download_at(&endpoints(&server), &auth)
        .unwrap_err();
    let message = err.to_string();
    assert!(matches!(err, GithubError::Access(_)), "{err:?}");
    assert!(message.contains("`GH_TOKEN`"), "{message}");
    assert!(!message.contains(TOKEN), "{message}");
    assert_eq!(auth.unauthenticated_not_found(), None);
}

/// An expired token must not break public installs: it is dropped, and the
/// rest of the fetch goes on without it.
#[test]
#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
fn a_rejected_token_is_dropped_and_the_fetch_retried_without_it() {
    let server = MockServer::start();
    let rejected = server.mock(|when, then| {
        when.header_exists("authorization");
        then.status(401);
    });
    let anonymous = server.mock(|when, then| {
        when.method(GET)
            .path("/repos/acme/crm/commits/main")
            .matches(has_no_authorization);
        then.status(200).body(SHA);
    });
    let body = gzipped_repo();
    let download = server.mock(|when, then| {
        when.method(GET)
            .path(format!("/acme/crm/tar.gz/{SHA}"))
            .matches(has_no_authorization);
        then.status(200).body(body.clone());
    });

    let auth = token_auth(TokenSource::Stored);
    let resolved = spec("main").resolve_at(&endpoints(&server), &auth).unwrap();
    resolved.download_at(&endpoints(&server), &auth).unwrap();
    assert!(auth.token_rejected());
    // Once rejected, the token isn't sent again.
    assert_eq!(rejected.hits(), 1);
    anonymous.assert();
    download.assert();
}

#[test]
#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
fn a_rejected_token_on_a_private_repo_says_it_was_rejected() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.header_exists("authorization");
        then.status(401);
    });
    server.mock(|when, then| {
        when.any_request();
        then.status(404);
    });
    let auth = token_auth(TokenSource::Stored);
    let message = spec("main")
        .resolve_at(&endpoints(&server), &auth)
        .unwrap_err()
        .to_string();
    assert!(message.contains("rejected"), "{message}");
    assert!(
        message.contains("submilli github authenticate"),
        "{message}"
    );
}

#[test]
#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
fn saml_sso_points_at_the_authorization_url() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.any_request();
        then.status(403).header(
            "X-GitHub-SSO",
            "required; url=https://github.com/orgs/acme/sso?authorization_request=abc",
        );
    });
    let err = spec("main")
        .resolve_at(&endpoints(&server), &token_auth(TokenSource::Stored))
        .unwrap_err();
    let message = err.to_string();
    assert!(matches!(err, GithubError::Access(_)), "{err:?}");
    assert!(
        message.contains("https://github.com/orgs/acme/sso?authorization_request=abc"),
        "{message}"
    );
}

#[test]
#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
fn a_used_up_rate_limit_is_reported_as_such() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.any_request();
        then.status(403)
            .header("x-ratelimit-remaining", "0")
            .header("x-ratelimit-reset", "0");
    });
    let auth = GithubAuth::new(None, TokenSource::ServerUnconfigured);
    let err = spec("main")
        .resolve_at(&endpoints(&server), &auth)
        .unwrap_err();
    let message = err.to_string();
    assert!(matches!(err, GithubError::RateLimited(_)), "{err:?}");
    assert!(message.contains("github_token_file"), "{message}");
}

/// The token is only for the host it was sent to: a redirect elsewhere is
/// refused rather than followed.
#[test]
#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
fn an_authenticated_redirect_to_another_host_is_not_followed() {
    let elsewhere = MockServer::start();
    let leaked = elsewhere.mock(|when, then| {
        when.any_request();
        then.status(200).body(SHA);
    });
    let server = MockServer::start();
    server.mock(|when, then| {
        when.any_request();
        then.status(302)
            .header("location", elsewhere.url("/repos/acme/crm/commits/main"));
    });
    let err = spec("main")
        .resolve_at(&endpoints(&server), &token_auth(TokenSource::Stored))
        .unwrap_err();
    assert!(err.to_string().contains("another host"), "{err}");
    assert_eq!(leaked.hits(), 0);
}

/// A renamed repository redirects within the API, which is followed.
#[test]
#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
fn an_authenticated_same_host_redirect_is_followed() {
    let server = MockServer::start();
    let moved = server.mock(|when, then| {
        when.path("/repositories/1/commits/main")
            .header("authorization", format!("Bearer {TOKEN}"));
        then.status(200).body(SHA);
    });
    server.mock(|when, then| {
        when.path("/repos/acme/crm/commits/main");
        then.status(301)
            .header("location", "/repositories/1/commits/main");
    });
    let resolved = spec("main")
        .resolve_at(&endpoints(&server), &token_auth(TokenSource::Stored))
        .unwrap();
    assert_eq!(resolved.sha, SHA);
    moved.assert();
}

#[test]
#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
fn the_dependency_fetcher_sends_the_token() {
    let server = MockServer::start();
    let body = gzipped_repo();
    let download = server.mock(|when, then| {
        when.path(format!("/acme/crm/tar.gz/{SHA}"))
            .header("authorization", format!("Bearer {TOKEN}"));
        then.status(200).body(body.clone());
    });
    let auth = token_auth(TokenSource::Stored);
    let fetcher = GithubRepoFetcher {
        auth: &auth,
        endpoints: endpoints(&server),
    };
    let fetched = fetcher.fetch("https://github.com/acme/crm", SHA).unwrap();
    assert_eq!(fetched.source_hash, sha256_hex(&body));
    download.assert();
}

#[test]
#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
fn token_identity_reports_login_and_expiry_or_rejection() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.path("/user")
            .header("authorization", format!("Bearer {TOKEN}"));
        then.status(200)
            .header(
                "github-authentication-token-expiration",
                "2026-10-08 21:42:29 UTC",
            )
            .body(r#"{"login":"octocat"}"#);
    });
    server.mock(|when, then| {
        when.path("/user");
        then.status(401);
    });
    let identity = token_identity_at(&endpoints(&server), &GithubToken::parse(TOKEN).unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(identity.login, "octocat");
    assert_eq!(identity.expires.as_deref(), Some("2026-10-08 21:42:29 UTC"));

    let rejected = token_identity_at(
        &endpoints(&server),
        &GithubToken::parse("ghp_other").unwrap(),
    )
    .unwrap_err();
    assert!(matches!(rejected, GithubError::Access(_)), "{rejected:?}");
}

/// GitHub's secondary limits answer 403 with `retry-after` while quota is left.
#[test]
#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
fn secondary_rate_limits_and_429_are_rate_limited() {
    for (status, header) in [(403, "retry-after"), (429, "x-unrelated")] {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.any_request();
            then.status(status).header(header, "90");
        });
        let err = spec("main")
            .resolve_at(&endpoints(&server), &GithubAuth::anonymous())
            .unwrap_err();
        assert!(matches!(err, GithubError::RateLimited(_)), "{err:?}");
        if header == "retry-after" {
            assert!(err.to_string().contains("in about 2 min"), "{err}");
        }
    }
}

#[test]
#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
fn an_authenticated_redirect_loop_or_missing_location_fails_cleanly() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.path("/repos/acme/crm/commits/loop");
        then.status(302)
            .header("location", "/repos/acme/crm/commits/loop");
    });
    server.mock(|when, then| {
        when.path("/repos/acme/crm/commits/nowhere");
        then.status(302);
    });
    let auth = token_auth(TokenSource::Stored);
    let looped = spec("loop")
        .resolve_at(&endpoints(&server), &auth)
        .unwrap_err();
    assert!(
        looped.to_string().contains("too many redirects"),
        "{looped}"
    );
    let missing = spec("nowhere")
        .resolve_at(&endpoints(&server), &auth)
        .unwrap_err();
    assert!(
        missing.to_string().contains("without a usable Location"),
        "{missing}"
    );
}

/// A ref is part of the URL the token goes to, so one that would point the
/// request at another API path is refused before any request.
#[test]
#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
fn a_ref_that_is_not_a_ref_never_reaches_github() {
    let server = MockServer::start();
    let any = server.mock(|when, then| {
        when.any_request();
        then.status(200).body(SHA);
    });
    let auth = token_auth(TokenSource::Stored);
    for bad in [
        "../../../user",
        "main?x=1",
        "a%2e%2e",
        "main#x",
        "/main",
        "a//b",
        ".hidden",
    ] {
        let err = spec(bad)
            .resolve_at(&endpoints(&server), &auth)
            .unwrap_err();
        assert!(matches!(err, GithubError::InvalidSpec(_)), "{bad}: {err:?}");
    }
    assert_eq!(any.hits(), 0);
    for good in ["main", "feature/x", "v1.2.3", "release-2024_01"] {
        spec(good).resolve_at(&endpoints(&server), &auth).unwrap();
    }
}

/// What GitHub sent back for a token-bearing request isn't repeated in the
/// error, which reaches a server's API caller.
#[test]
#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
fn a_body_that_is_not_a_sha_is_not_echoed() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.any_request();
        then.status(200).body(r#"{"email":"private@example.com"}"#);
    });
    let err = spec("main")
        .resolve_at(&endpoints(&server), &token_auth(TokenSource::Stored))
        .unwrap_err()
        .to_string();
    assert!(!err.contains("private@example.com"), "{err}");
}

/// A dependency the token can't read keeps its kind through the resolver's
/// string error, so the server answers as it would for the repository itself.
#[test]
#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
fn the_dependency_fetcher_keeps_the_error_kind() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.any_request();
        then.status(404);
    });
    let auth = GithubAuth::anonymous();
    let fetcher = GithubRepoFetcher {
        auth: &auth,
        endpoints: endpoints(&server),
    };
    let Err(err) = fetcher.fetch("https://github.com/acme/crm", SHA) else {
        panic!("a 404 must fail the fetch");
    };
    assert_eq!(err.kind, submilli_build::FetchErrorKind::Access);
    assert_eq!(auth.unauthenticated_not_found(), Some("acme"));
}

/// A 403 that is neither SSO nor a rate limit is an organization's token
/// policy: an access problem that names the token, not a failed transfer.
#[test]
#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
fn a_plain_403_to_a_token_is_an_access_error() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.any_request();
        then.status(403);
    });
    let err = spec("main")
        .resolve_at(
            &endpoints(&server),
            &token_auth(TokenSource::Env("GH_TOKEN")),
        )
        .unwrap_err();
    assert!(matches!(err, GithubError::Access(_)), "{err:?}");
    assert!(err.to_string().contains("`GH_TOKEN`"), "{err}");
    assert!(err.to_string().contains("Contents: Read-only"), "{err}");

    let anonymous = spec("main")
        .resolve_at(&endpoints(&server), &GithubAuth::anonymous())
        .unwrap_err();
    assert!(
        matches!(anonymous, GithubError::Resolve(_)),
        "{anonymous:?}"
    );
}

#[test]
#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
fn token_identity_reports_rate_limits_and_unidentifiable_tokens() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.path("/user")
            .header("authorization", "Bearer ghp_limited");
        then.status(403).header("x-ratelimit-remaining", "0");
    });
    server.mock(|when, then| {
        when.path("/user");
        then.status(403);
    });
    let limited = token_identity_at(
        &endpoints(&server),
        &GithubToken::parse("ghp_limited").unwrap(),
    )
    .unwrap_err();
    assert!(
        matches!(limited, GithubError::RateLimited(_)),
        "{limited:?}"
    );
    let app = token_identity_at(&endpoints(&server), &GithubToken::parse("ghs_app").unwrap());
    assert!(matches!(app, Ok(None)), "{app:?}");
}

/// After the token is rejected, a rate limit on the anonymous retry says so
/// instead of suggesting a token.
#[test]
#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
fn a_rate_limit_after_a_rejected_token_does_not_suggest_a_token() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.header_exists("authorization");
        then.status(401);
    });
    server.mock(|when, then| {
        when.any_request();
        then.status(403).header("x-ratelimit-remaining", "0");
    });
    let auth = token_auth(TokenSource::Stored);
    let err = spec("main")
        .resolve_at(&endpoints(&server), &auth)
        .unwrap_err()
        .to_string();
    // The rejection is reported once, by the caller, not as a hint to add a
    // token the user already has.
    assert!(!err.contains("raises the limit"), "{err}");
    assert!(auth.token_rejected());
}

/// A secondary limit may carry no header at all, only its message.
#[test]
#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
fn a_secondary_limit_known_only_from_its_body_is_rate_limited() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.any_request();
        then.status(403)
            .header("x-ratelimit-remaining", "4990")
            .body(r#"{"message":"You have exceeded a secondary rate limit."}"#);
    });
    let err = spec("main")
        .resolve_at(&endpoints(&server), &token_auth(TokenSource::Stored))
        .unwrap_err();
    assert!(matches!(err, GithubError::RateLimited(_)), "{err:?}");
    assert!(err.to_string().contains("in about 1 min"), "{err}");
    let identity =
        token_identity_at(&endpoints(&server), &GithubToken::parse(TOKEN).unwrap()).unwrap_err();
    assert!(
        matches!(identity, GithubError::RateLimited(_)),
        "{identity:?}"
    );
}

#[test]
#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
fn a_server_error_while_checking_a_token_is_not_blamed_on_the_token() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.path("/user");
        then.status(502);
    });
    let err = token_identity_at(&endpoints(&server), &GithubToken::parse(TOKEN).unwrap())
        .unwrap_err()
        .to_string();
    assert!(err.contains("HTTP 502"), "{err}");
    assert!(!err.contains("GitHub App"), "{err}");
}

#[test]
#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
fn a_429_while_checking_a_token_is_rate_limited() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.path("/user");
        then.status(429);
    });
    let err =
        token_identity_at(&endpoints(&server), &GithubToken::parse(TOKEN).unwrap()).unwrap_err();
    assert!(matches!(err, GithubError::RateLimited(_)), "{err:?}");
}

/// A commit that isn't in a public repository answers 404 like a private
/// repository does, so the message names the commit too.
#[test]
#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
fn a_missing_commit_is_named_in_the_not_found_message() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.any_request();
        then.status(404);
    });
    let err = resolved()
        .download_at(&endpoints(&server), &GithubAuth::anonymous())
        .unwrap_err()
        .to_string();
    assert!(err.contains(&format!("commit `{SHA}`")), "{err}");
}

/// With a token configured, a 404 is recorded as unauthenticated only after
/// GitHub rejected the token and the request was retried without it.
/// `LazyAuth::warn_if_rejected` relies on this to leave the rejection to the
/// not-found message.
#[test]
#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
fn a_not_found_with_a_token_configured_is_recorded_only_after_rejection() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.header_exists("authorization");
        then.status(404);
    });
    let auth = token_auth(TokenSource::Stored);
    spec("main")
        .resolve_at(&endpoints(&server), &auth)
        .unwrap_err();
    assert_eq!(auth.unauthenticated_not_found(), None);

    let rejecting = MockServer::start();
    rejecting.mock(|when, then| {
        when.header_exists("authorization");
        then.status(401);
    });
    rejecting.mock(|when, then| {
        when.any_request();
        then.status(404);
    });
    let auth = token_auth(TokenSource::Stored);
    let err = spec("main")
        .resolve_at(&endpoints(&rejecting), &auth)
        .unwrap_err()
        .to_string();
    assert!(auth.token_rejected());
    assert_eq!(auth.unauthenticated_not_found(), Some("acme"));
    assert!(err.contains("rejected"), "{err}");
}

#[test]
#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
fn a_missing_ref_says_so() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.any_request();
        then.status(422);
    });
    let auth = GithubAuth::anonymous();
    let err = spec("mian")
        .resolve_at(&endpoints(&server), &auth)
        .unwrap_err();
    // A typo is the caller's (400 on the server), and no reason to offer a token.
    assert!(matches!(err, GithubError::InvalidSpec(_)), "{err:?}");
    assert_eq!(auth.unauthenticated_not_found(), None);
    let err = err.to_string();
    assert!(
        err.contains("no such branch, tag, or commit in `acme/crm`"),
        "{err}"
    );
    assert!(err.contains("`mian`"), "{err}");
}
