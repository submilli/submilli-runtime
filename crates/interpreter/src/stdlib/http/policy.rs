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
    observer: Option<EgressObserver>,
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

#[derive(Clone)]
struct EgressObserver(Arc<dyn Fn(&str) + Send + Sync>);

impl std::fmt::Debug for EgressObserver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("EgressObserver")
    }
}

impl NetworkPolicy {
    /// Observe refused destinations without changing the network policy.
    pub fn with_denial_observer(mut self, observer: Arc<dyn Fn(&str) + Send + Sync>) -> Self {
        self.observer = Some(EgressObserver(observer));
        self
    }

    fn observe_denial(&self, host: &str) {
        if let Some(observer) = &self.observer {
            observer.0(host);
        }
    }

    /// Permit every address. The library/CLI default.
    pub fn allow_all() -> Self {
        Self::default()
    }

    /// A reqwest client builder whose DNS resolution is filtered by this policy:
    /// a name resolving only to forbidden addresses fails to resolve. Every
    /// outbound client the runtime builds — `submilli:http`, model providers,
    /// MCP servers — starts from this, so one policy governs them all. Literal
    /// IP hosts never reach a resolver; refuse those with
    /// [`Self::check_literal_host`] before sending.
    pub fn client_builder(self: &Arc<Self>) -> reqwest::ClientBuilder {
        reqwest::Client::builder().dns_resolver(Arc::new(PolicyResolver::new(Arc::clone(self))))
    }

    /// Refuse a URL whose host is a literal IP the policy forbids. Host names
    /// pass here and are judged at resolution.
    pub fn check_literal_host(&self, url: &str) -> Result<(), String> {
        let ip = match url::Url::parse(url)
            .ok()
            .and_then(|u| u.host().map(|h| h.to_owned()))
        {
            Some(url::Host::Ipv4(v4)) => IpAddr::V4(v4),
            Some(url::Host::Ipv6(v6)) => IpAddr::V6(v6),
            _ => return Ok(()),
        };
        if self.permits(ip) {
            Ok(())
        } else {
            self.observe_denial(&ip.to_string());
            Err(format!(
                "blocked by network policy: {ip} is private/loopback IP space; \
                 allow-list it on the server with --allow-ip / --allow-localhost / --allow-private"
            ))
        }
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

#[derive(Debug)]
pub(super) struct EgressDenied(pub String);

impl std::fmt::Display for EgressDenied {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for EgressDenied {}

impl Resolve for PolicyResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let policy = self.policy.clone();
        Box::pin(async move {
            let permitted = policy.resolve_permitted(name.as_str()).await.map_err(
                |e| -> Box<dyn std::error::Error + Send + Sync> {
                    match e {
                        ResolveFailure::Blocked(message) => Box::new(EgressDenied(message)),
                        ResolveFailure::Lookup(error) => Box::new(error),
                    }
                },
            )?;
            Ok(Box::new(permitted.into_iter()) as Addrs)
        })
    }
}

/// Why a host name yielded no address to connect to.
#[derive(Debug)]
pub enum ResolveFailure {
    /// It resolved, but only to addresses the policy forbids.
    Blocked(String),
    /// DNS itself failed; the policy had nothing to judge.
    Lookup(std::io::Error),
}

impl NetworkPolicy {
    /// Resolve `host` and keep the addresses the policy permits. The resolver
    /// installed on every outbound client runs this; a caller whose transport
    /// hides the resolver's error (rmcp wraps it as text) runs it first via
    /// [`Self::check_url`] so the refusal is reported in its own words.
    pub async fn resolve_permitted(&self, host: &str) -> Result<Vec<SocketAddr>, ResolveFailure> {
        // Port 0 — reqwest overrides it with the request's port. Resolution
        // happens here once; reqwest connects to exactly what we return.
        let resolved = tokio::net::lookup_host((host, 0))
            .await
            .map_err(ResolveFailure::Lookup)?;
        let permitted: Vec<SocketAddr> = resolved.filter(|addr| self.permits(addr.ip())).collect();
        if permitted.is_empty() {
            self.observe_denial(host);
            return Err(ResolveFailure::Blocked(blocked_message(host)));
        }
        Ok(permitted)
    }

    /// Refuse a URL the policy forbids, whether its host is a literal IP or a
    /// name that resolves only to forbidden addresses. A name that fails to
    /// resolve at all passes: that failure belongs to the connection attempt,
    /// which reports it in its own terms. Skips DNS entirely when the policy
    /// permits everything.
    pub async fn check_url(&self, url: &str) -> Result<(), String> {
        self.check_literal_host(url)?;
        if !self.enforce {
            return Ok(());
        }
        let Some(host) = url::Url::parse(url)
            .ok()
            .and_then(|u| u.host_str().map(str::to_owned))
        else {
            return Ok(());
        };
        match self.resolve_permitted(&host).await {
            Ok(_) | Err(ResolveFailure::Lookup(_)) => Ok(()),
            Err(ResolveFailure::Blocked(message)) => Err(message),
        }
    }
}

fn blocked_message(host: &str) -> String {
    format!(
        "blocked by network policy: {host} resolves only to private/loopback IP space; \
         allow-list it on the server with --allow-ip / --allow-localhost / --allow-private",
    )
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
        let msg = blocked_message("internal.example");
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
