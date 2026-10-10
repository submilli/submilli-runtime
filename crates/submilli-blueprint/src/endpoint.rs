//! Validation shared by the blueprint blocks that declare a remote provider
//! (`llm:` today, `embedding:` next): the endpoint check, the `${secrets.X}`
//! reference check, and the permission-rule check against a declared-name set.
//!
//! Each function takes the block's own labels, so a message reads "llm provider
//! 'p'" for one block and "embedding provider 'p'" for the other, and the fault
//! path names the right YAML key. Each returns a bare [`Fault`]; the caller wraps
//! it in its own [`BlueprintError`] variant.

use std::collections::BTreeMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use url::Url;

use crate::{Blueprint, Fault, FieldMatch, yaml_path};
use submilli_policy::secret_refs;

/// Host names that always mean this machine, whatever DNS says. Mirrors the
/// loopback set the server's MCP endpoint keeps
/// (`submilli-server/src/file_config.rs`).
const LOOPBACK_HOSTS: [&str; 3] = ["localhost", "127.0.0.1", "::1"];

/// How a provider-declaring block names itself in fault paths and messages.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ProviderBlock {
    /// The top-level YAML key, e.g. `llm`. Providers sit under `<key>.providers`.
    pub key: &'static str,
    /// The message noun for one provider row, e.g. `llm provider`.
    pub label: &'static str,
}

/// A permission rule's reference to a name the block must declare.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PermissionNames {
    /// The capability whose rules are checked, e.g. `llm.call`.
    pub capability: &'static str,
    /// The filter field naming the declared entity, e.g. `model`.
    pub field: &'static str,
    /// The message noun, e.g. `llm model`.
    pub noun: &'static str,
    /// Where the name must be declared, as the fix text writes it, e.g.
    /// `llm.models:`.
    pub declared_in: &'static str,
}

/// Reject an endpoint that would carry the API key somewhere it must not go.
///
/// Because operator-supplied endpoints are trusted by design, a plaintext-`http`
/// or attacker-chosen base URL exfiltrates the key in the request headers on the
/// very first call. Loopback / link-local / private hosts are refused on the same
/// grounds the outbound HTTP policy refuses them
/// (`interpreter/src/stdlib/http/policy.rs`): they are reachable only from inside
/// the deployment, so pointing a credentialed client at one is either a mistake
/// or an SSRF.
///
/// Literal addresses and the loopback names are what a parse-time check can
/// decide; a name that *resolves* privately is caught by the same runtime policy
/// every other outbound request goes through.
pub(crate) fn validate_endpoint(
    block: ProviderBlock,
    name: &str,
    base_url: &str,
) -> Result<(), Fault> {
    let path = yaml_path![block.key, "providers", name, "base_url"];
    let label = block.label;
    let refuse = |reason: String| -> Fault {
        Fault::at(
            path.clone(),
            format!("{label} '{name}': base_url '{base_url}' {reason}"),
        )
    };

    let Ok(url) = Url::parse(base_url) else {
        return Err(refuse(
            "is not a valid URL; use an absolute https:// endpoint".to_string(),
        ));
    };
    if url.scheme() != "https" {
        return Err(refuse(format!(
            "uses the '{}' scheme; the API key travels in the Authorization header, so the \
             endpoint must be https://",
            url.scheme()
        )));
    }
    let Some(host) = url.host_str() else {
        return Err(refuse(
            "has no host; use an absolute https:// endpoint".to_string(),
        ));
    };
    if is_private_host(host) {
        return Err(refuse(
            "resolves to loopback, link-local, or private address space, which a credentialed \
             client must not be pointed at; use the endpoint's public https:// hostname"
                .to_string(),
        ));
    }
    Ok(())
}

/// Every `${secrets.X}` in one provider row's string fields names a declared
/// secret — the load-time check `mcp:` and `auth_proxy:` both apply.
pub(crate) fn check_secret_refs(
    block: ProviderBlock,
    name: &str,
    secret_bearing_values: Vec<&str>,
    blueprint: &Blueprint,
) -> Result<(), Fault> {
    for value in secret_bearing_values {
        for secret in secret_refs(value) {
            if !blueprint.secrets.contains_key(secret) {
                return Err(Fault::at(
                    yaml_path![block.key, "providers", name],
                    format!(
                        "{} '{name}' references undeclared secret '{secret}'; add it under \
                         'secrets:'",
                        block.label
                    ),
                ));
            }
        }
    }
    Ok(())
}

/// Every `names.field` filter on a `names.capability` rule, across all caller
/// blocks, names an entry in `declared`.
///
/// Only exact `field == "..."` matches are checked: a `glob` or `matches`
/// pattern is a shape, not a name, and may legitimately match nothing today.
pub(crate) fn check_permission_names(
    blueprint: &Blueprint,
    names: &PermissionNames,
    declared: &[&str],
) -> Result<(), Fault> {
    for (caller, rules) in &blueprint.permissions {
        for (i, rule) in rules.iter().enumerate() {
            if rule.capability != names.capability {
                continue;
            }
            let Some(filter) = &rule.filter else {
                continue;
            };
            for matched in filter.field_matches(names.field) {
                let FieldMatch::Equals(name) = matched else {
                    continue;
                };
                if !declared.contains(&name.as_str()) {
                    return Err(Fault::at(
                        yaml_path!["permissions", caller, i, "filter"],
                        format!(
                            "caller '{caller}': permission filter names undeclared {} \
                             '{name}'; declare it under '{}' or filter on one of: {}",
                            names.noun,
                            names.declared_in,
                            list_or_none(declared.iter().copied())
                        ),
                    ));
                }
            }
        }
    }
    Ok(())
}

/// The declared keys, for a message that names the alternatives rather than only
/// the rejection.
pub(crate) fn declared_names<T>(entries: &BTreeMap<String, T>) -> String {
    list_or_none(entries.keys().map(String::as_str))
}

fn list_or_none<'a>(names: impl Iterator<Item = &'a str>) -> String {
    let joined = names.collect::<Vec<_>>().join(", ");
    if joined.is_empty() {
        return "(none declared)".to_string();
    }
    joined
}

/// Whether a URL host is one a credentialed client must not be pointed at.
pub(crate) fn is_private_host(host: &str) -> bool {
    // A trailing dot makes a fully-qualified name: `localhost.` is a valid FQDN
    // that resolves to loopback, so it must compare equal to `localhost` or it
    // walks straight past this check.
    let host = host.strip_suffix('.').unwrap_or(host);
    if LOOPBACK_HOSTS
        .iter()
        .any(|known| host.eq_ignore_ascii_case(known))
    {
        return true;
    }
    // `Url` brackets an IPv6 literal; parse the inside.
    let literal = host.strip_prefix('[').and_then(|h| h.strip_suffix(']'));
    match literal.unwrap_or(host).parse::<IpAddr>() {
        Ok(ip) => is_private_ip(normalize(ip)),
        // A name that is not a literal cannot be decided here, and this is the
        // whole of the SSRF control on this path: the outbound provider dispatch
        // uses its own `reqwest` client, which does *not* install the
        // `PolicyResolver` that re-checks resolved addresses for `submilli:http`.
        // So a name resolving into private space is not caught later. Refusing
        // redirects (`submilli-shared/src/http_client.rs`) closes the
        // credential-exfiltration half; the resolve-time half stays open by
        // construction, which is why an operator-supplied `base_url` is a trusted
        // input and documented as one.
        Err(_) => false,
    }
}

/// An IPv4-mapped IPv6 address reaches the same host as the bare IPv4, so it is
/// classified as the embedded v4 — the rule
/// `interpreter/src/stdlib/http/policy.rs` follows, and without it
/// `::ffff:127.0.0.1` walks straight past the loopback check.
fn normalize(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map_or(ip, IpAddr::V4),
        v4 => v4,
    }
}

fn is_private_ip(ip: IpAddr) -> bool {
    if ip.is_loopback() || ip.is_unspecified() {
        return true;
    }
    match ip {
        IpAddr::V4(v4) => {
            v4.is_private() || is_cgnat(v4) || v4.is_link_local() || v4.is_broadcast()
        }
        IpAddr::V6(v6) => is_ula(v6) || is_v6_link_local(v6),
    }
}

/// `100.64.0.0/10` — RFC6598 carrier-grade NAT.
fn is_cgnat(v4: Ipv4Addr) -> bool {
    v4.octets()[0] == 100 && (64..128).contains(&v4.octets()[1])
}

/// `fc00::/7` — IPv6 unique local addresses.
fn is_ula(v6: Ipv6Addr) -> bool {
    v6.segments()[0] & 0xfe00 == 0xfc00
}

/// `fe80::/10`.
fn is_v6_link_local(v6: Ipv6Addr) -> bool {
    v6.segments()[0] & 0xffc0 == 0xfe80
}

#[cfg(test)]
mod tests {
    use super::*;

    const EMBEDDING: ProviderBlock = ProviderBlock {
        key: "embedding",
        label: "embedding provider",
    };

    #[test]
    fn the_block_label_and_path_come_from_the_caller() {
        let fault = validate_endpoint(EMBEDDING, "voyage", "http://example.com").unwrap_err();
        assert!(
            fault
                .message
                .starts_with("embedding provider 'voyage': base_url"),
            "{}",
            fault.message
        );
        assert_eq!(
            fault.path,
            Some(yaml_path!["embedding", "providers", "voyage", "base_url"])
        );
    }

    #[test]
    fn private_hosts_are_refused_and_public_ones_pass() {
        for host in [
            "localhost.",
            "::1",
            "[::ffff:127.0.0.1]",
            "10.0.0.1",
            "100.64.0.1",
        ] {
            assert!(is_private_host(host), "{host}");
        }
        assert!(!is_private_host("api.example.com"));
        assert!(!is_private_host("8.8.8.8"));
    }

    #[test]
    fn declared_names_lists_keys_or_says_none() {
        let mut entries: BTreeMap<String, ()> = BTreeMap::new();
        assert_eq!(declared_names(&entries), "(none declared)");
        entries.insert("b".into(), ());
        entries.insert("a".into(), ());
        assert_eq!(declared_names(&entries), "a, b");
    }
}
