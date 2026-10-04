use std::sync::Arc;

use super::transport::{
    HttpClient, HttpError, HttpRequest, RedirectDenied, RedirectGuard, RedirectHop,
    ReqwestHttpClient,
};
use super::{HttpTransportPolicy, TransportPolicyError};
use url::Url;

fn policy(allow: bool, authenticated: bool) -> Arc<HttpTransportPolicy> {
    Arc::new(HttpTransportPolicy {
        allow_insecure_http: allow,
        auth_proxy_hosts: vec![("blocked.example".into(), false)],
        same_origin_redirects: authenticated,
    })
}

fn request(url: String, policy: Option<Arc<HttpTransportPolicy>>) -> HttpRequest {
    HttpRequest {
        method: "GET".into(),
        url,
        headers: vec![("x-api-key".into(), "test-token".into())],
        body: vec![],
        timeout_ms: 1000,
        max_response_size: 1024,
        decompress: false,
        transport_policy: policy,
        redirect_guard: None,
    }
}

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[tokio::test]
async fn download_progress_survives_decoder_failure() {
    use super::transport::DownloadProgress;
    let server = httpmock::MockServer::start_async().await;
    server
        .mock_async(|when, then| {
            when.path("/invalid-gzip");
            then.status(200)
                .header("content-encoding", "gzip")
                .body("not a gzip stream");
        })
        .await;
    let client = ReqwestHttpClient::default();
    let mut req = request(server.url("/invalid-gzip"), None);
    req.decompress = true;
    let progress = DownloadProgress::default();
    let mut output = Vec::new();
    assert!(
        client
            .download_with_progress(&req, &mut output, &progress)
            .await
            .is_err()
    );
    assert_eq!(progress.bytes_received(), 17);
    assert!(output.is_empty());
}

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[tokio::test]
async fn download_progress_survives_body_timeout() {
    use super::transport::DownloadProgress;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = [0_u8; 4096];
        assert!(socket.read(&mut request).await.unwrap() > 0);
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 16\r\n\r\n12345678")
            .await
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    });
    let client = ReqwestHttpClient::default();
    let mut req = request(format!("http://{address}/slow"), None);
    req.timeout_ms = 100;
    let progress = DownloadProgress::default();
    let mut output = Vec::new();
    let result = client
        .download_with_progress(&req, &mut output, &progress)
        .await;
    server.await.unwrap();
    assert!(matches!(result, Err(HttpError::Timeout)), "{result:?}");
    assert_eq!(progress.bytes_received(), 8);
    assert_eq!(output, b"12345678");
}

#[test]
fn redirect_policy_checks_scheme_host_and_effective_port() {
    let initial = Url::parse("https://EXAMPLE.com:443/start?key=secret").unwrap();
    let authenticated = policy(true, true);
    for destination in ["https://example.com/end", "https://example.com:443/end"] {
        assert!(
            authenticated
                .check_redirect(&initial, &Url::parse(destination).unwrap())
                .is_ok()
        );
    }
    for destination in [
        "http://example.com/end",
        "https://other.example/end",
        "https://example.com:444/end",
    ] {
        assert_eq!(
            authenticated.check_redirect(&initial, &Url::parse(destination).unwrap()),
            Err(TransportPolicyError::CrossOriginRedirect)
        );
    }
    let http = Url::parse("http://example.com/").unwrap();
    assert_eq!(
        policy(false, false).check_redirect(&initial, &http),
        Err(TransportPolicyError::BlueprintRequiresHttps)
    );
    assert!(policy(true, false).check_redirect(&initial, &http).is_ok());
    let blocked = Url::parse("http://blocked.example/?secret=do-not-print").unwrap();
    let error = policy(true, false)
        .check_redirect(&initial, &blocked)
        .unwrap_err();
    assert!(matches!(error, TransportPolicyError::RuleRequiresHttps(_)));
    assert!(!error.to_string().contains("do-not-print"));
}

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[tokio::test]
async fn initial_denial_and_policy_changes_do_not_reuse_stale_permissions() {
    let server = httpmock::MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.path("/ok");
            then.status(200).body("ok");
        })
        .await;
    let client = ReqwestHttpClient::default();
    for allow in [false, true, false, true] {
        let req = request(server.url("/ok"), Some(policy(allow, false)));
        let result = client.send(&req).await;
        assert_eq!(result.is_ok(), allow);
        let mut output = Vec::new();
        assert_eq!(client.download(&req, &mut output).await.is_ok(), allow);
        assert_eq!(output, if allow { b"ok".to_vec() } else { vec![] });
    }
    mock.assert_hits_async(4).await;
    assert!(client.send(&request(server.url("/ok"), None)).await.is_ok());
    mock.assert_hits_async(5).await;
}

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[tokio::test]
async fn authenticated_redirects_allow_same_origin_but_never_another_port() {
    let server = httpmock::MockServer::start_async().await;
    let other = httpmock::MockServer::start_async().await;
    let same = server
        .mock_async(|when, then| {
            when.path("/same");
            then.status(302).header("location", "/ok");
        })
        .await;
    let ok = server
        .mock_async(|when, then| {
            when.path("/ok").header("x-api-key", "test-token");
            then.status(200).body("ok");
        })
        .await;
    let cross = server
        .mock_async(|when, then| {
            when.path("/cross");
            then.status(307).header("location", other.url("/leak"));
        })
        .await;
    let leak = other
        .mock_async(|_, then| {
            then.status(200);
        })
        .await;
    let client = ReqwestHttpClient::default();
    for download in [false, true] {
        let req = request(
            server.url("/same?key=query-secret"),
            Some(policy(true, true)),
        );
        if download {
            let mut bytes = vec![];
            client.download(&req, &mut bytes).await.unwrap();
            assert_eq!(bytes, b"ok");
        } else {
            assert_eq!(client.send(&req).await.unwrap().body, b"ok");
        }
        let req = request(
            server.url("/cross?key=query-secret"),
            Some(policy(true, true)),
        );
        let error = if download {
            client.download(&req, &mut vec![]).await.unwrap_err()
        } else {
            client.send(&req).await.unwrap_err()
        };
        assert!(error.to_string().contains("same-origin"), "{error}");
        assert!(!error.to_string().contains("query-secret"), "{error}");
    }
    same.assert_hits_async(2).await;
    ok.assert_hits_async(2).await;
    cross.assert_hits_async(2).await;
    leak.assert_hits_async(0).await;
}

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[tokio::test]
async fn redirects_check_destination_rules_and_retain_hop_limits() {
    let server = httpmock::MockServer::start_async().await;
    let redirect = server
        .mock_async(|when, then| {
            when.path("/start");
            then.status(302)
                .header("location", "http://blocked.example/secret");
        })
        .await;
    let cycle = server
        .mock_async(|when, then| {
            when.path("/loop");
            then.status(302).header("location", "/loop");
        })
        .await;
    let client = ReqwestHttpClient::default();
    let error = client
        .send(&request(server.url("/start"), Some(policy(true, false))))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("auth_proxy rule"), "{error}");
    redirect.assert_hits_async(1).await;
    let error = client
        .send(&request(server.url("/loop"), Some(policy(true, true))))
        .await
        .unwrap_err();
    assert!(matches!(error, HttpError::Network(_)));
    assert!(error.to_string().contains("too many redirects"), "{error}");
    cycle.assert_hits_async(11).await;
}

/// Denies hops to any URL starting with one of `denied`, recording every hop it sees.
#[derive(Debug, Default)]
struct TestGuard {
    denied: Vec<String>,
    seen: std::sync::Mutex<Vec<(String, String, bool, u64)>>,
}

impl TestGuard {
    fn denying(denied: &[String]) -> Arc<Self> {
        Arc::new(Self {
            denied: denied.to_vec(),
            seen: Default::default(),
        })
    }

    fn seen(&self) -> Vec<(String, String, bool, u64)> {
        self.seen.lock().unwrap().clone()
    }
}

impl RedirectGuard for TestGuard {
    fn authorize(&self, hop: &RedirectHop<'_>) -> Result<(), RedirectDenied> {
        self.seen.lock().unwrap().push((
            hop.method.to_string(),
            hop.url.to_string(),
            hop.method_rewritten,
            hop.body_len,
        ));
        if self
            .denied
            .iter()
            .any(|prefix| hop.url.as_str().starts_with(prefix.as_str()))
        {
            return Err(RedirectDenied::new("main", "http.test", "denied in test"));
        }
        Ok(())
    }
}

fn guarded(method: &str, url: String, body: &[u8], guard: &Arc<TestGuard>) -> HttpRequest {
    HttpRequest {
        method: method.into(),
        body: body.to_vec(),
        redirect_guard: Some(Arc::clone(guard) as Arc<dyn RedirectGuard>),
        ..request(url, None)
    }
}

fn has_header(req: &httpmock::prelude::HttpMockRequest, name: &str) -> bool {
    req.headers
        .iter()
        .flatten()
        .any(|(key, _)| key.eq_ignore_ascii_case(name))
}

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[tokio::test]
async fn denied_307_and_308_hops_never_receive_the_body() {
    let server = httpmock::MockServer::start_async().await;
    let attacker = httpmock::MockServer::start_async().await;
    let collect = attacker
        .mock_async(|_, then| {
            then.status(200);
        })
        .await;
    let client = ReqwestHttpClient::default();
    for status in [307, 308] {
        let start = server
            .mock_async(|when, then| {
                when.path(format!("/start{status}"));
                then.status(status)
                    .header("location", attacker.url("/collect"));
            })
            .await;
        let guard = TestGuard::denying(&[attacker.base_url()]);
        let req = guarded(
            "POST",
            server.url(format!("/start{status}")),
            b"secret-body",
            &guard,
        );
        let error = client.send(&req).await.unwrap_err();
        assert!(matches!(error, HttpError::PermissionDenied(_)), "{error}");
        assert!(error.to_string().contains("denied in test"), "{error}");
        assert_eq!(
            guard.seen(),
            vec![("POST".into(), attacker.url("/collect"), false, 11)]
        );
        start.assert_hits_async(1).await;
    }
    collect.assert_hits_async(0).await;
}

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[tokio::test]
async fn redirects_rewrite_methods_like_reqwest() {
    let server = httpmock::MockServer::start_async().await;
    let client = ReqwestHttpClient::default();
    // (status, sent method, method on the next hop, rewritten, body kept)
    let cases = [
        (301, "POST", "GET", true, false),
        (302, "POST", "GET", true, false),
        (303, "POST", "GET", true, false),
        (301, "PUT", "PUT", false, true),
        (302, "DELETE", "DELETE", false, true),
        (302, "PATCH", "PATCH", false, true),
        (303, "PUT", "GET", true, false),
        (303, "DELETE", "GET", true, false),
        (303, "HEAD", "HEAD", false, false),
        (303, "OPTIONS", "GET", true, false),
        (301, "HEAD", "HEAD", false, false),
        (303, "GET", "GET", false, false),
        (307, "POST", "POST", false, true),
        (308, "PUT", "PUT", false, true),
    ];
    for (index, (status, method, next_method, rewritten, body_kept)) in
        cases.into_iter().enumerate()
    {
        let redirect = server
            .mock_async(|when, then| {
                when.path(format!("/start{index}"));
                then.status(status)
                    .header("location", format!("/dest{index}"));
            })
            .await;
        let destination = server
            .mock_async(|when, then| {
                let when = when
                    .method(httpmock::Method::from(next_method))
                    .path(format!("/dest{index}"));
                if body_kept {
                    when.header("content-type", "text/plain").body("payload");
                } else {
                    when.matches(|req| {
                        !has_header(req, "content-type")
                            && req.body.as_ref().is_none_or(Vec::is_empty)
                    });
                }
                then.status(200);
            })
            .await;
        let guard = TestGuard::denying(&[]);
        // GET and HEAD carry no body, as scripts send them.
        let has_body = !matches!(method, "GET" | "HEAD");
        let payload: &[u8] = if has_body { b"payload" } else { b"" };
        let mut req = guarded(
            method,
            server.url(format!("/start{index}")),
            payload,
            &guard,
        );
        if has_body {
            req.headers = vec![("content-type".into(), "text/plain".into())];
        }
        let case = format!("{status} {method}");
        assert_eq!(client.send(&req).await.unwrap().status, 200, "{case}");
        let body_len = if body_kept { 7 } else { 0 };
        assert_eq!(
            guard.seen(),
            vec![(
                next_method.into(),
                server.url(format!("/dest{index}")),
                rewritten,
                body_len
            )],
            "{case}"
        );
        redirect.assert_hits_async(1).await;
        destination.assert_hits_async(1).await;
    }
}

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[tokio::test]
async fn hops_after_a_method_rewrite_stay_rewritten() {
    let server = httpmock::MockServer::start_async().await;
    server
        .mock_async(|when, then| {
            when.path("/a");
            then.status(302).header("location", "/b");
        })
        .await;
    server
        .mock_async(|when, then| {
            when.path("/b");
            then.status(307).header("location", "/c");
        })
        .await;
    let end = server
        .mock_async(|when, then| {
            when.method(httpmock::Method::GET).path("/c");
            then.status(200);
        })
        .await;
    let guard = TestGuard::denying(&[]);
    let req = guarded("POST", server.url("/a"), b"payload", &guard);
    ReqwestHttpClient::default().send(&req).await.unwrap();
    assert_eq!(
        guard.seen(),
        vec![
            ("GET".into(), server.url("/b"), true, 0),
            ("GET".into(), server.url("/c"), true, 0),
        ]
    );
    end.assert_hits_async(1).await;
}

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[tokio::test]
async fn url_credentials_follow_same_origin_redirects_only() {
    let server = httpmock::MockServer::start_async().await;
    let other = httpmock::MockServer::start_async().await;
    let basic = "Basic dXNlcjpwYXNz";
    server
        .mock_async(|when, then| {
            when.path("/a").header("authorization", basic);
            then.status(302)
                .header("location", format!("http://127.0.0.1:{}/b", server.port()));
        })
        .await;
    let same = server
        .mock_async(|when, then| {
            when.path("/b").header("authorization", basic);
            then.status(307).header("location", other.url("/c"));
        })
        .await;
    let cross = other
        .mock_async(|when, then| {
            when.path("/c")
                .matches(|req| !has_header(req, "authorization"));
            then.status(200).body("clean");
        })
        .await;
    let client = ReqwestHttpClient::default();
    let url = format!("http://user:pass@127.0.0.1:{}/a", server.port());
    assert_eq!(
        client.send(&request(url, None)).await.unwrap().body,
        b"clean"
    );
    same.assert_hits_async(1).await;
    cross.assert_hits_async(1).await;
}

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[tokio::test]
async fn redirects_to_forbidden_addresses_or_schemes_stop_before_the_guard() {
    let server = httpmock::MockServer::start_async().await;
    let policy =
        Arc::new(super::NetworkPolicy::deny_private().allow_cidr("127.0.0.1/32".parse().unwrap()));
    let client = ReqwestHttpClient::new(policy);
    for (index, location) in [
        format!("http://127.0.0.2:{}/", server.port()),
        format!("http://[::1]:{}/", server.port()),
        "http://169.254.169.254/latest/meta-data".to_string(),
        "file:///etc/passwd".to_string(),
    ]
    .into_iter()
    .enumerate()
    {
        let redirect = server
            .mock_async(|when, then| {
                when.path(format!("/r{index}"));
                then.status(302).header("location", location.clone());
            })
            .await;
        let guard = TestGuard::denying(&[]);
        let error = client
            .send(&guarded(
                "GET",
                server.url(format!("/r{index}")),
                b"",
                &guard,
            ))
            .await
            .unwrap_err();
        if location.starts_with("file:") {
            assert!(
                matches!(error, HttpError::Network(_)),
                "{location}: {error}"
            );
        } else {
            assert!(
                matches!(error, HttpError::EgressDenied(_)),
                "{location}: {error}"
            );
        }
        assert!(guard.seen().is_empty(), "{location}");
        redirect.assert_hits_async(1).await;
    }
}

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[tokio::test]
async fn every_hop_of_a_chain_and_same_origin_paths_are_checked() {
    let server = httpmock::MockServer::start_async().await;
    let other = httpmock::MockServer::start_async().await;
    let first = server
        .mock_async(|when, then| {
            when.path("/a");
            then.status(302).header("location", "/b");
        })
        .await;
    let second = server
        .mock_async(|when, then| {
            when.path("/b");
            then.status(302).header("location", other.url("/c"));
        })
        .await;
    let third = other
        .mock_async(|when, then| {
            when.path("/c");
            then.status(200);
        })
        .await;
    let admin_redirect = server
        .mock_async(|when, then| {
            when.path("/ok");
            then.status(302).header("location", "/admin");
        })
        .await;
    let admin = server
        .mock_async(|when, then| {
            when.path("/admin");
            then.status(200);
        })
        .await;
    let client = ReqwestHttpClient::default();

    let guard = TestGuard::denying(&[other.base_url()]);
    let error = client
        .send(&guarded("GET", server.url("/a"), b"", &guard))
        .await
        .unwrap_err();
    assert!(matches!(error, HttpError::PermissionDenied(_)), "{error}");
    assert_eq!(guard.seen().len(), 2);

    let guard = TestGuard::denying(&[server.url("/admin")]);
    let error = client
        .send(&guarded("GET", server.url("/ok"), b"", &guard))
        .await
        .unwrap_err();
    assert!(matches!(error, HttpError::PermissionDenied(_)), "{error}");

    first.assert_hits_async(1).await;
    second.assert_hits_async(1).await;
    third.assert_hits_async(0).await;
    admin_redirect.assert_hits_async(1).await;
    admin.assert_hits_async(0).await;
}

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[tokio::test]
async fn denied_download_hops_write_nothing() {
    let server = httpmock::MockServer::start_async().await;
    let other = httpmock::MockServer::start_async().await;
    server
        .mock_async(|when, then| {
            when.path("/file");
            then.status(302).header("location", other.url("/file"));
        })
        .await;
    let file = other
        .mock_async(|_, then| {
            then.status(200).body("forbidden");
        })
        .await;
    let client = ReqwestHttpClient::default();
    let guard = TestGuard::denying(&[other.base_url()]);
    let mut output = Vec::new();
    let error = client
        .download(
            &guarded("GET", server.url("/file"), b"", &guard),
            &mut output,
        )
        .await
        .unwrap_err();
    assert!(matches!(error, HttpError::PermissionDenied(_)), "{error}");
    assert!(output.is_empty());
    file.assert_hits_async(0).await;
}

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[tokio::test]
async fn cross_origin_hops_drop_credentials() {
    let server = httpmock::MockServer::start_async().await;
    let other = httpmock::MockServer::start_async().await;
    server
        .mock_async(|when, then| {
            when.path("/start");
            then.status(307).header("location", other.url("/end"));
        })
        .await;
    let clean = other
        .mock_async(|when, then| {
            when.path("/end")
                .matches(|req| !has_header(req, "authorization") && !has_header(req, "cookie"));
            then.status(200).body("clean");
        })
        .await;
    let client = ReqwestHttpClient::default();
    let mut req = request(server.url("/start"), None);
    req.headers = vec![
        ("Authorization".into(), "Bearer script-token".into()),
        ("Cookie".into(), "session=1".into()),
    ];
    assert_eq!(client.send(&req).await.unwrap().body, b"clean");
    clean.assert_hits_async(1).await;
}

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[tokio::test]
async fn one_timeout_covers_every_hop() {
    let server = httpmock::MockServer::start_async().await;
    server
        .mock_async(|when, then| {
            when.path("/slow1");
            then.status(302)
                .header("location", "/slow2")
                .delay(std::time::Duration::from_millis(400));
        })
        .await;
    server
        .mock_async(|when, then| {
            when.path("/slow2");
            then.status(200)
                .delay(std::time::Duration::from_millis(400));
        })
        .await;
    let client = ReqwestHttpClient::default();
    let mut req = request(server.url("/slow1"), None);
    req.timeout_ms = 600;
    let error = client.send(&req).await.unwrap_err();
    assert!(matches!(error, HttpError::Timeout), "{error}");
}

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[tokio::test]
async fn url_credentials_never_cross_to_another_port_and_yield_to_the_location() {
    let server = httpmock::MockServer::start_async().await;
    let other = httpmock::MockServer::start_async().await;
    // Same host, another port: a different origin.
    server
        .mock_async(|when, then| {
            when.path("/port");
            then.status(302)
                .header("location", format!("http://127.0.0.1:{}/end", other.port()));
        })
        .await;
    let clean = other
        .mock_async(|when, then| {
            when.path("/end")
                .matches(|req| !has_header(req, "authorization"));
            then.status(200);
        })
        .await;
    // A Location naming its own credentials keeps them.
    server
        .mock_async(|when, then| {
            when.path("/own");
            then.status(302).header(
                "location",
                format!("http://other:secret@127.0.0.1:{}/mine", server.port()),
            );
        })
        .await;
    let own = server
        .mock_async(|when, then| {
            when.path("/mine")
                .header("authorization", "Basic b3RoZXI6c2VjcmV0");
            then.status(200);
        })
        .await;
    let client = ReqwestHttpClient::default();
    for path in ["/port", "/own"] {
        let url = format!("http://user:pass@127.0.0.1:{}{path}", server.port());
        assert_eq!(
            client.send(&request(url, None)).await.unwrap().status,
            200,
            "{path}"
        );
    }
    clean.assert_hits_async(1).await;
    own.assert_hits_async(1).await;
}
