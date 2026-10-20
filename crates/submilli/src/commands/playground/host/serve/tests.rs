use std::time::Duration;

use axum::Router;
use axum::routing::get;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc::UnboundedSender;

use super::*;

const GRACE: Duration = Duration::from_secs(5);

/// The health route, and a route that answers once `release` is notified.
fn routes(release: Arc<Notify>) -> Router {
    router().route(
        "/slow",
        get(move || async move {
            release.notified().await;
            "done"
        }),
    )
}

async fn listening(router: Router) -> (SocketAddr, UnboundedSender<Stop>, JoinHandle) {
    let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    let (stop, stops) = Stops::channel();
    let served = tokio::spawn(serve(listener, router, stops, GRACE));
    (addr, stop, served)
}

type JoinHandle = tokio::task::JoinHandle<anyhow::Result<Drained>>;

/// Send `GET path` with no token and return the raw response.
async fn get_raw(addr: SocketAddr, path: &str) -> String {
    let mut stream = TcpStream::connect(addr).await.unwrap();
    stream
        .write_all(
            format!("GET {path} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n").as_bytes(),
        )
        .await
        .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).await.unwrap();
    response
}

#[tokio::test]
async fn the_health_check_needs_no_token() {
    let (addr, stop, served) = listening(router()).await;
    let response = get_raw(addr, "/healthz").await;
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    stop.send(Stop::Requested).unwrap();
    assert_eq!(served.await.unwrap().unwrap(), Drained::Completed);
}

#[tokio::test]
async fn a_stop_lets_the_request_in_flight_finish_then_closes_the_listener() {
    let release = Arc::new(Notify::new());
    let (addr, stop, served) = listening(routes(Arc::clone(&release))).await;
    let request = tokio::spawn(get_raw(addr, "/slow"));
    tokio::time::sleep(Duration::from_millis(100)).await;
    stop.send(Stop::Requested).unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(!served.is_finished(), "the drain waits for the request");
    release.notify_one();
    let response = tokio::time::timeout(GRACE, request).await.unwrap().unwrap();
    assert!(response.ends_with("done"), "{response}");
    assert_eq!(
        tokio::time::timeout(GRACE, served)
            .await
            .unwrap()
            .unwrap()
            .unwrap(),
        Drained::Completed
    );
    assert!(
        TcpStream::connect(addr).await.is_err(),
        "the listener closed"
    );
}

#[tokio::test]
async fn a_second_signal_during_the_drain_stops_at_once() {
    let (addr, stop, served) = listening(routes(Arc::new(Notify::new()))).await;
    let _request = tokio::spawn(get_raw(addr, "/slow"));
    tokio::time::sleep(Duration::from_millis(100)).await;
    stop.send(Stop::Signal(libc::SIGINT)).unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        !served.is_finished(),
        "the first signal only begins the drain"
    );
    stop.send(Stop::Signal(libc::SIGINT)).unwrap();
    let drained = tokio::time::timeout(Duration::from_secs(1), served)
        .await
        .expect("stopped well inside the grace period")
        .unwrap()
        .unwrap();
    assert_eq!(drained, Drained::Forced(Forced::SecondSignal));
}

#[tokio::test]
async fn the_drain_gives_up_once_the_grace_period_ends() {
    let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    let (stop, stops) = Stops::channel();
    let grace = Duration::from_millis(300);
    let served = tokio::spawn(serve(
        listener,
        routes(Arc::new(Notify::new())),
        stops,
        grace,
    ));
    let _request = tokio::spawn(get_raw(addr, "/slow"));
    tokio::time::sleep(Duration::from_millis(100)).await;
    stop.send(Stop::Requested).unwrap();
    let drained = tokio::time::timeout(GRACE, served)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(drained, Drained::Forced(Forced::GraceElapsed));
}
