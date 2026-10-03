//! Submilli HTTP execution server.

pub mod app;
pub mod auth;
pub mod blueprint;
mod compiler_thread;
pub mod config;
pub mod error;
mod execution_timeout;
pub mod handlers;
pub mod idempotency;
pub mod idempotency_store;
pub mod logging;
mod mcp;
pub mod metrics;
pub mod packages;
pub mod runner;
pub mod serve;
pub mod session;
pub mod session_manager;
pub mod session_store;
pub mod tls;
pub mod volumes;

pub use app::{AppState, app, route_table};
pub use auth::{Access, ApiToken, AuthConfig, Role};
pub use config::ServerConfig;
pub use interpreter::runtime::{
    DEFAULT_MAX_EXECUTION_TOKENS, DEFAULT_MAX_STORE_BYTES, LlmLimits, NetworkPolicy, RuntimeConfig,
};
pub use serve::{runtime, serve};
pub use submilli_shared::mcp_token::{McpTokenError, OAuthTokenManager};
pub use submilli_shared::secret_store::{FileSecretStore, KeySource};
pub use submilli_shared::secret_store::{SecretStore, SecretStoreError};
