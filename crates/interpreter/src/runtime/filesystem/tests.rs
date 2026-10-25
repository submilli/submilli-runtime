use super::*;

#[test]
fn missing_runtime_is_a_backend_error() {
    let workers = FilesystemWorkers::default();
    let mut future = Box::pin(workers.run(|| Ok(())));
    let mut context = std::task::Context::from_waker(std::task::Waker::noop());
    assert!(matches!(
        std::future::Future::poll(future.as_mut(), &mut context),
        std::task::Poll::Ready(Err(FilesystemError::Backend(_)))
    ));
}

#[tokio::test]
async fn operation_errors_preserve_their_kind_and_allow_follow_up() {
    let workers = FilesystemWorkers::default();
    let result = workers
        .run(|| Err::<(), _>(std::io::Error::from(std::io::ErrorKind::NotFound).into()))
        .await;
    assert!(
        matches!(result, Err(FilesystemError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound)
    );
    workers.finish().await.unwrap();
    assert_eq!(workers.run(|| Ok(42)).await.unwrap(), 42);
}

#[tokio::test]
async fn cancelled_inflight_worker_failure_is_drained_and_allows_follow_up() {
    let workers = FilesystemWorkers::default();
    let (started, ready) = tokio::sync::oneshot::channel();
    let (release, wait) = tokio::sync::oneshot::channel();
    let mut operation = Box::pin(workers.run(move || -> Result<(), FilesystemError> {
        started.send(()).unwrap();
        wait.blocking_recv().unwrap();
        panic!("injected worker failure");
    }));
    tokio::select! {
        _ = &mut operation => panic!("worker finished before release"),
        result = ready => result.unwrap(),
    }
    drop(operation);
    release.send(()).unwrap();
    assert!(matches!(
        workers.finish().await,
        Err(FilesystemError::Backend(_))
    ));
    assert_eq!(workers.run(|| Ok(42)).await.unwrap(), 42);
}
