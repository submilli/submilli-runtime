use super::*;
use interpreter::runtime::limits::TenantLimits;

#[test]
fn code_observations_wait_for_the_blocking_pool() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .max_blocking_threads(1)
        .build()
        .unwrap()
        .block_on(async {
            let fixture = Fixture::new().await;
            std::fs::write(fixture.filesystem.vfs().root().join("input"), b"abc").unwrap();
            let limits = TenantLimits::new(1024);
            queued(fixture.filesystem.placement("/input"))
                .await
                .unwrap();
            let contents = queued(fixture.filesystem.read_regular(None, "/input", 10, &limits))
                .await
                .unwrap();
            assert_eq!(contents.bytes, b"abc");
            assert_eq!(limits.host_attached_bytes(), 4);
            drop(contents);
            let names = queued(fixture.filesystem.directory_names(None, "/", 10, &limits))
                .await
                .unwrap();
            assert!(names.names.iter().any(|name| name == "input"));
            drop(names);
            let effect = "stat".into();
            for follow in [true, false] {
                let metadata = queued(fixture.filesystem.metadata(&effect, None, "/input", follow))
                    .await
                    .unwrap();
                assert_eq!(metadata.len(), 3);
            }
            assert_eq!(limits.host_attached_bytes(), 0);
            assert!(limits.peak_bytes() > 0);
        });
}

async fn queued<F: std::future::Future>(future: F) -> F::Output {
    let (release, wait) = tokio::sync::oneshot::channel();
    let blocker = tokio::task::spawn_blocking(move || wait.blocking_recv());
    let mut future = Box::pin(future);
    let mut context = std::task::Context::from_waker(std::task::Waker::noop());
    assert!(
        future.as_mut().poll(&mut context).is_pending(),
        "filesystem I/O ran while blocking pool was occupied"
    );
    release.send(()).unwrap();
    blocker.await.unwrap().unwrap();
    future.await
}
