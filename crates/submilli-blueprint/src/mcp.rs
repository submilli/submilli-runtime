//! The blueprint `mcp:` block — outbound MCP servers the script can reach
//! through `@mcp/<server>` virtual packages. Each entry names a server by local
//! identifier and carries its transport, endpoint, and auth (static `headers:`
//! or `auth: oauth2`). `${secrets.X}` in header / OAuth values resolves from the
//! `secrets:` block, exactly as `auth_proxy:` does.
//!
//! v1 ships HTTP transport only (`streamable_http`, with `sse` as an alias);
//! stdio is deferred. The actual `@mcp/<server>` codegen, transport, and OAuth
//! CLI land in later slices — this module only parses and validates the
//! declaration.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::auth_proxy::secret_refs;
use crate::{Blueprint, BlueprintError, Fault, yaml_path};

const CAPABILITY_PREFIX: &str = "mcp.";
const DEFAULT_TRANSPORT: &str = "streamable_http";

fn default_transport() -> String {
    DEFAULT_TRANSPORT.to_string()
}

fn is_default_transport(transport: &str) -> bool {
    transport == DEFAULT_TRANSPORT
}

/// One declared outbound MCP server. Auth is either a static `headers:` map or
/// an `auth:` block, never both (checked in [`validate_mcp`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct McpServer {
    /// `streamable_http` or its `sse` alias. Defaults to `streamable_http` — the
    /// only non-alias value v1 supports — so it can be omitted. Kept as a string
    /// so the validator can name the deferred `stdio` case explicitly rather than
    /// emit a generic serde "unknown variant".
    #[serde(
        default = "default_transport",
        skip_serializing_if = "is_default_transport"
    )]
    pub transport: String,
    pub url: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth: Option<McpAuth>,
}

/// OAuth client config for servers that require it. The refresh-token dance is
/// operator-driven (CLI, a later slice); this only declares the client.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum McpAuth {
    Oauth2 {
        /// Optional: omit it and the CLI does Dynamic Client Registration
        /// (RFC 7591) at authenticate time, the MCP-spec default. Provide one
        /// only for servers that require a pre-registered client.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        client_id: Option<String>,
        /// Discovery is the default; these override it for servers that don't
        /// speak the MCP authorization metadata spec.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        authorization_endpoint: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        token_endpoint: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        scopes: Vec<String>,
    },
}

impl McpAuth {
    /// Every string field that may carry a `${secrets.X}` placeholder.
    fn secret_bearing_values(&self) -> Vec<&str> {
        let McpAuth::Oauth2 {
            client_id,
            authorization_endpoint,
            token_endpoint,
            scopes,
        } = self;
        let mut values = Vec::new();
        values.extend(
            [client_id, authorization_endpoint, token_endpoint]
                .into_iter()
                .flatten()
                .map(String::as_str),
        );
        values.extend(scopes.iter().map(String::as_str));
        values
    }
}

/// Validate every `mcp:` entry, plus the `mcp.<server>` permission rules that
/// reference one. Runs even when `mcp:` is empty, so a dangling `mcp.foo`
/// permission rule is still caught.
pub(crate) fn validate_mcp(blueprint: &Blueprint) -> Result<(), BlueprintError> {
    for (name, server) in &blueprint.mcp {
        validate_transport(name, &server.transport)?;
        if server.url.trim().is_empty() {
            return Err(BlueprintError::InvalidMcp(Fault::at(
                yaml_path!["mcp", name, "url"],
                format!("mcp server '{name}' must set a 'url'"),
            )));
        }
        if !server.headers.is_empty() && server.auth.is_some() {
            return Err(BlueprintError::InvalidMcp(Fault::at(
                yaml_path!["mcp", name],
                format!(
                    "mcp server '{name}' sets both 'headers' and 'auth'; use one (static-key auth or OAuth)"
                ),
            )));
        }
        check_secret_refs(name, server, blueprint)?;
    }
    validate_permission_servers(blueprint)?;
    Ok(())
}

fn validate_transport(name: &str, transport: &str) -> Result<(), BlueprintError> {
    match transport {
        "streamable_http" | "sse" => Ok(()),
        "stdio" => Err(BlueprintError::InvalidMcp(Fault::at(
            yaml_path!["mcp", name, "transport"],
            format!(
                "mcp server '{name}': stdio transport is deferred for v1; use 'streamable_http'"
            ),
        ))),
        other => Err(BlueprintError::InvalidMcp(Fault::at(
            yaml_path!["mcp", name, "transport"],
            format!(
                "mcp server '{name}': unknown transport '{other}' (use 'streamable_http' or its 'sse' alias)"
            ),
        ))),
    }
}

/// Every `${secrets.X}` in a server's headers and OAuth fields names a declared
/// secret — the same load-time check `auth_proxy:` applies.
fn check_secret_refs(
    name: &str,
    server: &McpServer,
    blueprint: &Blueprint,
) -> Result<(), BlueprintError> {
    for (header, value) in &server.headers {
        for secret in secret_refs(value) {
            if !blueprint.secrets.contains_key(secret) {
                return Err(BlueprintError::InvalidMcp(Fault::at(
                    yaml_path!["mcp", name, "headers", header],
                    format!("mcp server '{name}' references undeclared secret '{secret}'"),
                )));
            }
        }
    }
    for value in server.auth.iter().flat_map(McpAuth::secret_bearing_values) {
        for secret in secret_refs(value) {
            if !blueprint.secrets.contains_key(secret) {
                return Err(BlueprintError::InvalidMcp(Fault::at(
                    yaml_path!["mcp", name, "auth"],
                    format!("mcp server '{name}' references undeclared secret '{secret}'"),
                )));
            }
        }
    }
    Ok(())
}

/// Every `mcp.<server>` permission rule, across all caller blocks, names a server
/// declared in the `mcp:` block.
fn validate_permission_servers(blueprint: &Blueprint) -> Result<(), BlueprintError> {
    for (caller, rules) in &blueprint.permissions {
        for (i, rule) in rules.iter().enumerate() {
            let Some(server) = mcp_capability_server(&rule.capability) else {
                continue;
            };
            if rule.capability.contains('/') {
                return Err(BlueprintError::InvalidMcp(Fault::at(
                    yaml_path!["permissions", caller, i, "capability"],
                    format!(
                        "permission rule '{}': use capability 'mcp.{server}' with a filter such as 'tool == \"name\"' instead of '/tool'",
                        rule.capability
                    ),
                )));
            }
            if !blueprint.mcp.contains_key(server) {
                return Err(BlueprintError::InvalidMcp(Fault::at(
                    yaml_path!["permissions", caller, i, "capability"],
                    format!(
                        "permission rule '{}' references undeclared mcp server '{server}'",
                        rule.capability
                    ),
                )));
            }
        }
    }
    Ok(())
}

/// The server name in an `mcp.<server>` capability, or `None` if the capability
/// isn't an MCP one. Splitting the legacy `/tool` suffix lets validation suggest
/// the supported capability and filter spelling.
fn mcp_capability_server(capability: &str) -> Option<&str> {
    let rest = capability.strip_prefix(CAPABILITY_PREFIX)?;
    Some(rest.split_once('/').map_or(rest, |(server, _)| server))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BlueprintError, parse, to_yaml};

    const HEADERS_BP: &str = "\
name: x
secrets:
  LINEAR_API_KEY: { store: LINEAR_API_KEY }
mcp:
  linear:
    transport: streamable_http
    url: https://mcp.linear.app/mcp
    headers:
      Authorization: \"Bearer ${secrets.LINEAR_API_KEY}\"
";

    const OAUTH_BP: &str = "\
name: x
secrets:
  SF_CLIENT_ID: { store: SF_CLIENT_ID }
mcp:
  salesforce:
    transport: streamable_http
    url: https://salesforce-mcp.example.com/mcp
    auth:
      type: oauth2
      client_id: \"${secrets.SF_CLIENT_ID}\"
      scopes: [\"api\", \"refresh_token\"]
";

    #[test]
    fn parses_header_server() {
        let b = parse(HEADERS_BP).unwrap();
        let s = &b.mcp["linear"];
        assert_eq!(s.transport, "streamable_http");
        assert_eq!(s.url, "https://mcp.linear.app/mcp");
        assert_eq!(
            s.headers["Authorization"],
            "Bearer ${secrets.LINEAR_API_KEY}"
        );
        assert!(s.auth.is_none());
    }

    #[test]
    fn parses_oauth_server() {
        let b = parse(OAUTH_BP).unwrap();
        let McpAuth::Oauth2 {
            client_id, scopes, ..
        } = b.mcp["salesforce"].auth.as_ref().unwrap();
        assert_eq!(client_id.as_deref(), Some("${secrets.SF_CLIENT_ID}"));
        assert_eq!(scopes, &["api", "refresh_token"]);
    }

    #[test]
    fn oauth_client_id_is_optional() {
        // No client_id → Dynamic Client Registration at authenticate time.
        let b = parse(
            "name: x\nmcp:\n  s:\n    url: https://e.com/mcp\n    auth:\n      type: oauth2\n",
        )
        .unwrap();
        let McpAuth::Oauth2 { client_id, .. } = b.mcp["s"].auth.as_ref().unwrap();
        assert!(client_id.is_none());
        // Round-trips without emitting an empty client_id.
        let yaml = to_yaml(&b);
        assert!(!yaml.contains("client_id"), "{yaml}");
        assert_eq!(parse(&yaml).unwrap(), b);
    }

    #[test]
    fn omitted_transport_defaults_to_streamable_http() {
        let b = parse("name: x\nmcp:\n  s:\n    url: https://example.com/mcp\n").unwrap();
        assert_eq!(b.mcp["s"].transport, "streamable_http");
        // Round-trips without re-emitting the defaulted transport.
        let yaml = to_yaml(&b);
        assert!(
            !yaml.contains("transport"),
            "default transport leaked: {yaml}"
        );
        assert_eq!(parse(&yaml).unwrap(), b);
    }

    #[test]
    fn sse_alias_accepted() {
        let b =
            parse("name: x\nmcp:\n  s:\n    transport: sse\n    url: https://example.com/mcp\n")
                .unwrap();
        assert_eq!(b.mcp["s"].transport, "sse");
    }

    #[test]
    fn stdio_transport_rejected_with_deferred_message() {
        let err =
            parse("name: x\nmcp:\n  s:\n    transport: stdio\n    url: https://example.com\n")
                .unwrap_err();
        match err {
            BlueprintError::InvalidMcp(msg) => {
                assert!(msg.message.contains("deferred"), "got {}", msg.message);
            }
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn unknown_transport_rejected() {
        let err =
            parse("name: x\nmcp:\n  s:\n    transport: carrier_pigeon\n    url: https://e.com\n")
                .unwrap_err();
        assert!(matches!(err, BlueprintError::InvalidMcp(_)), "got {err:?}");
    }

    #[test]
    fn missing_url_rejected() {
        let err = parse("name: x\nmcp:\n  s:\n    transport: streamable_http\n    url: \"\"\n")
            .unwrap_err();
        assert!(matches!(err, BlueprintError::InvalidMcp(_)), "got {err:?}");
    }

    #[test]
    fn headers_and_auth_both_set_rejected() {
        let yaml = "\
name: x
secrets:
  K: { store: K }
mcp:
  s:
    transport: streamable_http
    url: https://e.com/mcp
    headers:
      Authorization: \"Bearer ${secrets.K}\"
    auth:
      type: oauth2
      client_id: \"${secrets.K}\"
";
        let err = parse(yaml).unwrap_err();
        match err {
            BlueprintError::InvalidMcp(msg) => {
                assert!(msg.message.contains("both"), "got {}", msg.message);
            }
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn undeclared_secret_in_header_rejected() {
        let yaml = "\
name: x
mcp:
  s:
    transport: streamable_http
    url: https://e.com/mcp
    headers:
      Authorization: \"Bearer ${secrets.NOPE}\"
";
        let err = parse(yaml).unwrap_err();
        match err {
            BlueprintError::InvalidMcp(msg) => {
                assert!(msg.message.contains("NOPE"), "got {}", msg.message);
            }
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn undeclared_secret_in_oauth_client_id_rejected() {
        let yaml = "\
name: x
mcp:
  s:
    transport: streamable_http
    url: https://e.com/mcp
    auth:
      type: oauth2
      client_id: \"${secrets.NOPE}\"
";
        let err = parse(yaml).unwrap_err();
        assert!(matches!(err, BlueprintError::InvalidMcp(_)), "got {err:?}");
    }

    #[test]
    fn legacy_tool_permission_is_rejected_with_filter_guidance() {
        let yaml = "\
name: x
mcp:
  linear:
    transport: streamable_http
    url: https://mcp.linear.app/mcp
permissions:
  main:
    - capability: mcp.linear/listIssues
      action: allow
";
        let err = parse(yaml).unwrap_err().to_string();
        assert!(err.contains("filter"), "{err}");
        assert!(err.contains("mcp.linear"), "{err}");
    }

    #[test]
    fn permission_rule_to_undeclared_server_rejected() {
        let yaml = "\
name: x
permissions:
  main:
    - capability: mcp.ghost/doThing
      action: allow
";
        let err = parse(yaml).unwrap_err();
        match err {
            BlueprintError::InvalidMcp(msg) => {
                assert!(msg.message.contains("ghost"), "got {}", msg.message);
            }
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn non_mcp_capabilities_are_ignored() {
        let yaml = "\
name: x
permissions:
  main:
    - capability: submilli/http.get
      action: allow
    - capability: stripe.com/charge
      action: allow
";
        assert!(parse(yaml).is_ok());
    }

    #[test]
    fn header_server_round_trips() {
        let parsed = parse(HEADERS_BP).unwrap();
        assert_eq!(parse(&to_yaml(&parsed)).unwrap(), parsed);
    }

    #[test]
    fn oauth_server_round_trips() {
        let parsed = parse(OAUTH_BP).unwrap();
        assert_eq!(parse(&to_yaml(&parsed)).unwrap(), parsed);
    }

    #[test]
    fn capability_server_extraction() {
        assert_eq!(
            mcp_capability_server("mcp.linear/listIssues"),
            Some("linear")
        );
        assert_eq!(mcp_capability_server("mcp.linear"), Some("linear"));
        assert_eq!(mcp_capability_server("submilli/http.get"), None);
        assert_eq!(mcp_capability_server("stripe.com/charge"), None);
    }
}
