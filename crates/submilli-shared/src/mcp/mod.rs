//! Outbound MCP support shared by the server and CLI: `.well-known` OAuth
//! discovery, tool-catalog discovery (`tools/list` → the typed `@mcp/<server>`
//! surface), and the streamable-HTTP call transport.
//!
//! The server's MCP *serving* side (the rmcp `StreamableHttpService`, its
//! session store, and the axum router) stays in `submilli-server`; only the
//! outbound client pieces both binaries need live here.

pub mod catalog;
pub mod discovery;
pub mod oauth;
pub mod schema_registry;
pub mod transport;

pub use discovery::{McpCatalog, discover_all, discover_selected};
pub use transport::StreamableHttpTransport;

/// A diagnostic raised while mapping a server's `tools/list` to the typed
/// `@mcp/<server>` surface. Tools whose argument schema can't be expressed in
/// the strict subset are dropped entirely; tools whose result can't be typed are
/// kept but return `unknown`. Either way the operator should know.
#[derive(Debug, Clone)]
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

    pub fn untyped_output(server: &str, tool: &str) -> Self {
        Self {
            server: server.to_string(),
            tool: tool.to_string(),
            message: format!(
                "tool `{tool}` returns unknown: its result schema isn't representable"
            ),
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
