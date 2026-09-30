//! Local test CA and key are fixtures only; no public endpoint is contacted.
use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_rustls::TlsAcceptor;
use tokio_rustls::rustls::{
    ServerConfig,
    pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject},
};

const CA: &[u8] = include_bytes!("../../../tests/http_tls_fixtures/ca.pem");
const CERT: &[u8] = include_bytes!("../../../tests/http_tls_fixtures/localhost.pem");
const KEY: &[u8] = include_bytes!("../../../tests/http_tls_fixtures/localhost-key.pem");

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[tokio::test]
async fn tls_downgrades_require_permission_and_never_forward_proxy_credentials() {
    let destination = httpmock::MockServer::start_async().await;
    let received = destination
        .mock_async(|_, then| {
            then.status(200).body("ok");
        })
        .await;
    let config = ServerConfig::builder_with_provider(Arc::new(
        tokio_rustls::rustls::crypto::aws_lc_rs::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_no_client_auth()
    .with_single_cert(
        vec![CertificateDer::from_pem_slice(CERT).unwrap()],
        PrivateKeyDer::from_pem_slice(KEY).unwrap(),
    )
    .unwrap();
    let acceptor = TlsAcceptor::from(Arc::new(config));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!(
        "https://localhost:{}/start?key=secret",
        listener.local_addr().unwrap().port()
    );
    let location = destination.url("/end");
    let server = tokio::spawn(async move {
        for _ in 0..6 {
            let (socket, _) = listener.accept().await.unwrap();
            let mut stream = acceptor.accept(socket).await.unwrap();
            let mut request = Vec::new();
            let mut chunk = [0; 1024];
            while !request.windows(4).any(|part| part == b"\r\n\r\n") {
                let count = stream.read(&mut chunk).await.unwrap();
                assert_ne!(count, 0);
                request.extend_from_slice(&chunk[..count]);
            }
            stream.write_all(format!("HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").as_bytes()).await.unwrap();
            stream.shutdown().await.unwrap();
        }
    });
    // Trust only our local CA; redirects still run through the production loop.
    let client = ReqwestHttpClient::with_client(
        Arc::new(crate::stdlib::http::policy::NetworkPolicy::allow_all()),
        |builder| builder.tls_certs_only([reqwest::Certificate::from_pem(CA).unwrap()]),
    );
    for (allow, authenticated) in [(false, false), (true, true), (true, false)] {
        let policy = Some(Arc::new(HttpTransportPolicy {
            allow_insecure_http: allow,
            auth_proxy_hosts: vec![],
            same_origin_redirects: authenticated,
        }));
        let request = HttpRequest {
            method: "GET".into(),
            url: url.clone(),
            headers: if authenticated {
                vec![("x-api-key".into(), "secret".into())]
            } else {
                vec![]
            },
            body: vec![],
            timeout_ms: 2000,
            max_response_size: 1024,
            decompress: false,
            transport_policy: policy,
            redirect_guard: None,
        };
        for download in [false, true] {
            let result = if download {
                client
                    .download(&request, &mut vec![])
                    .await
                    .map(|response| response.status)
            } else {
                client.send(&request).await.map(|response| response.status)
            };
            if allow && !authenticated {
                assert_eq!(result.unwrap(), 200);
            } else {
                let error = result.unwrap_err().to_string();
                assert!(
                    error.contains(if allow {
                        "same-origin"
                    } else {
                        "HTTPS required"
                    }),
                    "{error}"
                );
                assert!(!error.contains("key=secret"), "{error}");
            }
        }
    }
    tokio::time::timeout(Duration::from_secs(5), server)
        .await
        .unwrap()
        .unwrap();
    received.assert_hits_async(2).await;
}
