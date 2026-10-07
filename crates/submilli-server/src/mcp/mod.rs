//! MCP streamable-HTTP transport.
//!
//! One MCP tool — `submilli__typescript__execute` — served over rmcp's
//! [`StreamableHttpService`]. The blueprint is bound by the endpoint path
//! (`/mcp/{blueprint}`): each blueprint gets its own lazily-built service,
//! cached on [`AppState`], with `stateful_mode` (and therefore the
//! `MCP-Session-Id` header) enabled exactly for `per_session` blueprints.
//!
//! The outbound client pieces (discovery, catalog, transport, OAuth) live in
//! `submilli-shared` so the CLI can run authenticated MCP servers locally; this
//! module is only the server's *serving* side.
//!
//! [`StreamableHttpService`]: rmcp::transport::streamable_http_server::StreamableHttpService
//! [`AppState`]: crate::app::AppState

mod protocol;
mod router;
mod server;
mod session;

pub(crate) use router::{BlueprintServiceCache, mcp_handler, new_service_cache};
pub(crate) use submilli_shared::mcp::discovery::{McpCatalog, discover_all, discover_selected};
