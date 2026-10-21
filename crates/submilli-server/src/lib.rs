//! Submilli HTTP execution server.

pub mod audit;

mod adapters;
pub mod app;
mod app_tools;
mod application;
pub mod auth;
pub mod blueprint;
mod compiler_thread;
pub mod config;
pub mod database;
mod docs_scopes;
pub mod domain;
pub mod error;
mod execution_timeout;
mod finalizers;
mod graceful_shutdown;
pub mod handlers;
pub mod idempotency;
#[cfg(test)]
extern crate self as submilli_server;
pub mod filesystems;
mod import_archive;
pub mod logging;
mod mcp;
pub mod metrics;
pub mod packages;
#[cfg(test)]
#[path = "../tests/support/request_records.rs"]
mod request_records;
pub mod runner;
pub mod runs;
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
pub use serve::{prepare_blueprint_store, runtime, serve};
pub use submilli_shared::mcp_token::{McpTokenError, OAuthTokenManager};
pub use submilli_shared::secret_store::{FileSecretStore, KeySource};
pub use submilli_shared::secret_store::{SecretStore, SecretStoreError};
