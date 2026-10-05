//! The capability context of `submilli:http` requests, and the guard that
//! checks their redirect hops against it.
//!
//! The host fn checks the initial URL with these contexts; the transport
//! consults the guard before sending each redirect hop, so a hop the blueprint
//! denies for the request's caller is never sent.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use url::Url;

use super::transport::{EgressAt, RedirectDenied, RedirectGuard, RedirectHop};
use crate::runtime::decision::{CallSite, CallTicket, EntryPath};
use crate::runtime::security::SecurityCheck;
use crate::stdlib::shared::{audit_denial_at, authorize_capability};
use crate::stdlib::url::host_without_trailing_dots;

/// Host and path of `url` for capability context / metrics.
pub(super) fn host_and_path(url: &Url) -> (String, String) {
    let host = host_without_trailing_dots(url);
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
    cwd: String,
    /// The call that started the request: a hop's record names it and reuses its line,
    /// since the program's stack is gone by the time a hop is authorized.
    parent: Option<CallTicket>,
    hops: AtomicU32,
    /// The call a refusal of the request's current hop continues: the originating call until
    /// a redirect hop is authorized, then that hop's.
    ///
    /// Poison recovery is acceptable: the value is `Copy` and always overwritten whole, so a
    /// panicking holder cannot leave a partly updated site behind.
    current: Mutex<CallSite>,
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
        cwd: String,
        parent: Option<CallTicket>,
    ) -> Self {
        Self {
            caller,
            security_check,
            request,
            cwd,
            parent,
            hops: AtomicU32::new(0),
            current: Mutex::new(CallSite::new(parent, EntryPath::GatedOp)),
        }
    }

    /// The call a refusal of `at` belongs to.
    fn egress_site(&self, at: EgressAt, capability: &str) -> CallSite {
        match at {
            EgressAt::CurrentHop => *self
                .current
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            EgressAt::NewHop => self.hop_site(capability),
        }
    }

    /// Begins the call for the next redirect hop, when a recorder is installed.
    fn hop_site(&self, capability: &str) -> CallSite {
        let Some(recorder) = self.security_check.recorder() else {
            return CallSite::default();
        };
        let line = self.parent.and_then(|parent| parent.line);
        let ticket = recorder.begin_call(&self.caller, capability, line);
        let index = self.hops.fetch_add(1, Ordering::Relaxed);
        CallSite::new(
            Some(ticket),
            EntryPath::RedirectHop {
                parent_call_index: self.parent.map_or(0, |parent| parent.call_index),
                index,
            },
        )
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
    fn audit_egress_denial(&self, hop: &RedirectHop<'_>, at: EgressAt) {
        let (capability, context) = self.hop_check(hop);
        audit_denial_at(
            self.security_check.as_ref(),
            &self.caller,
            &capability,
            &context,
            "egress_guard",
            "outbound destination refused",
            self.egress_site(at, &capability),
        );
    }

    fn authorize(&self, hop: &RedirectHop<'_>) -> Result<(), RedirectDenied> {
        let (capability, context) = self.hop_check(hop);
        let site = self.hop_site(&capability);
        *self
            .current
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = site;
        authorize_capability(
            &self.caller,
            self.security_check.as_ref(),
            &capability,
            &context,
            &self.cwd,
            site,
        )
        .map_err(RedirectDenied::from_error)
    }
}
