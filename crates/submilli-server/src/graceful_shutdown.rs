//! Request ownership independent of the HTTP connection.

use std::future::Future;
use std::sync::Arc;

use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};
use tokio_util::{sync::CancellationToken, task::TaskTracker};
use tracing::Instrument;

#[derive(Default)]
pub(crate) struct GracefulShutdownTracker {
    tasks: TaskTracker,
    stop: CancellationToken,
}

impl GracefulShutdownTracker {
    /// Own and track work until completion, even if the waiter is dropped.
    /// Closing admission rejects new work; cancellation stops unfinished work.
    pub(crate) async fn watch<F>(&self, work: F) -> Result<F::Output, WatchError>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        // Use the tracker's own state for admission. A token registered before
        // close is counted by wait; a token registered after close is rejected.
        let token = self.tasks.token();
        if self.tasks.is_closed() {
            return Err(WatchError::ShuttingDown);
        }
        let runtime = tokio::runtime::Handle::try_current().map_err(WatchError::Runtime)?;
        let stop = self.stop.clone();
        runtime
            .spawn(
                async move {
                    let _token = token;
                    tokio::select! {
                        biased;
                        () = stop.cancelled() => Err(WatchError::ShuttingDown),
                        result = work => Ok(result),
                    }
                }
                .in_current_span(),
            )
            .await
            .map_err(WatchError::Task)?
    }

    pub(crate) fn close(&self) {
        self.tasks.close();
    }

    pub(crate) async fn wait(&self) {
        self.tasks.wait().await;
    }

    pub(crate) fn cancel(&self) {
        self.close();
        self.stop.cancel();
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum WatchError {
    #[error("server is shutting down")]
    ShuttingDown,
    #[error("owned work requires a Tokio runtime: {0}")]
    Runtime(#[source] tokio::runtime::TryCurrentError),
    #[error("owned task failed: {0}")]
    Task(#[source] tokio::task::JoinError),
}

/// Run outside authentication/audit middleware so their task-local context and
/// completion hooks live as long as the handler, even after HTTP disconnects.
pub(crate) async fn run(
    State(shutdown): State<Arc<GracefulShutdownTracker>>,
    request: Request,
    next: Next,
) -> Response {
    match shutdown.watch(next.run(request)).await {
        Ok(response) => response,
        Err(WatchError::ShuttingDown) => {
            (StatusCode::SERVICE_UNAVAILABLE, "server is shutting down").into_response()
        }
        Err(error) => {
            tracing::error!(%error, "request task failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower::ServiceExt;

    #[tokio::test]
    async fn watch_keeps_non_http_work_alive_after_waiter_cancellation() {
        let tracker = Arc::new(GracefulShutdownTracker::default());
        let started = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let (result_tx, result_rx) = tokio::sync::oneshot::channel();
        let waiter = {
            let tracker = Arc::clone(&tracker);
            let started = Arc::clone(&started);
            let release = Arc::clone(&release);
            tokio::spawn(async move {
                tracker
                    .watch(async move {
                        started.notify_one();
                        release.notified().await;
                        result_tx.send(42).unwrap();
                    })
                    .await
            })
        };
        started.notified().await;
        waiter.abort();
        assert!(waiter.await.unwrap_err().is_cancelled());
        tracker.close();
        release.notify_one();
        tokio::time::timeout(std::time::Duration::from_secs(1), tracker.wait())
            .await
            .unwrap();
        assert_eq!(result_rx.await.unwrap(), 42);
    }

    async fn panicked_handler() -> Response {
        panic!("injected handler panic");
    }

    #[tokio::test]
    async fn closed_admission_does_not_run_handler() {
        let tasks = Arc::new(GracefulShutdownTracker::default());
        tasks.close();
        let router = axum::Router::new()
            .route("/", axum::routing::get(panicked_handler))
            .layer(axum::middleware::from_fn_with_state(
                Arc::clone(&tasks),
                run,
            ));
        let response = router
            .oneshot(
                Request::builder()
                    .uri("/")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        tokio::time::timeout(std::time::Duration::from_secs(1), tasks.wait())
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn panicked_handler_returns_error_and_releases_tracking() {
        let tasks = Arc::new(GracefulShutdownTracker::default());
        let router = axum::Router::new()
            .route("/", axum::routing::get(panicked_handler))
            .route("/healthy", axum::routing::get(|| async { "healthy" }))
            .layer(axum::middleware::from_fn_with_state(
                Arc::clone(&tasks),
                run,
            ));
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let response = router
            .oneshot(
                Request::builder()
                    .uri("/healthy")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        tasks.close();
        tokio::time::timeout(std::time::Duration::from_secs(1), tasks.wait())
            .await
            .unwrap();
    }
}
