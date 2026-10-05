use super::*;

#[tokio::test]
async fn forced_database_drain_releases_lock_after_work_finishes() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("server.db");
    let database = Arc::new(crate::database::ServerDatabase::open(&path).await.unwrap());
    close_database(database, Ok(DrainStatus::Forced), Duration::ZERO)
        .await
        .unwrap();
    let reopened = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match crate::database::ServerDatabase::open(&path).await {
                Ok(database) => break database,
                Err(crate::database::DatabaseError::AlreadyOpen(_)) => {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
                Err(error) => panic!("unexpected database error: {error}"),
            }
        }
    })
    .await
    .unwrap();
    reopened.close().await.unwrap();
}

use axum::{body::Body, response::IntoResponse};
use tokio::io::AsyncWriteExt;

struct RunningServer {
    address: SocketAddr,
    shutdown: Arc<Notify>,
    requests: Arc<crate::request_tasks::RequestTasks>,
    task: tokio::task::JoinHandle<Result<DrainStatus>>,
}

async fn start(router: axum::Router, grace: Duration) -> RunningServer {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let shutdown = Arc::new(Notify::new());
    let requests = Arc::new(crate::request_tasks::RequestTasks::default());
    let router = router.layer(axum::middleware::from_fn_with_state(
        Arc::clone(&requests),
        crate::request_tasks::run,
    ));
    let signals = ShutdownSignals::install().unwrap();
    let task = tokio::spawn(serve_listener(
        listener,
        router,
        signals,
        Arc::clone(&shutdown),
        Arc::clone(&requests),
        grace,
    ));
    RunningServer {
        address,
        shutdown,
        requests,
        task,
    }
}

async fn raw_request(address: SocketAddr) -> tokio::net::TcpStream {
    let mut client = tokio::net::TcpStream::connect(address).await.unwrap();
    client
        .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .await
        .unwrap();
    client
}

struct DropSignal(Arc<Notify>);
impl Drop for DropSignal {
    fn drop(&mut self) {
        self.0.notify_one();
    }
}

#[tokio::test]
#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
async fn disconnected_handler_is_drained_during_shutdown() {
    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let finished = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let router = axum::Router::new().route(
        "/",
        axum::routing::get({
            let entered = Arc::clone(&entered);
            let release = Arc::clone(&release);
            let finished = Arc::clone(&finished);
            move || {
                let entered = Arc::clone(&entered);
                let release = Arc::clone(&release);
                let finished = Arc::clone(&finished);
                async move {
                    entered.notify_one();
                    release.notified().await;
                    finished.store(true, std::sync::atomic::Ordering::Release);
                    "done"
                }
            }
        }),
    );
    let mut server = start(router, Duration::from_secs(5)).await;
    let client = raw_request(server.address).await;
    entered.notified().await;
    drop(client);
    server.shutdown.notify_one();
    assert!(
        tokio::time::timeout(Duration::from_millis(30), &mut server.task)
            .await
            .is_err()
    );
    release.notify_one();
    let status = tokio::time::timeout(Duration::from_secs(5), server.task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(matches!(status, DrainStatus::Completed { .. }));
    assert!(finished.load(std::sync::atomic::Ordering::Acquire));
}

#[tokio::test]
#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
async fn shutdown_waits_for_handler_and_streamed_response() {
    let entered = Arc::new(Notify::new());
    let handler_release = Arc::new(Notify::new());
    let body_release = Arc::new(Notify::new());
    let router = axum::Router::new().route(
        "/",
        axum::routing::get({
            let entered = Arc::clone(&entered);
            let handler_release = Arc::clone(&handler_release);
            let body_release = Arc::clone(&body_release);
            move || {
                let entered = Arc::clone(&entered);
                let handler_release = Arc::clone(&handler_release);
                let body_release = Arc::clone(&body_release);
                async move {
                    entered.notify_one();
                    handler_release.notified().await;
                    Body::from_stream(futures::stream::once(async move {
                        body_release.notified().await;
                        Ok::<_, std::io::Error>("complete response")
                    }))
                    .into_response()
                }
            }
        }),
    );
    let mut server = start(router, Duration::from_secs(5)).await;
    let address = server.address;
    let client = tokio::spawn(async move {
        reqwest::get(format!("http://{address}/"))
            .await
            .unwrap()
            .text()
            .await
            .unwrap()
    });
    entered.notified().await;
    server.shutdown.notify_one();
    assert!(
        tokio::time::timeout(Duration::from_millis(30), &mut server.task)
            .await
            .is_err()
    );
    handler_release.notify_one();
    tokio::time::timeout(Duration::from_secs(5), server.requests.wait())
        .await
        .unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(30), &mut server.task)
            .await
            .is_err()
    );
    body_release.notify_one();
    assert_eq!(client.await.unwrap(), "complete response");
    assert!(matches!(
        tokio::time::timeout(Duration::from_secs(5), server.task)
            .await
            .unwrap()
            .unwrap()
            .unwrap(),
        DrainStatus::Completed { .. }
    ));
}

#[tokio::test]
#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
async fn grace_deadline_cancels_owned_handlers() {
    let entered = Arc::new(Notify::new());
    let dropped = Arc::new(Notify::new());
    let router = axum::Router::new().route(
        "/",
        axum::routing::get({
            let entered = Arc::clone(&entered);
            let dropped = Arc::clone(&dropped);
            move || {
                let entered = Arc::clone(&entered);
                let dropped = Arc::clone(&dropped);
                async move {
                    let _guard = DropSignal(dropped);
                    entered.notify_one();
                    std::future::pending::<()>().await;
                    "unreachable response"
                }
            }
        }),
    );
    let server = start(router, Duration::from_millis(30)).await;
    let _client = raw_request(server.address).await;
    entered.notified().await;
    server.shutdown.notify_one();
    assert!(matches!(
        tokio::time::timeout(Duration::from_secs(5), server.task)
            .await
            .unwrap()
            .unwrap()
            .unwrap(),
        DrainStatus::Forced
    ));
    tokio::time::timeout(Duration::from_secs(5), dropped.notified())
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), server.requests.wait())
        .await
        .unwrap();
}
