//! Request-scoped blueprint restrictions, independent of the blueprint parser.

use url::Url;

/// Custom HTTP transports must check the initial destination and every redirect
/// with this policy before sending. A request without a policy keeps the
/// embedder's existing transport behavior.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpTransportPolicy {
    pub allow_insecure_http: bool,
    /// Ordered, first-match-wins host rules, including their cleartext opt-ins.
    pub auth_proxy_hosts: Vec<(String, bool)>,
    /// Set only after auth-proxy injection. Never forward injected credentials
    /// to a different scheme, host, or effective port.
    pub same_origin_redirects: bool,
}

impl HttpTransportPolicy {
    pub fn check_destination(&self, url: &Url) -> Result<(), TransportPolicyError> {
        match url.scheme() {
            "https" => return Ok(()),
            "http" => {}
            _ => return Err(TransportPolicyError::UnsupportedScheme),
        }
        if !self.allow_insecure_http {
            return Err(TransportPolicyError::BlueprintRequiresHttps);
        }
        if let Some((host, false)) = self
            .auth_proxy_hosts
            .iter()
            .find(|(host, _)| Some(host.as_str()) == url.host_str())
        {
            return Err(TransportPolicyError::RuleRequiresHttps(host.clone()));
        }
        Ok(())
    }

    pub fn check_redirect(&self, initial: &Url, next: &Url) -> Result<(), TransportPolicyError> {
        self.check_destination(next)?;
        if self.same_origin_redirects && initial.origin() != next.origin() {
            return Err(TransportPolicyError::CrossOriginRedirect);
        }
        Ok(())
    }
}

/// Contains no request URL, headers, or query values, so credentials cannot
/// leak through a policy diagnostic.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TransportPolicyError {
    BlueprintRequiresHttps,
    RuleRequiresHttps(String),
    CrossOriginRedirect,
    UnsupportedScheme,
}

impl std::fmt::Display for TransportPolicyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BlueprintRequiresHttps => f.write_str(
                "HTTPS required: blueprint allow_insecure_http is false; use HTTPS or explicitly enable allow_insecure_http in the blueprint",
            ),
            Self::RuleRequiresHttps(host) => write!(
                f,
                "HTTPS required: auth_proxy rule for host '{host}' requires its own allow_insecure_http: true",
            ),
            Self::CrossOriginRedirect => f.write_str(
                "auth_proxy credentials require a same-origin redirect (same scheme, host, and effective port)",
            ),
            Self::UnsupportedScheme => f.write_str("HTTP requests require an http or https URL"),
        }
    }
}

impl std::error::Error for TransportPolicyError {}
