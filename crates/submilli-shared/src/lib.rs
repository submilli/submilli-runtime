//! Infrastructure shared by the `submilli-server` binary and the `submilli`
//! CLI: the secret store, the MCP OAuth stack (auth-state, token minting,
//! discovery, and the outbound transport), the outbound model provider, and the
//! LLM-facing prompt renderer.
//!
//! None of this is axum- or server-bound — the CLI uses it to run authenticated
//! MCP servers and resolve `store:` secrets locally, with the same semantics the
//! server enforces in production.

pub mod github;
pub mod host;
pub mod llm;
pub mod mcp;
pub mod mcp_auth;
pub mod mcp_token;
pub mod oauth_provider;
pub mod prompt;
pub mod secret_store;

pub use github::{FetchedRepo, GithubError, GithubSpec, ResolvedRepo};
pub use host::{BlueprintAuthProxy, BlueprintSecretProvider, EnvFileSecretResolver, PolicyCheck};
pub use oauth_provider::OAuthProvider;
