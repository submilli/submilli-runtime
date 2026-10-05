//! Request ownership independent of the HTTP connection.

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
pub(crate) struct RequestTasks {
    tasks: TaskTracker,
    stop: CancellationToken,
}

impl RequestTasks {
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

/// Run outside authentication/audit middleware so their task-local context and
/// completion hooks live as long as the handler, even after HTTP disconnects.
pub(crate) async fn run(
    State(tasks): State<Arc<RequestTasks>>,
    request: Request,
    next: Next,
) -> Response {
    // Register before checking admission: close + wait cannot miss an accepted
    // task racing with shutdown. A token acquired after close is rejected.
    let token = tasks.tasks.token();
    if tasks.tasks.is_closed() {
        return shutting_down();
    }
    let runtime = match tokio::runtime::Handle::try_current() {
        Ok(runtime) => runtime,
        Err(error) => {
            tracing::error!(%error, "request requires a Tokio runtime");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    let stop = tasks.stop.clone();
    let task = runtime.spawn(
        async move {
            let _token = token;
            tokio::select! {
                biased;
                () = stop.cancelled() => shutting_down(),
                response = next.run(request) => response,
            }
        }
        .in_current_span(),
    );
    match task.await {
        Ok(response) => response,
        Err(error) => {
            tracing::error!(%error, "request task failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

fn shutting_down() -> Response {
    (StatusCode::SERVICE_UNAVAILABLE, "server is shutting down").into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower::ServiceExt;

    async fn panicked_handler() -> Response {
        panic!("injected handler panic");
    }

    #[tokio::test]
    async fn closed_admission_does_not_run_handler() {
        let tasks = Arc::new(RequestTasks::default());
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
        let tasks = Arc::new(RequestTasks::default());
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
