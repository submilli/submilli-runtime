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
pub mod library_visibility;
pub mod llm;
pub mod mcp;
pub mod mcp_auth;
pub mod mcp_token;
pub mod oauth_provider;
pub mod prompt;
pub mod secret_store;

pub use github::{FetchAuth, FetchedRepo, GithubError, GithubSpec, ResolvedRepo};
pub use host::{BlueprintAuthProxy, BlueprintSecretProvider, EnvFileSecretResolver, PolicyCheck};
pub use oauth_provider::OAuthProvider;

/// Resolve operator-owned Git identity against the session's variable bindings.
pub fn resolve_git(
    blueprint: &submilli_blueprint::Blueprint,
    variables: &submilli_blueprint::VarBindings,
) -> Result<Option<interpreter::stdlib::git::GitConfig>, submilli_blueprint::BlueprintError> {
    blueprint
        .git
        .as_ref()
        .map(|config| {
            let config = config.resolve(variables)?;
            Ok(interpreter::stdlib::git::GitConfig {
                name: config.identity.name,
                email: config.identity.email,
                username: config.username,
            })
        })
        .transpose()
}
