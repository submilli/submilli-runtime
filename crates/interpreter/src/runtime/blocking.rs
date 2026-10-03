//! Blocking work whose cleanup belongs to an execution, not its request.

use std::fmt;

// Match Tokio’s default blocking admission bound. Repository-lock waiters
// must not consume Git’s smaller active-operation limit before taking the lock.
static WORKER_SLOTS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(512);

/// A failure of the worker boundary, rather than an operation's returned error.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockingWorkError {
    WorkerPanicked,
    WorkerCancelled,
    ResultChannelClosed,
    RuntimeUnavailable,
    AllocationFailed,
    ThreadSpawnFailed,
}

impl fmt::Display for BlockingWorkError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        out.write_str(match self {
            Self::WorkerPanicked => "blocking worker panicked",
            Self::WorkerCancelled => "blocking worker was cancelled before returning",
            Self::ResultChannelClosed => "blocking worker result channel closed before returning",
            Self::RuntimeUnavailable => "blocking worker requires a Tokio runtime",
            Self::AllocationFailed => "could not reserve blocking worker ownership records",
            Self::ThreadSpawnFailed => "could not create blocking worker thread",
        })
    }
}

impl std::error::Error for BlockingWorkError {}

/// All failures observed while draining abandoned workers.
#[derive(Debug)]
pub struct BlockingWorkDrainError {
    errors: Vec<BlockingWorkError>,
}

impl BlockingWorkDrainError {
    pub fn errors(&self) -> &[BlockingWorkError] {
        &self.errors
    }
}

impl fmt::Display for BlockingWorkDrainError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, error) in self.errors.iter().enumerate() {
            if index > 0 {
                out.write_str("; ")?;
            }
            error.fmt(out)?;
        }
        Ok(())
    }
}

impl std::error::Error for BlockingWorkDrainError {}

/// Embedders must finish draining before releasing execution resources.
/// Dropping a waiter leaves its actual blocking worker registered here.
#[derive(Default)]
pub struct BlockingWork {
    pending: Vec<Worker>,
    drain_failures: Vec<BlockingWorkError>,
}

impl BlockingWork {
    /// Run a synchronous closure on an owned native thread and await its result.
    /// At most 512 such threads are admitted across executions, independently
    /// of the Tokio runtime’s blocking-pool configuration.
    /// Cancellation leaves the join registered until another finish call drains it.
    pub async fn spawn<T: Send + 'static>(
        &mut self,
        work: impl FnOnce() -> T + Send + 'static,
    ) -> Result<T, BlockingWorkError> {
        self.spawn_with_slots(work, &WORKER_SLOTS).await
    }

    async fn spawn_with_slots<T: Send + 'static>(
        &mut self,
        work: impl FnOnce() -> T + Send + 'static,
        slots: &'static tokio::sync::Semaphore,
    ) -> Result<T, BlockingWorkError> {
        tokio::runtime::Handle::try_current().map_err(|_| BlockingWorkError::RuntimeUnavailable)?;
        let permit = slots
            .acquire()
            .await
            .map_err(|_| BlockingWorkError::WorkerCancelled)?;
        self.reserve_worker()?;
        let (sender, receiver) = tokio::sync::oneshot::channel();
        let worker = Worker::start(
            move || {
                // Undelivered results are destroyed before the worker completes.
                let _ = sender.send(work());
            },
            Some(permit),
        )?;
        self.pending.push(worker);
        self.join_last().await?;
        receive_result(receiver).await
    }

    /// Drain all owned work, even when one worker fails. Cancelling this future
    /// retains unfinished joins and failures already observed for a later call.
    pub async fn finish(&mut self) -> Result<(), BlockingWorkDrainError> {
        while !self.pending.is_empty() {
            if let Err(error) = self.join_last().await {
                // spawn reserved space for every outstanding worker's failure.
                self.drain_failures.push(error);
            }
        }
        if self.drain_failures.is_empty() {
            return Ok(());
        }
        Err(BlockingWorkDrainError {
            errors: std::mem::take(&mut self.drain_failures),
        })
    }

    fn reserve_worker(&mut self) -> Result<(), BlockingWorkError> {
        let outstanding = self
            .pending
            .len()
            .checked_add(1)
            .ok_or(BlockingWorkError::AllocationFailed)?;
        self.pending
            .try_reserve(1)
            .map_err(|_| BlockingWorkError::AllocationFailed)?;
        self.drain_failures
            .try_reserve(outstanding)
            .map_err(|_| BlockingWorkError::AllocationFailed)?;
        Ok(())
    }

    async fn join_last(&mut self) -> Result<(), BlockingWorkError> {
        let worker = self
            .pending
            .last_mut()
            .ok_or(BlockingWorkError::ResultChannelClosed)?;
        worker.wait().await;
        let worker = self
            .pending
            .pop()
            .ok_or(BlockingWorkError::ResultChannelClosed)?;
        worker.join()
    }
}

struct Worker {
    thread: std::thread::JoinHandle<Result<(), BlockingWorkError>>,
    completed: Option<tokio::sync::oneshot::Receiver<()>>,
}

impl Worker {
    fn start(
        work: impl FnOnce() + Send + 'static,
        permit: Option<tokio::sync::SemaphorePermit<'static>>,
    ) -> Result<Self, BlockingWorkError> {
        Self::start_with(std::thread::Builder::new(), work, permit)
    }

    fn start_with(
        builder: std::thread::Builder,
        work: impl FnOnce() + Send + 'static,
        permit: Option<tokio::sync::SemaphorePermit<'static>>,
    ) -> Result<Self, BlockingWorkError> {
        let runtime = tokio::runtime::Handle::try_current()
            .map_err(|_| BlockingWorkError::RuntimeUnavailable)?;
        let (sender, completed) = tokio::sync::oneshot::channel();
        let thread = builder
            .spawn(move || {
                let _permit = permit;
                let _completion = Completion(Some(sender));
                let _runtime = runtime.enter();
                // This is the worker panic boundary, equivalent to the pool’s
                // boundary. Payload cleanup stays on this owned thread.
                match std::panic::catch_unwind(std::panic::AssertUnwindSafe(work)) {
                    Ok(()) => Ok(()),
                    Err(payload) => {
                        dispose_panic_payload(payload);
                        Err(BlockingWorkError::WorkerPanicked)
                    }
                }
            })
            .map_err(|_| BlockingWorkError::ThreadSpawnFailed)?;
        Ok(Self {
            thread,
            completed: Some(completed),
        })
    }

    async fn wait(&mut self) {
        if let Some(completed) = self.completed.as_mut() {
            let _ = completed.await;
            self.completed = None;
        }
        // Completion wakes the owner during thread teardown. Never synchronously
        // join a thread whose remaining teardown could still block the executor.
        while !self.thread.is_finished() {
            tokio::task::yield_now().await;
        }
    }

    fn join(self) -> Result<(), BlockingWorkError> {
        match self.thread.join() {
            Ok(result) => result,
            Err(payload) => {
                dispose_panic_payload(payload);
                Err(BlockingWorkError::WorkerPanicked)
            }
        }
    }
}

struct Completion(Option<tokio::sync::oneshot::Sender<()>>);

impl Drop for Completion {
    fn drop(&mut self) {
        if let Some(sender) = self.0.take() {
            let _ = sender.send(());
        }
    }
}

fn dispose_panic_payload(mut payload: Box<dyn std::any::Any + Send>) {
    // Payload destructors are arbitrary embedder code. Dispose each payload
    // outside an active unwind; a destructor's replacement panic is disposed
    // iteratively, preserving ownership without recursive drop or leaking it.
    loop {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(payload))) {
            Ok(()) => return,
            Err(replacement) => payload = replacement,
        }
    }
}

async fn receive_result<T>(
    receiver: tokio::sync::oneshot::Receiver<T>,
) -> Result<T, BlockingWorkError> {
    receiver
        .await
        .map_err(|_| BlockingWorkError::ResultChannelClosed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn worker_panics_return_errors_and_allow_follow_up() {
        let mut workers = BlockingWork::default();
        assert_eq!(
            workers.spawn(|| panic!("injected worker panic")).await,
            Err(BlockingWorkError::WorkerPanicked)
        );
        assert_eq!(
            workers.spawn(|| std::panic::panic_any(42_u32)).await,
            Err(BlockingWorkError::WorkerPanicked)
        );
        assert!(workers.finish().await.is_ok());
        assert_eq!(workers.spawn(|| 42).await, Ok(42));
        assert_eq!(
            workers.spawn(|| Err::<(), _>("operation failed")).await,
            Ok(Err("operation failed"))
        );
    }

    #[test]
    fn spawning_without_runtime_returns_error() {
        let mut workers = BlockingWork::default();
        let mut future = Box::pin(workers.spawn(|| 42));
        let mut context = std::task::Context::from_waker(std::task::Waker::noop());
        assert!(matches!(
            std::future::Future::poll(future.as_mut(), &mut context),
            std::task::Poll::Ready(Err(BlockingWorkError::RuntimeUnavailable))
        ));
        drop(future);
        assert!(workers.pending.is_empty());
    }

    #[tokio::test]
    async fn closed_result_channel_returns_error() {
        let (sender, receiver) = tokio::sync::oneshot::channel::<()>();
        drop(sender);
        assert_eq!(
            receive_result(receiver).await,
            Err(BlockingWorkError::ResultChannelClosed)
        );
    }

    #[tokio::test]
    async fn thread_admission_failure_returns_error_and_allows_follow_up() {
        let result = Worker::start_with(
            std::thread::Builder::new().stack_size(usize::MAX),
            || {},
            None,
        );
        assert!(matches!(result, Err(BlockingWorkError::ThreadSpawnFailed)));
        let mut workers = BlockingWork::default();
        assert_eq!(workers.spawn(|| 42).await, Ok(42));
        workers.finish().await.unwrap();
    }

    #[tokio::test]
    async fn panicking_payload_destructors_are_disposed_without_interrupting_drain() {
        struct Payload(std::sync::Arc<std::sync::atomic::AtomicUsize>, u8);
        impl Drop for Payload {
            fn drop(&mut self) {
                self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                if self.1 > 0 {
                    std::panic::panic_any(Payload(self.0.clone(), self.1 - 1));
                }
            }
        }
        let drops = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mark = drops.clone();
        let mut workers = BlockingWork::default();
        assert_eq!(
            workers
                .spawn(move || std::panic::panic_any(Payload(mark, 3)))
                .await,
            Err(BlockingWorkError::WorkerPanicked)
        );
        assert_eq!(drops.load(std::sync::atomic::Ordering::SeqCst), 4);
        assert_eq!(workers.spawn(|| 42).await, Ok(42));
        workers.finish().await.unwrap();
    }

    #[tokio::test]
    async fn native_workers_preserve_runtime_context() {
        let mut workers = BlockingWork::default();
        assert_eq!(
            workers
                .spawn(|| { tokio::runtime::Handle::current().block_on(async { 42 }) })
                .await,
            Ok(42)
        );
    }

    #[tokio::test]
    async fn admission_waits_for_abandoned_worker_cleanup_and_can_be_cancelled() {
        static SLOTS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(1);
        let mut first = BlockingWork::default();
        let mut second = BlockingWork::default();
        let (started, ready) = tokio::sync::oneshot::channel();
        let (release, gate) = std::sync::mpsc::channel();
        let mut waiter = Box::pin(first.spawn_with_slots(
            move || {
                started.send(()).unwrap();
                gate.recv_timeout(Duration::from_secs(5)).unwrap();
            },
            &SLOTS,
        ));
        tokio::select! {
            result = &mut waiter => panic!("worker ended early: {result:?}"),
            result = ready => result.unwrap(),
        }
        drop(waiter);
        let called = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let mark = called.clone();
        assert!(
            tokio::time::timeout(
                Duration::from_millis(20),
                second.spawn_with_slots(
                    move || mark.store(true, std::sync::atomic::Ordering::SeqCst),
                    &SLOTS
                )
            )
            .await
            .is_err()
        );
        assert!(second.pending.is_empty());
        assert!(!called.load(std::sync::atomic::Ordering::SeqCst));
        release.send(()).unwrap();
        first.finish().await.unwrap();
        assert_eq!(second.spawn_with_slots(|| 42, &SLOTS).await, Ok(42));
    }

    #[tokio::test]
    async fn completed_abandoned_worker_releases_admission_before_join() {
        static SLOTS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(1);
        let mut workers = BlockingWork::default();
        let (started, ready) = tokio::sync::oneshot::channel();
        let (release, gate) = std::sync::mpsc::channel();
        let mut waiter = Box::pin(workers.spawn_with_slots(
            move || {
                started.send(()).unwrap();
                gate.recv_timeout(Duration::from_secs(5)).unwrap();
                panic!("abandoned failure");
            },
            &SLOTS,
        ));
        tokio::select! {
            result = &mut waiter => panic!("worker ended early: {result:?}"),
            result = ready => result.unwrap(),
        }
        drop(waiter);
        release.send(()).unwrap();
        assert_eq!(
            tokio::time::timeout(
                Duration::from_secs(5),
                workers.spawn_with_slots(|| 42, &SLOTS)
            )
            .await
            .unwrap(),
            Ok(42)
        );
        assert_eq!(workers.pending.len(), 1);
        assert_eq!(
            workers.finish().await.unwrap_err().errors(),
            &[BlockingWorkError::WorkerPanicked]
        );
    }

    #[tokio::test]
    async fn payload_cleanup_stays_owned_and_does_not_block_executor() {
        struct Payload {
            started: Option<tokio::sync::oneshot::Sender<()>>,
            gate: std::sync::mpsc::Receiver<()>,
        }
        impl Drop for Payload {
            fn drop(&mut self) {
                self.started.take().unwrap().send(()).unwrap();
                self.gate.recv_timeout(Duration::from_secs(5)).unwrap();
                panic!("payload destructor failure");
            }
        }
        let mut workers = BlockingWork::default();
        let (started, ready) = tokio::sync::oneshot::channel();
        let (release, gate) = std::sync::mpsc::channel();
        let mut waiter = Box::pin(workers.spawn(move || {
            std::panic::panic_any(Payload {
                started: Some(started),
                gate,
            });
        }));
        tokio::select! {
            result = &mut waiter => panic!("worker ended early: {result:?}"),
            result = ready => result.unwrap(),
        }
        drop(waiter);
        assert!(
            tokio::time::timeout(Duration::from_millis(20), workers.finish())
                .await
                .is_err()
        );
        assert_eq!(workers.pending.len(), 1);
        release.send(()).unwrap();
        let failures = workers.finish().await.unwrap_err();
        assert_eq!(failures.errors(), &[BlockingWorkError::WorkerPanicked]);
        assert_eq!(workers.spawn(|| 42).await, Ok(42));
    }

    #[tokio::test]
    async fn cancelled_waiter_retains_actual_worker_and_result_cleanup() {
        struct Output(std::sync::Arc<std::sync::atomic::AtomicBool>);
        impl Drop for Output {
            fn drop(&mut self) {
                self.0.store(true, std::sync::atomic::Ordering::SeqCst);
            }
        }
        let mut workers = BlockingWork::default();
        let (started, ready) = tokio::sync::oneshot::channel();
        let (release, gate) = std::sync::mpsc::channel();
        let dropped = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let mark = dropped.clone();
        let mut waiter = Box::pin(workers.spawn(move || {
            started.send(()).unwrap();
            gate.recv_timeout(Duration::from_secs(5)).unwrap();
            Output(mark)
        }));
        tokio::select! {
            result = &mut waiter => panic!("worker returned early: {}", result.is_ok()),
            result = ready => result.unwrap(),
        }
        drop(waiter);
        assert_eq!(workers.spawn(|| 42).await, Ok(42));
        assert_eq!(workers.pending.len(), 1);
        let mut finish = Box::pin(workers.finish());
        assert!(
            tokio::time::timeout(Duration::from_millis(20), &mut finish)
                .await
                .is_err()
        );
        assert!(!dropped.load(std::sync::atomic::Ordering::SeqCst));
        release.send(()).unwrap();
        finish.await.unwrap();
        assert!(dropped.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[tokio::test]
    async fn cancelled_drain_retains_handles_and_every_observed_failure() {
        let mut workers = BlockingWork::default();
        let (release, gate) = std::sync::mpsc::channel();
        workers.reserve_worker().unwrap();
        workers.pending.push(
            Worker::start(
                move || {
                    gate.recv_timeout(Duration::from_secs(5)).unwrap();
                    panic!("late failure");
                },
                None,
            )
            .unwrap(),
        );
        for _ in 0..2 {
            workers.reserve_worker().unwrap();
            let task = Worker::start(|| panic!("early failure"), None).unwrap();
            while !task.thread.is_finished() {
                tokio::task::yield_now().await;
            }
            workers.pending.push(task);
        }
        assert!(
            tokio::time::timeout(Duration::from_millis(20), workers.finish())
                .await
                .is_err()
        );
        assert_eq!(workers.pending.len(), 1);
        assert_eq!(
            workers.drain_failures,
            vec![BlockingWorkError::WorkerPanicked; 2]
        );
        release.send(()).unwrap();
        let errors = workers.finish().await.unwrap_err();
        assert_eq!(errors.errors(), &[BlockingWorkError::WorkerPanicked; 3]);
        assert!(workers.pending.is_empty());
        assert!(workers.finish().await.is_ok());
    }
}
