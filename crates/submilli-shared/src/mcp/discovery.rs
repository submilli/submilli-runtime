//! Outbound discovery of a blueprint's MCP servers.
//!
//! At connection time the server calls `tools/list` on each declared `mcp:`
//! server and feeds the result to the [`catalog`](super::catalog) mapper, yielding
//! the `@mcp/<server>` virtual-package [`PackageDeclaration`]. A server that's down (or
//! whose auth fails) doesn't fail the connection: it is omitted from the catalog
//! and a warning explains why it is unavailable.
//!
//! Discovery does network I/O. The server caches catalogs until the blueprint or
//! its OAuth credentials change.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use crate::host::BlueprintSecretResolver;
use http::{HeaderName, HeaderValue};
use interpreter::PackageDeclaration;
use interpreter::packages::ModuleSummary;
use interpreter::stdlib::http::NetworkPolicy;
use rmcp::ServiceExt;
use rmcp::model::ClientInfo;
use rmcp::transport::StreamableHttpClientTransport;
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use serde_json::Value;
use submilli_blueprint::{Blueprint, HarnessSecretBindings, McpAuth, McpServer, interpolate};
use tracing::warn;

use crate::mcp::ToolWarning;
use crate::mcp::catalog::{ToolCatalogEntry, build_mcp_definitions, package_name};
use crate::mcp::schema_registry;
use crate::mcp_auth::{AuthState, blueprint_auth_state};
use crate::mcp_token::OAuthTokenManager;
use crate::secret_store::SecretStore;

/// The auth inputs discovery needs, decoupled from any server state: the secret
/// store (for `blueprint_auth_state` and static-`headers:` `${secrets.X}`) and
/// the OAuth token manager (for bearer tokens on `auth: oauth2` servers).
#[derive(Clone, Copy)]
pub struct DiscoveryAuth<'a> {
    pub secret_store: Option<&'a Arc<dyn SecretStore>>,
    pub oauth: Option<&'a Arc<OAuthTokenManager>>,
    pub harness_secrets: Option<&'a Arc<HarnessSecretBindings>>,
    /// The outbound-address policy the `tools/list` connection is made under;
    /// the same one every later `tools/call` uses.
    pub network_policy: &'a Arc<NetworkPolicy>,
}

/// `tools/list` must complete within this bound or the server is treated as
/// unreachable — a hung endpoint can't stall blueprint binding.
const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(10);

/// One reachable server's virtual package. Unavailable servers (unauthenticated or
/// unreachable) are omitted from the catalog entirely — see [`discover_all`].
pub struct McpPackage {
    pub server: String,
    pub defs: PackageDeclaration,
    /// Tools dropped or degraded while mapping this server's schemas.
    pub warnings: Vec<ToolWarning>,
}

impl McpPackage {
    /// The importable package name, `@mcp/<server>`.
    pub fn name(&self) -> String {
        package_name(&self.server)
    }

    /// One-line summary for `packages.docs`/`search`: the tool count.
    pub fn description(&self) -> String {
        let n = self.defs.values.len();
        format!("MCP server '{}' ({n} tool{})", self.server, plural(n))
    }

    /// Whether `q` (already lowercased) matches the package name, server name, or
    /// a tool/type name. An empty `q` matches.
    fn matches_query(&self, q: &str) -> bool {
        if q.is_empty() {
            return true;
        }
        if self.name().to_lowercase().contains(q) || self.server.to_lowercase().contains(q) {
            return true;
        }
        self.defs
            .values
            .keys()
            .chain(self.defs.types.keys())
            .any(|k| k.to_lowercase().contains(q))
    }
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

/// The `@mcp/*` packages for one blueprint, discovered once and cached.
pub struct McpCatalog {
    packages: Vec<McpPackage>,
    /// Server-level warnings for servers omitted from the catalog (unauthenticated
    /// or unreachable). Surfaced alongside per-tool warnings.
    unavailable: Vec<ToolWarning>,
}

impl McpCatalog {
    pub fn empty() -> Self {
        Self {
            packages: Vec::new(),
            unavailable: Vec::new(),
        }
    }

    /// The package definitions, as the slice `compile_script` wants.
    pub fn defs_refs(&self) -> Vec<&PackageDeclaration> {
        self.packages.iter().map(|p| &p.defs).collect()
    }

    /// Find a package by its full name (`@mcp/<server>`), for `packages.docs`.
    pub fn package(&self, full_name: &str) -> Option<&McpPackage> {
        self.packages
            .iter()
            .find(|p| package_name(&p.server) == full_name)
    }

    /// Every warning for surfacing to operators (server logs at discovery, and the
    /// execute response): per-tool warnings plus omitted-server notices.
    pub fn warnings(&self) -> impl Iterator<Item = &ToolWarning> {
        self.packages
            .iter()
            .flat_map(|p| p.warnings.iter())
            .chain(self.unavailable.iter())
    }

    /// Why a declared server could not be discovered.
    pub fn unavailable_reason(&self, server: &str) -> Option<&str> {
        self.unavailable
            .iter()
            .find(|warning| warning.server == server)
            .map(|warning| warning.message.as_str())
    }

    /// `@mcp/<server>` packages matching `query` (case-insensitive, against the
    /// package name, server name, or a tool name), for `packages.search`. An
    /// empty query lists every declared server.
    pub fn search(&self, query: &str) -> Vec<ModuleSummary> {
        let q = query.trim().to_lowercase();
        self.packages
            .iter()
            .filter(|p| p.matches_query(&q))
            .map(|p| ModuleSummary {
                name: p.name(),
                description: p.description(),
            })
            .collect()
    }
}

/// Discover every server in the blueprint's `mcp:` block. Servers that are
/// unavailable — an unauthenticated OAuth server, or one that fails/times out at
/// discovery — are **omitted** from the catalog (not importable), each recorded as
/// a server-level warning, so the rest of the blueprint stays usable.
pub async fn discover_all(
    auth: DiscoveryAuth<'_>,
    blueprint_name: &str,
    blueprint: &Blueprint,
) -> McpCatalog {
    discover_matching(auth, blueprint_name, blueprint, None).await
}

pub async fn discover_selected(
    auth: DiscoveryAuth<'_>,
    blueprint_name: &str,
    blueprint: &Blueprint,
    servers: &BTreeSet<String>,
) -> McpCatalog {
    discover_matching(auth, blueprint_name, blueprint, Some(servers)).await
}

async fn discover_matching(
    auth: DiscoveryAuth<'_>,
    blueprint_name: &str,
    blueprint: &Blueprint,
    servers: Option<&BTreeSet<String>>,
) -> McpCatalog {
    let unauthenticated: HashSet<String> =
        match blueprint_auth_state(blueprint, auth.secret_store).await {
            AuthState::Pending { unauthenticated } => unauthenticated.into_iter().collect(),
            AuthState::Active => HashSet::new(),
        };

    let mut packages = Vec::with_capacity(blueprint.mcp.len());
    let mut unavailable = Vec::new();
    for (server_name, server) in &blueprint.mcp {
        if servers.is_some_and(|selected| !selected.contains(server_name)) {
            continue;
        }
        if unauthenticated.contains(server_name) {
            warn!(
                server = server_name,
                "MCP server omitted: not authenticated — run `submilli server mcp authenticate {blueprint_name} {server_name}`"
            );
            unavailable.push(ToolWarning::server_unavailable(
                server_name,
                "not authenticated — run `submilli server mcp authenticate`",
            ));
            continue;
        }
        match discover_server(auth, blueprint_name, blueprint, server_name, server).await {
            Ok(pkg) => packages.push(pkg),
            Err(reason) => unavailable.push(ToolWarning::server_unavailable(server_name, &reason)),
        }
    }
    McpCatalog {
        packages,
        unavailable,
    }
}

/// Discover one server's tools, preserving its failure reason for callers.
async fn discover_server(
    auth: DiscoveryAuth<'_>,
    blueprint_name: &str,
    blueprint: &Blueprint,
    server_name: &str,
    server: &McpServer,
) -> Result<McpPackage, String> {
    let fetch = fetch_tools(auth, blueprint_name, blueprint, server_name, server);
    match tokio::time::timeout(DISCOVERY_TIMEOUT, fetch).await {
        Ok(Ok(tools)) => {
            // A built-in schema pack (matched by endpoint host) fills typed returns for
            // tools whose server publishes no representable `outputSchema`.
            let pack = schema_registry::pack_for_url(&server.url);
            if let Some(pack) = pack {
                tracing::info!(
                    server = server_name,
                    pack = pack.id(),
                    "MCP discovery: applying built-in output-schema pack"
                );
            }
            let (defs, warnings) = build_mcp_definitions(server_name, &tools, pack);
            for w in &warnings {
                warn!(server = server_name, tool = w.tool, "{}", w.message);
            }
            Ok(McpPackage {
                server: server_name.to_string(),
                defs,
                warnings,
            })
        }
        Ok(Err(err)) => {
            warn!(server = server_name, %err, "MCP discovery failed; @mcp/{server_name} omitted");
            Err(format!("{err:#}"))
        }
        Err(_) => {
            warn!(
                server = server_name,
                "MCP discovery timed out; @mcp/{server_name} omitted"
            );
            Err(format!(
                "discovery timed out after {} seconds",
                DISCOVERY_TIMEOUT.as_secs()
            ))
        }
    }
}

/// Connect to one server and return its tool catalog. `anyhow` here is server
/// glue — the error becomes a discovery warning.
async fn fetch_tools(
    auth: DiscoveryAuth<'_>,
    blueprint_name: &str,
    blueprint: &Blueprint,
    server_name: &str,
    server: &McpServer,
) -> anyhow::Result<Vec<ToolCatalogEntry>> {
    let (auth_header, custom_headers) =
        resolve_auth(auth, blueprint_name, blueprint, server_name, server).await?;

    // `StreamableHttpClientTransportConfig` is `#[non_exhaustive]`; build it via
    // the constructor and set fields rather than a struct literal.
    let mut config = StreamableHttpClientTransportConfig::with_uri(server.url.as_str());
    config.auth_header = auth_header.clone();
    config.custom_headers = custom_headers.clone();
    // Tolerate stateless MCP servers. Some (e.g. Linear with `Authorization:`
    // header auth) authenticate per request and never issue an `Mcp-Session-Id`;
    // with `allow_stateless = false` rmcp rejects their initialize response with
    // "missing session id". `true` works for both stateful and stateless servers
    // (a real 401/connection failure still errors).
    config.allow_stateless = true;
    tracing::info!(
        server = server_name,
        url = %server.url,
        auth_header = auth_header.is_some(),
        custom_headers = custom_headers.len(),
        "MCP discovery: connecting"
    );
    auth.network_policy
        .check_url(server.url.as_str())
        .await
        .map_err(anyhow::Error::msg)?;
    let transport = StreamableHttpClientTransport::with_client(
        crate::mcp::transport::policy_http_client(auth.network_policy),
        config,
    );
    let client = ClientInfo::default().serve(transport).await?;
    let tools = client.list_all_tools().await;
    // Best-effort shutdown regardless of the list outcome.
    let _ = client.cancel().await;
    let tools = tools?;

    Ok(tools
        .into_iter()
        .map(|t| ToolCatalogEntry {
            name: t.name.to_string(),
            description: t.description.map(|d| d.to_string()),
            input_schema: Value::Object((*t.input_schema).clone()),
            output_schema: t.output_schema.map(|s| Value::Object((*s).clone())),
        })
        .collect())
}

/// Resolve a server's auth into the transport's `(auth_header, custom_headers)`:
/// OAuth servers mint a bearer token; static-`headers:` servers resolve their
/// `${secrets.X}` placeholders into custom headers.
async fn resolve_auth(
    auth: DiscoveryAuth<'_>,
    blueprint_name: &str,
    blueprint: &Blueprint,
    server_name: &str,
    server: &McpServer,
) -> anyhow::Result<(Option<String>, HashMap<HeaderName, HeaderValue>)> {
    if let Some(McpAuth::Oauth2 { .. }) = &server.auth {
        let manager = auth.oauth.ok_or_else(|| {
            anyhow::anyhow!("OAuth server '{server_name}' needs a secret store, none configured")
        })?;
        let token = manager.access_token(blueprint_name, server_name).await?;
        // A `Bearer <token>` header is "badly formatted" upstream when the token is
        // empty or carries whitespace. Trim defensively, log when we had to (never
        // logging the token), and fail clearly on an empty token rather than
        // sending `Bearer `.
        let trimmed = token.trim();
        if trimmed.len() != token.len() {
            warn!(
                server = server_name,
                token_len = token.len(),
                trimmed_len = trimmed.len(),
                "MCP OAuth token had surrounding whitespace — trimmed"
            );
        }
        if trimmed.is_empty() {
            anyhow::bail!("OAuth token for '{server_name}' is empty — re-authenticate");
        }
        return Ok((Some(trimmed.to_string()), HashMap::new()));
    }

    let resolver = match auth.harness_secrets {
        Some(secrets) => {
            BlueprintSecretResolver::with_harness(auth.secret_store.cloned(), Arc::clone(secrets))
        }
        None => BlueprintSecretResolver::new(auth.secret_store.cloned()),
    };
    let mut headers = HashMap::with_capacity(server.headers.len());
    for (name, value) in &server.headers {
        let resolved = interpolate(value, blueprint, &resolver).await?;
        let header_name = HeaderName::from_bytes(name.as_bytes())?;
        let header_value = HeaderValue::from_str(&resolved)?;
        headers.insert(header_name, header_value);
    }
    Ok((None, headers))
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn discovery_reports_network_policy_failure() {
        let bp = submilli_blueprint::parse(
            "name: x\nmcp:\n  blocked:\n    url: http://127.0.0.1:1/mcp\n",
        )
        .unwrap();
        let policy = Arc::new(NetworkPolicy::deny_private());
        let catalog = discover_all(
            DiscoveryAuth {
                secret_store: None,
                oauth: None,
                harness_secrets: None,
                network_policy: &policy,
            },
            "x",
            &bp,
        )
        .await;
        let warnings: Vec<_> = catalog.warnings().collect();
        assert_eq!(warnings.len(), 1);
        assert!(
            warnings[0].message.contains("blocked by network policy"),
            "{}",
            warnings[0].message
        );
    }

    use super::*;
    use serde_json::json;

    fn server_pkg(server: &str, tools: &[&str]) -> McpPackage {
        let entries: Vec<ToolCatalogEntry> = tools
            .iter()
            .map(|t| ToolCatalogEntry {
                name: t.to_string(),
                description: None,
                input_schema: json!({ "type": "object", "properties": {} }),
                output_schema: None,
            })
            .collect();
        let (defs, _) = build_mcp_definitions(server, &entries, None);
        McpPackage {
            server: server.to_string(),
            defs,
            warnings: Vec::new(),
        }
    }

    fn catalog(packages: Vec<McpPackage>) -> McpCatalog {
        McpCatalog {
            packages,
            unavailable: Vec::new(),
        }
    }

    fn names(hits: &[ModuleSummary]) -> Vec<String> {
        hits.iter().map(|m| m.name.clone()).collect()
    }

    #[test]
    fn empty_query_lists_every_server() {
        let catalog = catalog(vec![
            server_pkg("linear", &["createIssue"]),
            server_pkg("github", &["openPr"]),
        ]);
        let got = names(&catalog.search(""));
        assert!(got.contains(&"@mcp/linear".to_string()), "{got:?}");
        assert!(got.contains(&"@mcp/github".to_string()), "{got:?}");
    }

    #[test]
    fn matches_by_server_and_package_name() {
        let catalog = catalog(vec![server_pkg("linear", &["createIssue"])]);
        assert_eq!(names(&catalog.search("linear")), vec!["@mcp/linear"]);
        // The `@mcp/` prefix means a bare "mcp" query finds every server.
        assert_eq!(names(&catalog.search("mcp")), vec!["@mcp/linear"]);
    }

    #[test]
    fn matches_by_tool_name_and_misses_otherwise() {
        let catalog = catalog(vec![
            server_pkg("linear", &["createIssue"]),
            server_pkg("github", &["openPr"]),
        ]);
        assert_eq!(names(&catalog.search("createissue")), vec!["@mcp/linear"]);
        assert!(catalog.search("nonexistent-tool").is_empty());
    }

    #[test]
    fn description_reports_tool_count() {
        assert_eq!(
            server_pkg("linear", &["a", "b"]).description(),
            "MCP server 'linear' (2 tools)"
        );
        assert_eq!(
            server_pkg("solo", &["only"]).description(),
            "MCP server 'solo' (1 tool)"
        );
    }

    #[test]
    fn unavailable_servers_are_omitted_but_warned() {
        let mut cat = catalog(vec![server_pkg("linear", &["createIssue"])]);
        cat.unavailable.push(ToolWarning::server_unavailable(
            "github",
            "not authenticated",
        ));
        // Omitted from search/packages…
        assert_eq!(names(&cat.search("")), vec!["@mcp/linear"]);
        assert!(cat.package("@mcp/github").is_none());
        // …but surfaced as a warning.
        assert!(
            cat.warnings()
                .any(|w| w.server == "github" && w.message.contains("not authenticated"))
        );
    }
}
