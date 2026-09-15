//! Network egress policy for outbound HTTP — SSRF protection.
//!
//! A [`NetworkPolicy`] decides whether a resolved IP may be connected to.
//! [`PolicyResolver`] enforces it as reqwest's custom DNS resolver: it resolves
//! the host and drops policy-forbidden addresses, so the connection only ever
//! targets vetted IPs. Filtering at resolution rather than on the host string is
//! what closes the DNS-rebinding window — a public name that resolves to an
//! internal IP is caught, and reqwest connects to exactly the addresses we
//! approved.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;

use ipnet::IpNet;
use reqwest::dns::{Addrs, Name, Resolve, Resolving};

/// Outbound-address policy. The default (`enforce == false`) permits every
/// address — the interpreter library and `submilli run` stay unrestricted. The
/// `submilli-server` binary opts into [`NetworkPolicy::deny_private`].
#[derive(Clone, Debug, Default)]
pub struct NetworkPolicy {
    /// Master switch. `false` (default) permits everything — no filtering. When
    /// `true`, private/loopback/special IP space is blocked except where an
    /// opt-out below applies.
    enforce: bool,
    allow_localhost: bool,
    allow_private: bool,
    /// Explicit `--allow-ip` entries; an address inside any of these is always
    /// permitted, overriding every other rule.
    allow: Vec<IpNet>,
}

impl NetworkPolicy {
    /// Permit every address. The library/CLI default.
    pub fn allow_all() -> Self {
        Self::default()
    }

    /// Block private, loopback, link-local and other special-purpose IP space.
    /// The secure base the server layers opt-outs onto.
    pub fn deny_private() -> Self {
        Self {
            enforce: true,
            ..Self::default()
        }
    }

    /// Allow IPv4 + IPv6 loopback (`127.0.0.0/8`, `::1`).
    #[must_use]
    pub fn allow_localhost(mut self, yes: bool) -> Self {
        self.allow_localhost = yes;
        self
    }

    /// Allow all RFC1918 / CGNAT / IPv6-ULA private ranges.
    #[must_use]
    pub fn allow_private(mut self, yes: bool) -> Self {
        self.allow_private = yes;
        self
    }

    /// Allow a specific address or CIDR range, overriding every block.
    #[must_use]
    pub fn allow_cidr(mut self, net: IpNet) -> Self {
        self.allow.push(net);
        self
    }

    /// Whether a request may connect to `ip`.
    pub fn permits(&self, ip: IpAddr) -> bool {
        if !self.enforce {
            return true;
        }
        // An IPv4-mapped IPv6 address (`::ffff:10.0.0.1`) reaches the same host
        // as the bare IPv4 — classify it as the embedded v4 or it bypasses the
        // private/loopback checks below.
        let ip = normalize(ip);
        if self.allow.iter().any(|net| net.contains(&ip)) {
            return true;
        }
        if ip.is_loopback() {
            return self.allow_localhost;
        }
        if is_private(ip) {
            return self.allow_private;
        }
        // Unspecified, link-local (incl. the 169.254.169.254 cloud-metadata
        // endpoint) and broadcast are never legitimate sandbox targets; only an
        // explicit allow-list entry (checked above) can reach them.
        !is_special(ip)
    }
}

/// reqwest DNS resolver that drops policy-forbidden addresses post-resolution.
#[derive(Debug)]
pub struct PolicyResolver {
    policy: Arc<NetworkPolicy>,
}

impl PolicyResolver {
    pub fn new(policy: Arc<NetworkPolicy>) -> Self {
        Self { policy }
    }
}

impl Resolve for PolicyResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let policy = self.policy.clone();
        Box::pin(async move {
            let host = name.as_str().to_string();
            // Port 0 — reqwest overrides it with the request's port. Resolution
            // happens here once; reqwest connects to exactly what we return.
            let resolved = tokio::net::lookup_host((host.as_str(), 0))
                .await
                .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { Box::new(e) })?;
            let permitted: Vec<SocketAddr> =
                resolved.filter(|addr| policy.permits(addr.ip())).collect();
            if permitted.is_empty() {
                return Err(blocked_error(&host));
            }
            Ok(Box::new(permitted.into_iter()) as Addrs)
        })
    }
}

fn blocked_error(host: &str) -> Box<dyn std::error::Error + Send + Sync> {
    Box::new(std::io::Error::other(format!(
        "blocked by network policy: {host} resolves only to private/loopback IP space; \
         allow-list it on the server with --allow-ip / --allow-localhost / --allow-private",
    )))
}

fn normalize(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map_or(ip, IpAddr::V4),
        v4 => v4,
    }
}

/// RFC1918 (v4), CGNAT (`100.64.0.0/10`), and IPv6 ULA (`fc00::/7`).
fn is_private(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_private() || is_cgnat(v4),
        IpAddr::V6(v6) => is_ula(v6),
    }
}

/// Special-purpose space that is never a legitimate outbound target.
fn is_special(ip: IpAddr) -> bool {
    if ip.is_unspecified() {
        return true;
    }
    match ip {
        IpAddr::V4(v4) => v4.is_link_local() || v4.is_broadcast(),
        IpAddr::V6(v6) => is_v6_link_local(v6),
    }
}

/// `100.64.0.0/10` — RFC6598 carrier-grade NAT.
fn is_cgnat(v4: Ipv4Addr) -> bool {
    let [a, b, ..] = v4.octets();
    a == 100 && (64..=127).contains(&b)
}

/// `fc00::/7` — RFC4193 unique local addresses.
fn is_ula(v6: Ipv6Addr) -> bool {
    (v6.octets()[0] & 0xfe) == 0xfc
}

/// `fe80::/10` — link-local unicast (`Ipv6Addr::is_unicast_link_local` is unstable).
fn is_v6_link_local(v6: Ipv6Addr) -> bool {
    let [a, b, ..] = v6.octets();
    a == 0xfe && (b & 0xc0) == 0x80
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    fn cidr(s: &str) -> IpNet {
        s.parse().unwrap()
    }

    #[test]
    fn allow_all_permits_everything() {
        let p = NetworkPolicy::allow_all();
        for s in ["10.0.0.1", "127.0.0.1", "8.8.8.8", "169.254.169.254", "::1"] {
            assert!(p.permits(ip(s)), "{s}");
        }
    }

    #[test]
    fn deny_private_blocks_internal_space() {
        let p = NetworkPolicy::deny_private();
        for s in [
            "10.0.0.1",
            "172.16.5.9",
            "192.168.1.1",
            "100.64.0.1",      // CGNAT
            "127.0.0.1",       // loopback
            "::1",             // loopback v6
            "169.254.169.254", // link-local (cloud metadata)
            "fe80::1",         // link-local v6
            "fc00::1",         // ULA
            "0.0.0.0",         // unspecified
            "255.255.255.255", // broadcast
        ] {
            assert!(!p.permits(ip(s)), "should block {s}");
        }
    }

    #[test]
    fn deny_private_permits_public() {
        let p = NetworkPolicy::deny_private();
        for s in ["8.8.8.8", "1.1.1.1", "93.184.216.34", "2606:4700::1111"] {
            assert!(p.permits(ip(s)), "should permit {s}");
        }
    }

    #[test]
    fn ipv4_mapped_ipv6_is_classified_as_v4() {
        let p = NetworkPolicy::deny_private();
        assert!(
            !p.permits(ip("::ffff:10.0.0.1")),
            "mapped private must block"
        );
        assert!(
            !p.permits(ip("::ffff:127.0.0.1")),
            "mapped loopback must block"
        );
        assert!(p.permits(ip("::ffff:8.8.8.8")), "mapped public ok");
    }

    #[test]
    fn allow_localhost_toggles_loopback_only() {
        let p = NetworkPolicy::deny_private().allow_localhost(true);
        assert!(p.permits(ip("127.0.0.1")));
        assert!(p.permits(ip("::1")));
        assert!(!p.permits(ip("10.0.0.1")), "private still blocked");
    }

    #[test]
    fn allow_private_toggles_private_only() {
        let p = NetworkPolicy::deny_private().allow_private(true);
        assert!(p.permits(ip("10.0.0.1")));
        assert!(p.permits(ip("fc00::1")));
        assert!(!p.permits(ip("127.0.0.1")), "loopback still blocked");
        assert!(
            !p.permits(ip("169.254.169.254")),
            "link-local still blocked"
        );
    }

    #[test]
    fn allow_cidr_overrides_blocks() {
        let p = NetworkPolicy::deny_private().allow_cidr(cidr("10.1.0.0/16"));
        assert!(p.permits(ip("10.1.2.3")));
        assert!(!p.permits(ip("10.2.0.1")), "outside the allowed range");

        let host = NetworkPolicy::deny_private().allow_cidr(cidr("169.254.169.254/32"));
        assert!(
            host.permits(ip("169.254.169.254")),
            "explicit metadata allow"
        );
    }

    #[test]
    fn blocked_error_names_the_policy_and_host() {
        let msg = blocked_error("internal.example").to_string();
        assert!(msg.contains("blocked by network policy"), "{msg}");
        assert!(msg.contains("internal.example"), "{msg}");
    }

    #[test]
    fn permits_filters_blocked_addrs() {
        // The reqwest resolver keeps only `permits`-approved IPs; verify the
        // decision directly (end-to-end SSRF is covered by the server's tests).
        let p = NetworkPolicy::deny_private();
        assert!(!p.permits("10.0.0.1".parse().unwrap()), "rfc1918 blocked");
        assert!(p.permits("8.8.8.8".parse().unwrap()), "public allowed");
        assert!(!p.permits("127.0.0.1".parse().unwrap()), "loopback blocked");
    }
}
