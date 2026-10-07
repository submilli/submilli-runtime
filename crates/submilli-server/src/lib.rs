//! Submilli HTTP execution server.

pub mod audit;

pub mod app;
pub mod auth;
pub mod blueprint;
mod compiler_thread;
pub mod config;
pub mod database;
pub mod error;
mod execution_timeout;
mod graceful_shutdown;
pub mod handlers;
pub mod idempotency;
pub mod idempotency_store;
pub mod local_apply;
pub mod logging;
mod mcp;
pub mod metrics;
pub mod packages;
pub mod record;
pub mod runner;
pub mod serve;
pub mod session;
pub mod session_manager;
pub mod session_store;
pub mod tls;
pub mod volumes;

pub use app::{AppState, app, route_table};
pub use auth::{Access, ApiToken, AuthConfig, Role};
pub use config::{PreExecute, PreExecuteHook, PreExecuteRefusal, RunTelemetry, ServerConfig};
pub use interpreter::runtime::{
    DEFAULT_MAX_EXECUTION_TOKENS, DEFAULT_MAX_STORE_BYTES, LlmLimits, NetworkPolicy, RuntimeConfig,
};
pub use local_apply::{LocalApplied, LocalApplyError, LocalDiagnostic};
#[cfg(unix)]
pub use serve::WatchedSignal;
pub use serve::{EmbeddedSignals, prepare_blueprint_store, runtime, serve, serve_embedded};
pub use submilli_shared::mcp_token::{McpTokenError, OAuthTokenManager};
pub use submilli_shared::secret_store::{FileSecretStore, KeySource};
pub use submilli_shared::secret_store::{SecretStore, SecretStoreError};
