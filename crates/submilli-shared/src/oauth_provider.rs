//! Configured OAuth client apps for MCP servers.

/// One configured OAuth client app. `match_host` is matched against the
/// *authorization server's* host (e.g. `github.com`), discovered from the MCP
/// server's `.well-known` metadata — not the MCP URL's host.
#[derive(Clone, Debug)]
pub struct OAuthProvider {
    pub match_host: String,
    pub client_id: String,
    /// Raw secret reference, resolved at use: a literal, `${env.VAR}`, or
    /// `${secrets.KEY}` (resolved from the secret store). `None` for public
    /// clients.
    pub client_secret: Option<String>,
    pub scopes: Vec<String>,
}
