//! `submilli blueprint add-mcp <name> <url>` — add an outbound MCP server to a
//! local `blueprint.yaml`, so operators don't hand-write the `mcp:` block.
//!
//! Static-key auth (`--header` / `--authorization-bearer`), OAuth (`--oauth` /
//! `--client-id`), or — given neither — a best-effort network probe of the
//! server's `.well-known/oauth-protected-resource` decides whether OAuth is
//! required. OAuth needs no client id by default: the CLI does Dynamic Client
//! Registration at `authenticate` time.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use submilli_blueprint::{Action, McpAuth, McpServer, PermissionRule};

const DEFAULT_FILE: &str = "blueprint.yaml";

#[derive(clap::Args)]
pub struct Args {
    /// Local identifier for the server: its key in the `mcp:` block and the
    /// import name `@mcp/<name>`.
    name: String,
    /// The MCP endpoint URL.
    url: String,
    /// Blueprint file to edit (default: ./blueprint.yaml).
    #[arg(long)]
    blueprint: Option<PathBuf>,
    /// Make this an OAuth server (`auth: type: oauth2`). A client id isn't
    /// required — the CLI does Dynamic Client Registration at authenticate time.
    #[arg(long)]
    oauth: bool,
    /// OAuth client id, for servers that need a pre-registered client. Implies
    /// `--oauth`. A literal or a `${secrets.X}` reference.
    #[arg(long)]
    client_id: Option<String>,
    /// OAuth scope to request (repeatable). Implies `--oauth`.
    #[arg(long = "scope")]
    scopes: Vec<String>,
    /// Static request header, `"Name: value"` (repeatable). Use `${secrets.X}`
    /// for credentials. Conflicts with the OAuth flags.
    #[arg(long = "header", conflicts_with_all = ["oauth", "client_id", "scopes"])]
    headers: Vec<String>,
    /// Shorthand for static API-key auth: sets the header
    /// `Authorization: Bearer ${secrets.<NAME>}`. The secret must already be
    /// declared (`submilli blueprint secret add`). Implies static auth, so the
    /// OAuth probe is skipped. Conflicts with the OAuth flags.
    #[arg(
        long = "authorization-bearer",
        value_name = "SECRET_NAME",
        conflicts_with_all = ["oauth", "client_id", "scopes"]
    )]
    authorization_bearer: Option<String>,
    /// Skip the network probe that auto-detects whether the server needs OAuth.
    #[arg(long)]
    no_probe: bool,
}

pub fn execute(args: Args) -> Result<ExitCode> {
    match run(&args) {
        Ok(message) => {
            println!("✓ {message}");
            Ok(ExitCode::SUCCESS)
        }
        Err(e) => {
            eprintln!("error: {e:#}");
            Ok(ExitCode::from(1))
        }
    }
}

fn run(args: &Args) -> Result<String> {
    let path = args
        .blueprint
        .clone()
        .unwrap_or_else(|| PathBuf::from(DEFAULT_FILE));
    let yaml = fs::read_to_string(&path).with_context(|| {
        format!(
            "reading {} (run `submilli blueprint init` first?)",
            path.display()
        )
    })?;
    let mut blueprint =
        submilli_blueprint::parse(&yaml).with_context(|| format!("parsing {}", path.display()))?;

    if blueprint.mcp.contains_key(&args.name) {
        bail!(
            "blueprint '{}' already declares an mcp server '{}'",
            blueprint.name,
            args.name
        );
    }

    if let Some(secret) = &args.authorization_bearer
        && !blueprint.secrets.contains_key(secret)
    {
        bail!(
            "--authorization-bearer references secret '{secret}', not declared in {}.\n  \
             Declare it first, e.g.: submilli blueprint secret add {secret} --env <ENV_VAR>",
            path.display()
        );
    }

    let (headers, auth) = resolve_auth(args)?;
    let kind = describe(&headers, &auth);
    blueprint.mcp.insert(
        args.name.clone(),
        McpServer {
            transport: "streamable_http".into(),
            url: args.url.clone(),
            headers,
            auth,
        },
    );

    // Gate the new server in the permission policy, deny-by-default (like
    // `blueprint init`), so it's visible for the operator to allow/constrain. Only
    // when a policy already exists — without one the capability is already denied
    // by default, so we leave the blueprint policy-free and tell the operator how
    // to opt in rather than materialize a lone `mcp.<server>` rule.
    let capability = format!("mcp.{}", args.name);
    let gated = blueprint.has_permission_policy();
    if gated {
        blueprint
            .permissions
            .entry("main".to_string())
            .or_default()
            .push(PermissionRule {
                capability: capability.clone(),
                filter: None,
                action: Action::Deny,
            });
    }

    let updated = submilli_blueprint::to_yaml(&blueprint);
    // Re-validate the whole file before writing (secret refs, headers XOR auth, …)
    // so we never leave an unparseable blueprint on disk.
    submilli_blueprint::parse(&updated)
        .context("the resulting blueprint is invalid — not written")?;
    fs::write(&path, &updated).with_context(|| format!("writing {}", path.display()))?;

    let mut message = format!(
        "added mcp server '{}' ({kind}) to {}",
        args.name,
        path.display()
    );
    if gated {
        message.push_str(&format!(
            "\n  Gated by `{capability}` (deny by default) — set its action to `allow` \
             (optionally `filter: tool == \"...\"`) to use it."
        ));
    } else {
        message.push_str(&format!(
            "\n  No `permissions:` block, so `{capability}` is denied by default; add a \
             `permissions:` rule allowing it (or set `default: allow`) to use it."
        ));
    }
    if kind == "oauth" {
        message.push_str(&format!(
            "\n  Authenticate locally: submilli mcp authenticate {} --blueprint {}\n  After applying to a server: submilli server mcp authenticate {} {}",
            args.name, path.display(), blueprint.name, args.name
        ));
    }
    Ok(message)
}

/// Decide the auth shape: static headers (`--header` / `--authorization-bearer`)
/// win; `--oauth`/`--client-id`/`--scope` request OAuth; otherwise a network
/// probe auto-detects it.
fn resolve_auth(args: &Args) -> Result<(BTreeMap<String, String>, Option<McpAuth>)> {
    let mut headers = parse_headers(&args.headers)?;
    if let Some(secret) = &args.authorization_bearer {
        if headers.contains_key("Authorization") {
            bail!("--authorization-bearer conflicts with an explicit `Authorization` --header");
        }
        headers.insert(
            "Authorization".into(),
            format!("Bearer ${{secrets.{secret}}}"),
        );
    }
    if !headers.is_empty() {
        return Ok((headers, None));
    }
    let oauth_requested = args.oauth || args.client_id.is_some() || !args.scopes.is_empty();
    let is_oauth = oauth_requested || (!args.no_probe && probe_requires_oauth(&args.url));
    if is_oauth {
        return Ok((
            BTreeMap::new(),
            Some(McpAuth::Oauth2 {
                client_id: args.client_id.clone(),
                authorization_endpoint: None,
                token_endpoint: None,
                scopes: args.scopes.clone(),
            }),
        ));
    }
    Ok((BTreeMap::new(), None))
}

fn describe(headers: &BTreeMap<String, String>, auth: &Option<McpAuth>) -> &'static str {
    if auth.is_some() {
        "oauth"
    } else if !headers.is_empty() {
        "static header auth"
    } else {
        "no auth"
    }
}

/// Parse `"Name: value"` header arguments into a map.
fn parse_headers(raw: &[String]) -> Result<BTreeMap<String, String>> {
    let mut headers = BTreeMap::new();
    for entry in raw {
        let (name, value) = entry
            .split_once(':')
            .with_context(|| format!("header '{entry}' must be in `Name: value` form"))?;
        let name = name.trim();
        if name.is_empty() {
            bail!("header '{entry}' has an empty name");
        }
        headers.insert(name.to_string(), value.trim().to_string());
    }
    Ok(headers)
}

/// Best-effort: `true` if the server advertises OAuth protected-resource metadata.
/// Any network/parse failure is treated as "not OAuth" (the operator can still
/// pass `--client-id` explicitly).
fn probe_requires_oauth(url: &str) -> bool {
    let Ok(origin) = origin(url) else {
        return false;
    };
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .build()
        .into();
    // RFC 9728 places the metadata path-aware (the resource's path appended after
    // `.well-known/...`); fall back to the bare origin for servers that serve there.
    for well_known in well_known_resource_urls(&origin, url) {
        let Ok(resp) = agent.get(&well_known).call() else {
            continue;
        };
        if resp.status().as_u16() != 200 {
            continue;
        }
        let has_as = resp
            .into_body()
            .read_json::<serde_json::Value>()
            .ok()
            .and_then(|v| v.get("authorization_servers").cloned())
            .and_then(|servers| servers.as_array().map(|a| !a.is_empty()))
            .unwrap_or(false);
        if has_as {
            return true;
        }
    }
    false
}

/// Path-aware then origin `.well-known/oauth-protected-resource` URLs for `url`
/// (whose origin is `origin`), most-specific first.
fn well_known_resource_urls(origin: &str, url: &str) -> Vec<String> {
    let path = url[origin.len()..]
        .split(['?', '#'])
        .next()
        .unwrap_or("")
        .trim_end_matches('/');
    let mut urls = Vec::new();
    if !path.is_empty() {
        urls.push(format!(
            "{origin}/.well-known/oauth-protected-resource{path}"
        ));
    }
    urls.push(format!("{origin}/.well-known/oauth-protected-resource"));
    urls
}

/// `scheme://host[:port]` of a URL.
fn origin(url: &str) -> Result<String> {
    let scheme_end = url.find("://").context("url is missing a scheme")? + 3;
    let host_len = url[scheme_end..]
        .find('/')
        .unwrap_or(url.len() - scheme_end);
    Ok(url[..scheme_end + host_len].to_string())
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::net::TcpListener;
    use std::path::Path;
    use std::thread;

    use super::*;

    fn args(name: &str, url: &str, path: &Path) -> Args {
        Args {
            name: name.into(),
            url: url.into(),
            blueprint: Some(path.to_path_buf()),
            oauth: false,
            client_id: None,
            scopes: Vec::new(),
            headers: Vec::new(),
            authorization_bearer: None,
            no_probe: true,
        }
    }

    fn temp_blueprint() -> (tempfile::TempDir, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("blueprint.yaml");
        fs::write(&path, "name: test\n").unwrap();
        (tmp, path)
    }

    fn temp_blueprint_with_policy() -> (tempfile::TempDir, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("blueprint.yaml");
        fs::write(&path, "name: test\ndefault: deny\n").unwrap();
        (tmp, path)
    }

    #[test]
    fn gates_new_server_when_a_policy_exists() {
        let (_tmp, path) = temp_blueprint_with_policy();
        run(&args("linear", "https://mcp.linear.app/mcp", &path)).unwrap();
        let bp = submilli_blueprint::parse(&fs::read_to_string(&path).unwrap()).unwrap();
        let rule = bp.permissions["main"]
            .iter()
            .find(|r| r.capability == "mcp.linear")
            .expect("mcp.linear capability rule added");
        assert_eq!(rule.action, Action::Deny);
    }

    #[test]
    fn leaves_policy_free_blueprint_policy_free() {
        let (_tmp, path) = temp_blueprint();
        run(&args("linear", "https://x/mcp", &path)).unwrap();
        let bp = submilli_blueprint::parse(&fs::read_to_string(&path).unwrap()).unwrap();
        assert!(
            !bp.has_permission_policy(),
            "adding a server must not silently impose a policy"
        );
    }

    #[test]
    fn adds_plain_server() {
        let (_tmp, path) = temp_blueprint();
        run(&args("linear", "https://mcp.linear.app/mcp", &path)).unwrap();
        let bp = submilli_blueprint::parse(&fs::read_to_string(&path).unwrap()).unwrap();
        let server = &bp.mcp["linear"];
        assert_eq!(server.url, "https://mcp.linear.app/mcp");
        assert!(server.auth.is_none() && server.headers.is_empty());
    }

    #[test]
    fn adds_oauth_server() {
        let (_tmp, path) = temp_blueprint();
        let mut a = args("sf", "https://sf/mcp", &path);
        a.client_id = Some("client-123".into());
        a.scopes = vec!["api".into()];
        run(&a).unwrap();
        let bp = submilli_blueprint::parse(&fs::read_to_string(&path).unwrap()).unwrap();
        let McpAuth::Oauth2 {
            client_id, scopes, ..
        } = bp.mcp["sf"].auth.as_ref().unwrap();
        assert_eq!(client_id.as_deref(), Some("client-123"));
        assert_eq!(scopes, &["api"]);
    }

    #[test]
    fn oauth_without_client_id() {
        let (_tmp, path) = temp_blueprint();
        let mut a = args("notion", "https://mcp.notion.com/mcp", &path);
        a.oauth = true;
        run(&a).unwrap();
        let bp = submilli_blueprint::parse(&fs::read_to_string(&path).unwrap()).unwrap();
        let McpAuth::Oauth2 { client_id, .. } = bp.mcp["notion"].auth.as_ref().unwrap();
        assert!(client_id.is_none());
    }

    #[test]
    fn adds_static_header_server() {
        let (_tmp, path) = temp_blueprint();
        let mut a = args("gh", "https://api.githubcopilot.com/mcp/", &path);
        a.headers = vec!["Authorization: Bearer ghp_x".into()];
        run(&a).unwrap();
        let bp = submilli_blueprint::parse(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(bp.mcp["gh"].headers["Authorization"], "Bearer ghp_x");
    }

    #[test]
    fn authorization_bearer_adds_static_header_for_declared_secret() {
        let (_tmp, path) = temp_blueprint();
        fs::write(
            &path,
            "name: test\nsecrets:\n  LINEAR_API_KEY: { env: LINEAR_API_KEY }\n",
        )
        .unwrap();
        let mut a = args("linear", "https://mcp.linear.app/mcp", &path);
        a.authorization_bearer = Some("LINEAR_API_KEY".into());
        run(&a).unwrap();
        let bp = submilli_blueprint::parse(&fs::read_to_string(&path).unwrap()).unwrap();
        let server = &bp.mcp["linear"];
        assert_eq!(
            server.headers["Authorization"],
            "Bearer ${secrets.LINEAR_API_KEY}"
        );
        assert!(server.auth.is_none(), "bearer auth is static, not oauth");
    }

    #[test]
    fn authorization_bearer_requires_a_declared_secret() {
        let (_tmp, path) = temp_blueprint();
        let mut a = args("linear", "https://mcp.linear.app/mcp", &path);
        a.authorization_bearer = Some("MISSING".into());
        let err = run(&a).unwrap_err();
        assert!(err.to_string().contains("not declared"), "{err}");
    }

    #[test]
    fn authorization_bearer_conflicts_with_explicit_authorization_header() {
        let (_tmp, path) = temp_blueprint();
        fs::write(&path, "name: test\nsecrets:\n  K: { env: K }\n").unwrap();
        let mut a = args("linear", "https://mcp.linear.app/mcp", &path);
        a.headers = vec!["Authorization: Bearer x".into()];
        a.authorization_bearer = Some("K".into());
        let err = run(&a).unwrap_err();
        assert!(err.to_string().contains("conflicts"), "{err}");
    }

    #[test]
    fn rejects_duplicate_server() {
        let (_tmp, path) = temp_blueprint();
        run(&args("linear", "https://a/mcp", &path)).unwrap();
        let err = run(&args("linear", "https://b/mcp", &path)).unwrap_err();
        assert!(err.to_string().contains("already declares"), "{err}");
    }

    #[test]
    fn header_must_have_colon() {
        assert!(parse_headers(&["no-colon".into()]).is_err());
        assert!(parse_headers(&[": empty-name".into()]).is_err());
        let ok = parse_headers(&["X-Key: v".into()]).unwrap();
        assert_eq!(ok["X-Key"], "v");
    }

    #[test]
    fn origin_strips_path() {
        assert_eq!(
            origin("https://h.example.com/mcp/x").unwrap(),
            "https://h.example.com"
        );
        assert_eq!(
            origin("http://127.0.0.1:9/y").unwrap(),
            "http://127.0.0.1:9"
        );
    }

    /// Serve one canned JSON response on a localhost port; returns its base URL.
    fn mock_well_known(status: u16, body: &'static str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            }
        });
        base
    }

    #[test]
    fn probe_detects_oauth() {
        let base = mock_well_known(200, r#"{"authorization_servers":["https://idp"]}"#);
        assert!(probe_requires_oauth(&format!("{base}/mcp")));
    }

    #[test]
    fn probe_negative_on_404() {
        let base = mock_well_known(404, "{}");
        assert!(!probe_requires_oauth(&format!("{base}/mcp")));
    }
}
