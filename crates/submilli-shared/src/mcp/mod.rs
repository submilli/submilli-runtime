//! Outbound MCP support shared by the server and CLI: `.well-known` OAuth
//! discovery, tool-catalog discovery (`tools/list` → the typed `@mcp/<server>`
//! surface), and the streamable-HTTP call transport.
//!
//! The server's MCP *serving* side (the rmcp `StreamableHttpService`, its
//! session store, and the axum router) stays in `submilli-server`; only the
//! outbound client pieces both binaries need live here.

mod bounded_client;
pub mod catalog;
pub mod discovery;
mod draining_transport;
pub mod oauth;
pub mod schema_registry;
pub mod transport;

pub use discovery::{DiscoveryError, McpCatalog, discover_all, discover_selected};
pub use transport::StreamableHttpTransport;

/// A diagnostic raised while mapping a server's `tools/list` to the typed
/// `@mcp/<server>` surface. Invalid or duplicate names are dropped; untyped
/// results are summarized per server. Unsupported input fields remain callable
/// as `unknown`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ToolWarning {
    pub server: String,
    pub tool: String,
    pub message: String,
}

impl ToolWarning {
    pub fn dropped(server: &str, tool: &str, reason: &str) -> Self {
        Self {
            server: server.to_string(),
            tool: tool.to_string(),
            message: format!("tool `{tool}` dropped: {reason}"),
        }
    }

    /// A server omitted from the catalog (unauthenticated or unreachable). The
    /// empty `tool` marks it server-level.
    pub fn server_unavailable(server: &str, reason: &str) -> Self {
        Self {
            server: server.to_string(),
            tool: String::new(),
            message: format!("server unavailable: {reason}"),
        }
    }
}
