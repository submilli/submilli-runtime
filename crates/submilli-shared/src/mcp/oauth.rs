//! Server-side MCP OAuth client: `.well-known` discovery, optional Dynamic Client
//! Registration, and the authorization-code → token exchange. Credentials (the
//! `client_secret`) live only in server config — never on the CLI or in a
//! blueprint. The CLI/demo drive the browser and forward the `code` here.
//!
//! All outbound calls go through the SSRF-governed [`HttpClient`].

use std::sync::Arc;

use interpreter::runtime::{HttpClient, HttpRequest};
use serde::Deserialize;

use crate::OAuthProvider;
use crate::secret_store::SecretStore;

const TIMEOUT_MS: u64 = 30_000;
const MAX_RESPONSE: u64 = 64 * 1024;

/// Resolved authorization-server endpoints for a provider.
pub struct Endpoints {
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub registration_endpoint: Option<String>,
    /// `scopes_supported` advertised by the protected-resource metadata (RFC
    /// 9728), used as the default scope set when none is configured.
    pub scopes_supported: Vec<String>,
}

/// A token-endpoint grant: a refresh token (preferred) and/or a static access
/// token. At least one is present.
pub struct TokenGrant {
    pub access_token: Option<String>,
    pub refresh_token: Option<String>,
}

#[derive(Deserialize)]
struct ProtectedResource {
    #[serde(default)]
    authorization_servers: Vec<String>,
    #[serde(default)]
    scopes_supported: Vec<String>,
}

#[derive(Deserialize)]
struct AsMetadata {
    authorization_endpoint: Option<String>,
    token_endpoint: Option<String>,
    registration_endpoint: Option<String>,
}

#[derive(Deserialize)]
struct Registration {
    client_id: String,
}

/// The bare host of a URL (no scheme, port, or path).
pub fn host_of(url: &str) -> Option<&str> {
    let authority = url.split_once("://")?.1.split(['/', '?', '#']).next()?;
    Some(authority.split(':').next().unwrap_or(authority))
}

/// Match a configured provider by the authorization-server host.
pub fn match_provider<'a>(
    providers: &'a [OAuthProvider],
    authorization_endpoint: &str,
) -> Option<&'a OAuthProvider> {
    let host = host_of(authorization_endpoint)?;
    providers.iter().find(|p| p.match_host == host)
}

/// A recognized secret reference: which source, and the name to look up.
enum SecretRef<'a> {
    Env(&'a str),
    Store(&'a str),
}

/// Classify `raw` as an `${env.X}` / `${secrets.X}` reference, or `None` for a
/// literal. The single recognizer both [`resolve_secret_ref`] and
/// [`is_secret_ref`] share, so a value can never be a reference to one and a
/// literal to the other.
fn parse_secret_ref(raw: &str) -> Option<SecretRef<'_>> {
    if let Some(var) = raw.strip_prefix("${env.").and_then(|s| s.strip_suffix('}')) {
        return Some(SecretRef::Env(var));
    }
    if let Some(key) = raw
        .strip_prefix("${secrets.")
        .and_then(|s| s.strip_suffix('}'))
    {
        return Some(SecretRef::Store(key));
    }
    None
}

/// Whether `raw` is a well-formed `${env.X}` / `${secrets.X}` reference (the
/// forms [`resolve_secret_ref`] resolves) rather than a literal. Callers that
/// *persist* a reference (e.g. `submilli mcp provider add --client-secret`)
/// validate with this so a malformed ref is rejected up front instead of being
/// silently stored and later sent verbatim as a literal.
pub fn is_secret_ref(raw: &str) -> bool {
    parse_secret_ref(raw).is_some()
}

/// Resolve a config secret reference: a literal, `${env.VAR}`, or `${secrets.KEY}`
/// (read from the server secret store).
pub async fn resolve_secret_ref(raw: &str, store: Option<&Arc<dyn SecretStore>>) -> Option<String> {
    match parse_secret_ref(raw) {
        Some(SecretRef::Env(var)) => std::env::var(var).ok().filter(|v| !v.is_empty()),
        Some(SecretRef::Store(key)) => store?.get(key).await.ok().flatten(),
        None => Some(raw.to_string()),
    }
}

/// Path-aware (RFC 9728 / 8414) then origin `.well-known/<name>` URLs.
fn well_known_urls(url: &str, name: &str) -> Vec<String> {
    let Some((scheme, rest)) = url.split_once("://") else {
        return Vec::new();
    };
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], rest[i..].split(['?', '#']).next().unwrap_or("")),
        None => (rest, ""),
    };
    let origin = format!("{scheme}://{authority}");
    let path = path.trim_end_matches('/');
    let mut urls = Vec::new();
    if !path.is_empty() {
        urls.push(format!("{origin}/.well-known/{name}{path}"));
    }
    urls.push(format!("{origin}/.well-known/{name}"));
    urls
}

/// GET a URL and return the body if the status is 200.
async fn get_ok(http: &Arc<dyn HttpClient>, url: &str) -> Option<Vec<u8>> {
    let req = HttpRequest {
        method: "GET".to_string(),
        url: url.to_string(),
        headers: vec![("accept".to_string(), "application/json".to_string())],
        body: Vec::new(),
        timeout_ms: TIMEOUT_MS,
        max_response_size: MAX_RESPONSE,
        decompress: false,
        transport_policy: None,
    };
    let resp = http.send(&req).await.ok()?;
    (resp.status == 200).then_some(resp.body)
}

/// Fetch the first metadata document that resolves, trying each `name` at its
/// path-aware and origin locations.
async fn fetch_well_known<T: serde::de::DeserializeOwned>(
    http: &Arc<dyn HttpClient>,
    url: &str,
    names: &[&str],
) -> Result<T, String> {
    for name in names {
        for candidate in well_known_urls(url, name) {
            if let Some(body) = get_ok(http, &candidate).await
                && let Ok(parsed) = serde_json::from_slice::<T>(&body)
            {
                return Ok(parsed);
            }
        }
    }
    Err(format!("no OAuth metadata found for {url}"))
}

/// Discover a provider's endpoints from the MCP server URL (RFC 9728 → 8414).
pub async fn discover(http: &Arc<dyn HttpClient>, mcp_url: &str) -> Result<Endpoints, String> {
    let ProtectedResource {
        authorization_servers,
        scopes_supported,
    } = fetch_well_known(http, mcp_url, &["oauth-protected-resource"]).await?;
    let issuer = authorization_servers
        .into_iter()
        .next()
        .ok_or("protected-resource metadata lists no authorization_servers")?;
    let meta: AsMetadata = fetch_well_known(
        http,
        &issuer,
        &["oauth-authorization-server", "openid-configuration"],
    )
    .await?;
    Ok(Endpoints {
        authorization_endpoint: meta
            .authorization_endpoint
            .ok_or("authorization server metadata has no authorization_endpoint")?,
        token_endpoint: meta
            .token_endpoint
            .ok_or("authorization server metadata has no token_endpoint")?,
        registration_endpoint: meta.registration_endpoint,
        scopes_supported,
    })
}

/// RFC 7591 Dynamic Client Registration for a public native client (used when no
/// provider is configured for the host but the AS advertises registration).
pub async fn register_client(
    http: &Arc<dyn HttpClient>,
    registration_endpoint: &str,
    redirect_uri: &str,
) -> Result<String, String> {
    let body = serde_json::json!({
        "client_name": "Submilli",
        "token_endpoint_auth_method": "none",
        "application_type": "native",
        "response_types": ["code"],
        "grant_types": ["authorization_code", "refresh_token"],
        "redirect_uris": [redirect_uri],
    });
    let req = HttpRequest {
        method: "POST".to_string(),
        url: registration_endpoint.to_string(),
        headers: vec![
            ("content-type".to_string(), "application/json".to_string()),
            ("accept".to_string(), "application/json".to_string()),
        ],
        body: serde_json::to_vec(&body).unwrap_or_default(),
        timeout_ms: TIMEOUT_MS,
        max_response_size: MAX_RESPONSE,
        decompress: false,
        transport_policy: None,
    };
    let resp = http
        .send(&req)
        .await
        .map_err(|e| format!("dynamic client registration failed: {e}"))?;
    if resp.status != 200 && resp.status != 201 {
        return Err(format!(
            "dynamic client registration returned HTTP {}",
            resp.status
        ));
    }
    let reg: Registration = serde_json::from_slice(&resp.body)
        .map_err(|_| "registration response did not include client_id".to_string())?;
    Ok(reg.client_id)
}

/// Exchange an authorization code (+ PKCE verifier) for a token. GitHub-aware:
/// asks for JSON, falls back to form parsing, and treats a `200` with an `error`
/// field as a failure.
#[allow(clippy::too_many_arguments)]
pub async fn exchange_code(
    http: &Arc<dyn HttpClient>,
    token_endpoint: &str,
    client_id: &str,
    client_secret: Option<&str>,
    code: &str,
    redirect_uri: &str,
    code_verifier: &str,
) -> Result<TokenGrant, String> {
    let body = {
        let mut form = url::form_urlencoded::Serializer::new(String::new());
        form.append_pair("grant_type", "authorization_code");
        form.append_pair("code", code);
        form.append_pair("redirect_uri", redirect_uri);
        form.append_pair("client_id", client_id);
        form.append_pair("code_verifier", code_verifier);
        if let Some(secret) = client_secret {
            form.append_pair("client_secret", secret);
        }
        form.finish().into_bytes()
    };
    let req = HttpRequest {
        method: "POST".to_string(),
        url: token_endpoint.to_string(),
        headers: vec![
            (
                "content-type".to_string(),
                "application/x-www-form-urlencoded".to_string(),
            ),
            ("accept".to_string(), "application/json".to_string()),
        ],
        body,
        timeout_ms: TIMEOUT_MS,
        max_response_size: MAX_RESPONSE,
        decompress: false,
        transport_policy: None,
    };
    let resp = http
        .send(&req)
        .await
        .map_err(|e| format!("token exchange request failed: {e}"))?;
    let fields = TokenFields::parse(&resp.body);
    if resp.status >= 400 || fields.error.is_some() {
        return Err(fields
            .error_description
            .or(fields.error)
            .unwrap_or_else(|| format!("token exchange returned HTTP {}", resp.status)));
    }
    if fields.access_token.is_none() && fields.refresh_token.is_none() {
        return Err("token response included neither refresh_token nor access_token".to_string());
    }
    Ok(TokenGrant {
        access_token: fields.access_token,
        refresh_token: fields.refresh_token,
    })
}

#[derive(Default, Deserialize)]
struct TokenFields {
    #[serde(default)]
    access_token: Option<String>,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    error_description: Option<String>,
}

impl TokenFields {
    /// Parse a token response as JSON, falling back to `application/x-www-form-
    /// urlencoded` (GitHub form-encodes even when sent `Accept: application/json`).
    fn parse(body: &[u8]) -> Self {
        if let Ok(parsed) = serde_json::from_slice::<TokenFields>(body) {
            return parsed;
        }
        let mut out = TokenFields::default();
        for (k, v) in url::form_urlencoded::parse(body) {
            match k.as_ref() {
                "access_token" => out.access_token = Some(v.into_owned()),
                "refresh_token" => out.refresh_token = Some(v.into_owned()),
                "error" => out.error = Some(v.into_owned()),
                "error_description" => out.error_description = Some(v.into_owned()),
                _ => {}
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn well_known_urls_are_path_aware_then_origin() {
        let urls = well_known_urls(
            "https://api.githubcopilot.com/mcp/",
            "oauth-protected-resource",
        );
        assert_eq!(
            urls,
            vec![
                "https://api.githubcopilot.com/.well-known/oauth-protected-resource/mcp"
                    .to_string(),
                "https://api.githubcopilot.com/.well-known/oauth-protected-resource".to_string(),
            ]
        );
        // No path → origin only.
        assert_eq!(
            well_known_urls("https://mcp.linear.app", "oauth-protected-resource"),
            vec!["https://mcp.linear.app/.well-known/oauth-protected-resource".to_string()]
        );
    }

    #[test]
    fn is_secret_ref_recognizes_only_well_formed_refs() {
        // Well-formed references.
        assert!(is_secret_ref("${env.FOO}"));
        assert!(is_secret_ref("${secrets.GH_SECRET}"));
        // Malformed / literal — must NOT be treated as references, or a caller
        // that persists a "reference" would silently store a literal secret.
        assert!(!is_secret_ref("ghs_literalsecret"));
        assert!(!is_secret_ref("${secrets.GH_SECRET")); // no closing brace
        assert!(!is_secret_ref("${FOO}")); // wrong namespace
        assert!(!is_secret_ref("${secrets.X} ")); // trailing junk
        assert!(!is_secret_ref(" ${secrets.X}")); // leading junk
        assert!(!is_secret_ref("${env.X}extra"));
    }

    #[tokio::test]
    async fn is_secret_ref_agrees_with_resolve_literal_fallback() {
        // Whatever `is_secret_ref` rejects, `resolve_secret_ref` must treat as a
        // literal (return it verbatim) — they share `parse_secret_ref`, so this
        // guards against the two ever drifting apart.
        for raw in ["${FOO}", "${secrets.X", "plain", "${env.X}x"] {
            assert!(!is_secret_ref(raw));
            assert_eq!(resolve_secret_ref(raw, None).await.as_deref(), Some(raw));
        }
    }

    #[test]
    fn host_of_strips_scheme_port_path() {
        assert_eq!(
            host_of("https://github.com/login/oauth/authorize"),
            Some("github.com")
        );
        assert_eq!(host_of("http://127.0.0.1:8128/x"), Some("127.0.0.1"));
        assert_eq!(host_of("notaurl"), None);
    }

    #[test]
    fn protected_resource_parses_scopes_supported() {
        let pr: ProtectedResource = serde_json::from_str(
            r#"{"authorization_servers":["https://github.com/login/oauth"],
                "scopes_supported":["repo","read:org","gist"]}"#,
        )
        .unwrap();
        assert_eq!(pr.authorization_servers, ["https://github.com/login/oauth"]);
        assert_eq!(pr.scopes_supported, ["repo", "read:org", "gist"]);

        // Absent `scopes_supported` defaults to empty (servers that don't advertise).
        let bare: ProtectedResource =
            serde_json::from_str(r#"{"authorization_servers":["https://x/y"]}"#).unwrap();
        assert!(bare.scopes_supported.is_empty());
    }

    #[test]
    fn token_fields_parse_json_and_form() {
        let json = TokenFields::parse(br#"{"access_token":"at","refresh_token":"rt"}"#);
        assert_eq!(json.access_token.as_deref(), Some("at"));
        assert_eq!(json.refresh_token.as_deref(), Some("rt"));

        let form = TokenFields::parse(b"access_token=gho_x&scope=repo&token_type=bearer");
        assert_eq!(form.access_token.as_deref(), Some("gho_x"));
        assert!(form.refresh_token.is_none());

        let err = TokenFields::parse(b"error=bad_verification_code&error_description=nope");
        assert_eq!(err.error.as_deref(), Some("bad_verification_code"));
    }
}
