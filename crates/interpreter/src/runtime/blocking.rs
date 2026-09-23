//! Blocking work whose cleanup belongs to an execution, not its request.

#[derive(Default)]
pub struct BlockingWork {
    pending: Vec<tokio::task::JoinHandle<()>>,
}

impl BlockingWork {
    /// Run a synchronous closure on the blocking pool and await its result.
    /// An independent task retains the join if this future is cancelled.
    pub async fn spawn<T: Send + 'static>(
        &mut self,
        work: impl FnOnce() -> T + Send + 'static,
    ) -> T {
        self.pending.retain(|task| !task.is_finished());
        let worker = tokio::task::spawn_blocking(work);
        let (sender, receiver) = tokio::sync::oneshot::channel();
        self.pending.push(tokio::spawn(async move {
            let result = worker.await;
            let _ = sender.send(result);
        }));
        match receiver.await {
            Ok(Ok(result)) => result,
            Ok(Err(error)) if error.is_panic() => std::panic::resume_unwind(error.into_panic()),
            Ok(Err(error)) => panic!("blocking worker stopped before returning: {error}"),
            Err(_) => panic!("blocking worker owner stopped before returning"),
        }
    }

    /// The execution owner calls this after dropping a cancelled host future
    /// and before releasing its store, VFS, and resource reservations.
    pub async fn finish(&mut self) {
        for task in self.pending.drain(..) {
            let _ = task.await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::FutureExt;

    #[tokio::test]
    async fn worker_panic_preserves_payload_and_finishes_cleanup() {
        let mut workers = BlockingWork::default();
        let result = std::panic::AssertUnwindSafe(workers.spawn(|| {
            std::panic::panic_any(42_u32);
        }))
        .catch_unwind()
        .await;
        assert_eq!(*result.unwrap_err().downcast::<u32>().unwrap(), 42);
        workers.finish().await;
    }
}
