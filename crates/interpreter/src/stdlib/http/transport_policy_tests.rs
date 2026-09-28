use std::sync::Arc;

use super::transport::{HttpClient, HttpError, HttpRequest, ReqwestHttpClient};
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
    }
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
