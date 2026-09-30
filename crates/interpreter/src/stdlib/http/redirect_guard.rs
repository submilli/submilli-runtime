//! The capability context of `submilli:http` requests, and the guard that
//! checks their redirect hops against it.
//!
//! The host fn checks the initial URL with these contexts; the transport
//! consults the guard before sending each redirect hop, so a hop the blueprint
//! denies for the request's caller is never sent.

use std::sync::Arc;

use url::Url;

use super::transport::{RedirectDenied, RedirectGuard, RedirectHop};
use crate::runtime::security::SecurityCheck;
use crate::stdlib::shared::authorize_capability;

/// Host and path of `url` for capability context / metrics. A fully qualified
/// `evil.test.` names the same host as `evil.test`, so the trailing dot is
/// dropped or a `host == "evil.test"` rule would not see it.
pub(super) fn host_and_path(url: &Url) -> (String, String) {
    let host = url.host_str().unwrap_or("").trim_end_matches('.');
    (host.to_string(), url.path().to_string())
}

/// Context of an `http.<verb>` check.
pub(super) fn verb_context(
    host: &str,
    path: &str,
    body_size: u64,
    timeout_ms: u64,
) -> serde_json::Value {
    serde_json::json!({
        "host": host,
        "path": path,
        "body_size": body_size,
        "timeout_ms": timeout_ms,
    })
}

/// The `http.download` fields that do not depend on the URL.
pub(super) struct DownloadTarget {
    pub vfs_path: String,
    pub max_bytes: u64,
    pub overwrite: bool,
    pub decompress: bool,
}

impl DownloadTarget {
    /// Context of an `http.download` check for a URL with this host and path.
    pub(super) fn context(&self, host: &str, url_path: &str) -> serde_json::Value {
        serde_json::json!({
            "host": host,
            "url_path": url_path,
            "vfs_path": self.vfs_path,
            "max_bytes": self.max_bytes,
            "overwrite": self.overwrite,
            "decompress": self.decompress,
        })
    }
}

pub(super) enum GuardedRequest {
    Verb { timeout_ms: u64 },
    Download(DownloadTarget),
}

/// Authorizes each redirect hop for the caller resolved when the request began.
/// Owned by one request, so no authorization outlives it or reaches another caller.
pub(super) struct CapabilityGuard {
    caller: String,
    security_check: Arc<dyn SecurityCheck>,
    request: GuardedRequest,
}

impl std::fmt::Debug for CapabilityGuard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CapabilityGuard")
            .field("caller", &self.caller)
            .finish_non_exhaustive()
    }
}

impl CapabilityGuard {
    pub(super) fn new(
        caller: String,
        security_check: Arc<dyn SecurityCheck>,
        request: GuardedRequest,
    ) -> Self {
        Self {
            caller,
            security_check,
            request,
        }
    }

    /// The capability and context a hop is checked against. A hop whose method a
    /// 301/302/303 rewrote to GET is checked as `http.get` on its host and path
    /// alone: it sends no body, and the original request's size and timeout
    /// describe a different request.
    fn hop_check(&self, hop: &RedirectHop<'_>) -> (String, serde_json::Value) {
        let (host, path) = host_and_path(hop.url);
        match &self.request {
            GuardedRequest::Download(target) => {
                ("http.download".to_string(), target.context(&host, &path))
            }
            GuardedRequest::Verb { .. } if hop.method_rewritten => (
                "http.get".to_string(),
                serde_json::json!({ "host": host, "path": path }),
            ),
            GuardedRequest::Verb { timeout_ms } => (
                format!("http.{}", hop.method.to_ascii_lowercase()),
                verb_context(&host, &path, hop.body_len, *timeout_ms),
            ),
        }
    }
}

impl RedirectGuard for CapabilityGuard {
    fn authorize(&self, hop: &RedirectHop<'_>) -> Result<(), RedirectDenied> {
        let (capability, context) = self.hop_check(hop);
        authorize_capability(
            &self.caller,
            self.security_check.as_ref(),
            &capability,
            &context,
        )
        .map_err(RedirectDenied::from_error)
    }
}
