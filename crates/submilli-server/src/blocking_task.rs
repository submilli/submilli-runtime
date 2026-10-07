//! Fallible admission to Tokio's blocking pool without rejected side effects.

use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::sync::mpsc;

use tokio::runtime::Handle;
use tokio::task::{JoinError, JoinHandle};

#[derive(Debug)]
pub(crate) enum BlockingTaskError {
    RuntimeUnavailable,
    AdmissionFailed(String),
    WorkNotStarted,
    Join(JoinError),
}

impl std::fmt::Display for BlockingTaskError {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RuntimeUnavailable => out.write_str("blocking task requires a Tokio runtime"),
            Self::AdmissionFailed(message) => {
                write!(out, "blocking task admission failed: {message}")
            }
            Self::WorkNotStarted => {
                out.write_str("blocking task stopped before receiving its work")
            }
            Self::Join(error) => write!(out, "blocking task failed: {error}"),
        }
    }
}

impl std::error::Error for BlockingTaskError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Join(error) => Some(error),
            _ => None,
        }
    }
}

/// Once admitted, Tokio owns the operation even if this waiter is cancelled.
/// Operation errors remain in T, separate from worker/admission failures.
pub(crate) async fn run<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> Result<T, BlockingTaskError> {
    spawn(work)?
        .await
        .map_err(BlockingTaskError::Join)?
        .ok_or(BlockingTaskError::WorkNotStarted)
}

/// Admit work synchronously, retaining Tokio's ownership if its join is dropped.
/// On rejection, captured resources are dropped here, before returning the error.
pub(crate) fn spawn<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> Result<JoinHandle<Option<T>>, BlockingTaskError> {
    let runtime = Handle::try_current().map_err(|_| BlockingTaskError::RuntimeUnavailable)?;
    admit(work, |receive| {
        #[cfg(test)]
        if REJECT_ADMISSION
            .try_with(|reject| reject.replace(false))
            .unwrap_or(false)
        {
            let error = std::io::Error::other("injected admission failure");
            panic!("OS can't spawn worker thread: {error}");
        }
        runtime.spawn_blocking(receive)
    })
}

fn admit<T: Send + 'static, F: FnOnce() -> T + Send + 'static>(
    work: F,
    spawn: impl FnOnce(Box<dyn FnOnce() -> Option<T> + Send>) -> JoinHandle<Option<T>>,
) -> Result<JoinHandle<Option<T>>, BlockingTaskError> {
    // Tokio 1.52.3 queues a task BEFORE thread creation can fail with NoThreads
    // (runtime/blocking/pool.rs). A queued task can execute after that panic.
    // Queue only the receiver; keep all operation resources on this side until
    // admission succeeds. A failed admission closes the channel without work.
    let (sender, receiver) = mpsc::sync_channel::<F>(1);
    let receive = Box::new(move || receiver.recv().ok().map(|work| work()));
    let worker = catch_unwind(AssertUnwindSafe(|| spawn(receive))).map_err(admission_error)?;
    // Capacity is one and this is the only send, so publication cannot wait for
    // the worker. There is no await/cancellation point between admission and send.
    sender
        .send(work)
        .map_err(|_| BlockingTaskError::WorkNotStarted)?;
    Ok(worker)
}

fn admission_error(payload: Box<dyn std::any::Any + Send>) -> BlockingTaskError {
    // Translate only the pinned pool's formatted OS failure, not assertions,
    // poisoned locks, or arbitrary panic payloads. No operation runs inside this
    // unwind boundary. Recheck this contract when upgrading Tokio.
    match payload.downcast::<String>() {
        Ok(message) if message.starts_with("OS can't spawn worker thread: ") => {
            BlockingTaskError::AdmissionFailed(*message)
        }
        Ok(payload) => resume_unwind(payload),
        Err(payload) => resume_unwind(payload),
    }
}

#[cfg(test)]
tokio::task_local! {
    static REJECT_ADMISSION: std::cell::Cell<bool>;
}

#[cfg(test)]
pub(crate) async fn reject_next_admission<T>(future: impl Future<Output = T>) -> T {
    REJECT_ADMISSION
        .scope(std::cell::Cell::new(true), future)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    struct DropCount(Arc<AtomicUsize>);

    impl Drop for DropCount {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn tokio_thread_creation_failure_does_not_run_or_retain_the_operation() {
        // A current-thread runtime starts without a worker. The impossible
        // stack size makes the blocking pool's first OS thread creation fail,
        // exercising the pinned dependency boundary without exhausting threads.
        let runtime = tokio::runtime::Builder::new_current_thread()
            .thread_stack_size(usize::MAX)
            .build()
            .unwrap();
        let drops = Arc::new(AtomicUsize::new(0));
        let resource = DropCount(drops.clone());
        let result = runtime.block_on(run(move || {
            drop(resource);
            panic!("rejected work ran");
        }));
        assert!(matches!(result, Err(BlockingTaskError::AdmissionFailed(_))));
        assert_eq!(drops.load(Ordering::SeqCst), 1);
        drop(runtime);

        let healthy = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        assert_eq!(healthy.block_on(run(|| 42)).unwrap(), 42);
    }

    #[tokio::test]
    async fn rejected_admission_releases_work_and_allows_follow_up() {
        let drops = Arc::new(AtomicUsize::new(0));
        let resource = DropCount(drops.clone());
        let result = reject_next_admission(run(move || {
            drop(resource);
            panic!("rejected work ran");
        }))
        .await;
        assert!(
            matches!(result, Err(BlockingTaskError::AdmissionFailed(message))
            if message.contains("injected admission failure"))
        );
        assert_eq!(drops.load(Ordering::SeqCst), 1);
        assert_eq!(run(|| 42).await.unwrap(), 42);
    }

    #[tokio::test]
    async fn a_wrapper_retained_after_admission_failure_never_receives_work() {
        let drops = Arc::new(AtomicUsize::new(0));
        let resource = DropCount(drops.clone());
        let mut queued = None;
        let result = admit(
            move || {
                drop(resource);
                panic!("rejected work ran");
            },
            |receive| {
                queued = Some(receive);
                let error = std::io::Error::other("injected OS failure");
                panic!("OS can't spawn worker thread: {error}");
            },
        );
        assert!(matches!(result, Err(BlockingTaskError::AdmissionFailed(_))));
        // Even a wrapper that has not been dropped must not retain resources.
        assert_eq!(drops.load(Ordering::SeqCst), 1);
        assert!(queued.unwrap()().is_none());
        assert_eq!(run(|| 42).await.unwrap(), 42);
    }

    #[tokio::test]
    async fn a_started_wrapper_exits_without_work_after_admission_failure() {
        let mut wrapper = None;
        let result = admit(
            || panic!("rejected work ran"),
            |receive| {
                wrapper = Some(tokio::task::spawn_blocking(receive));
                let error = std::io::Error::other("injected OS failure");
                panic!("OS can't spawn worker thread: {error}");
            },
        );
        assert!(matches!(result, Err(BlockingTaskError::AdmissionFailed(_))));
        assert!(wrapper.unwrap().await.unwrap().is_none());
    }

    #[tokio::test]
    async fn operation_errors_and_panics_remain_distinct_from_admission_errors() {
        assert_eq!(
            run(|| Err::<(), _>("operation failed")).await.unwrap(),
            Err("operation failed")
        );
        let result = run(|| panic!("OS can't spawn worker thread: operation panic")).await;
        assert!(matches!(result, Err(BlockingTaskError::Join(error)) if error.is_panic()));
        assert_eq!(run(|| 42).await.unwrap(), 42);
    }

    #[test]
    fn unrelated_admission_panics_are_resumed() {
        for payload in [
            Box::new("other failure".to_string()) as Box<dyn std::any::Any + Send>,
            Box::new(42_u32),
        ] {
            let result = catch_unwind(AssertUnwindSafe(|| {
                admit(|| (), |_| resume_unwind(payload))
            }));
            let payload = result.unwrap_err();
            assert!(
                payload
                    .downcast_ref::<String>()
                    .is_some_and(|message| message == "other failure")
                    || payload.downcast_ref::<u32>() == Some(&42)
            );
        }
    }

    #[test]
    fn missing_runtime_is_an_error() {
        let mut future = Box::pin(run(|| 42));
        let mut context = std::task::Context::from_waker(std::task::Waker::noop());
        assert!(matches!(
            future.as_mut().poll(&mut context),
            std::task::Poll::Ready(Err(BlockingTaskError::RuntimeUnavailable))
        ));
    }

    #[tokio::test]
    async fn a_worker_cancelled_before_publication_returns_an_error() {
        let result = admit(
            || panic!("cancelled work ran"),
            |receive| {
                drop(receive);
                tokio::spawn(async { None })
            },
        );
        assert!(matches!(result, Err(BlockingTaskError::WorkNotStarted)));
    }

    #[test]
    fn shutdown_runtime_cancels_the_join() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let handle = runtime.handle().clone();
        drop(runtime);
        let _entered = handle.enter();
        let result = futures::executor::block_on(run(|| panic!("shutdown work ran")));
        assert!(matches!(
            result,
            Err(BlockingTaskError::WorkNotStarted | BlockingTaskError::Join(_))
        ));
    }

    #[test]
    fn shutdown_drains_admitted_work_that_has_not_started() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .max_blocking_threads(1)
            .build()
            .unwrap();
        let (release, gate) = mpsc::sync_channel(1);
        let (started, ready) = mpsc::sync_channel(1);
        let occupied = runtime.spawn_blocking(move || {
            started.send(()).unwrap();
            gate.recv_timeout(Duration::from_secs(5)).unwrap();
        });
        ready.recv_timeout(Duration::from_secs(5)).unwrap();
        let mut future = Box::pin(run(|| 42));
        {
            let _entered = runtime.enter();
            let mut context = std::task::Context::from_waker(std::task::Waker::noop());
            assert!(future.as_mut().poll(&mut context).is_pending());
        }
        runtime.shutdown_background();
        release.send(()).unwrap();
        futures::executor::block_on(occupied).unwrap();
        assert_eq!(futures::executor::block_on(future).unwrap(), 42);
    }

    #[tokio::test]
    async fn cancelled_waiter_leaves_work_and_cleanup_owned_by_the_pool() {
        let drops = Arc::new(AtomicUsize::new(0));
        let resource = DropCount(drops.clone());
        let (started, ready) = tokio::sync::oneshot::channel();
        let (release, gate) = mpsc::sync_channel(1);
        let (finished, completed) = tokio::sync::oneshot::channel();
        let waiter = tokio::spawn(run(move || {
            started.send(()).unwrap();
            gate.recv_timeout(Duration::from_secs(5)).unwrap();
            drop(resource);
            finished.send(()).unwrap();
        }));
        ready.await.unwrap();
        waiter.abort();
        assert!(waiter.await.unwrap_err().is_cancelled());
        assert_eq!(drops.load(Ordering::SeqCst), 0);
        release.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(5), completed)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(drops.load(Ordering::SeqCst), 1);
    }
}
