//! Embedder-supplied security policy trait.
//!
//! Caller identity is fixed by the host-function binding's closure; Wasm code
//! cannot forge a different `caller` value.

use std::sync::Arc;

use super::approvals::{Approver, AskRule};
use super::decision::{CallSite, DecisionExplanation, DecisionRecorder};
use crate::stdlib::capabilities;

#[non_exhaustive]
pub enum CheckOutcome {
    Allow {
        rule: Option<usize>,
    },
    Deny {
        reason: String,
        rule: Option<usize>,
    },
    /// The call may run once `approver` approves it. A call that cannot be put to
    /// them is denied, with the [`denial_reason`] a deny gives.
    Ask {
        approver: Approver,
        rule: AskRule,
    },
}

/// A decision observation; recording must never change authorization semantics.
///
/// `#[non_exhaustive]` so it can grow: build one with [`AuditDecision::new`] and the
/// `with_*` methods. Sinks that predate a field ignore it.
#[derive(Clone, Copy)]
#[non_exhaustive]
pub struct AuditDecision<'a> {
    pub caller: &'a str,
    pub capability: &'a str,
    pub context: &'a serde_json::Value,
    pub allowed: bool,
    pub source: &'a str,
    pub rule: Option<usize>,
    pub reason: Option<&'a str>,
    /// Why the policy decided as it did. Present only while a recorder is installed.
    pub explanation: Option<&'a DecisionExplanation>,
    /// How the decision was reached, and the call it belongs to.
    pub site: CallSite,
}

impl<'a> AuditDecision<'a> {
    pub fn new(
        caller: &'a str,
        capability: &'a str,
        context: &'a serde_json::Value,
        allowed: bool,
        source: &'a str,
        rule: Option<usize>,
        reason: Option<&'a str>,
    ) -> Self {
        Self {
            caller,
            capability,
            context,
            allowed,
            source,
            rule,
            reason,
            explanation: None,
            site: CallSite::default(),
        }
    }

    pub fn with_explanation(mut self, explanation: Option<&'a DecisionExplanation>) -> Self {
        self.explanation = explanation;
        self
    }

    pub fn with_site(mut self, site: CallSite) -> Self {
        self.site = site;
        self
    }
}

pub trait SecurityCheck: Send + Sync {
    /// Optional embedder audit sink. No guest fuel is charged for observation.
    fn audit(&self, _decision: AuditDecision<'_>) {}
    /// The metadata actually tested by the policy, including lexical VFS normalization.
    fn audit_context<'a>(
        &self,
        _capability: &str,
        context: &'a serde_json::Value,
        _cwd: &str,
    ) -> std::borrow::Cow<'a, serde_json::Value> {
        std::borrow::Cow::Borrowed(context)
    }
    fn check_with_cwd(
        &self,
        caller: &str,
        capability: &str,
        context: &serde_json::Value,
        _cwd: &str,
    ) -> CheckOutcome {
        self.check(caller, capability, context)
    }
    fn check(&self, caller: &str, capability: &str, context: &serde_json::Value) -> CheckOutcome;
    /// The per-run decision recorder, when one is installed. The runtime computes
    /// explanations and captures source lines only while this is `Some`.
    fn recorder(&self) -> Option<&dyn DecisionRecorder> {
        None
    }
    /// Why the policy would decide as it does for this call, for a recorder.
    /// Called only while [`Self::recorder`] is `Some`, with the context as the host
    /// function built it; the policy applies its own normalization.
    fn explain(
        &self,
        _caller: &str,
        _capability: &str,
        _context: &serde_json::Value,
        _cwd: &str,
    ) -> Option<DecisionExplanation> {
        None
    }
}

/// Default policy: allows all calls, logging each to stderr so stdout carries
/// only the program's result.
pub struct AllowAllCheck;

impl SecurityCheck for AllowAllCheck {
    fn check(&self, caller: &str, capability: &str, context: &serde_json::Value) -> CheckOutcome {
        use std::io::Write;
        let _ = writeln!(
            std::io::stderr().lock(),
            "[security] caller={caller} capability={capability} context={context}"
        );
        CheckOutcome::Allow { rule: None }
    }
}

pub fn default_check() -> Arc<dyn SecurityCheck> {
    Arc::new(AllowAllCheck)
}

/// The reason a policy denial gives `caller` for `capability`. `context` is the one
/// the policy tested ([`SecurityCheck::audit_context`]), so a filesystem path is named
/// as normalized.
pub fn denial_reason(caller: &str, capability: &str, context: &serde_json::Value) -> String {
    format!(
        "policy denied {capability}{} for {caller}",
        denial_target(capability, context)
    )
}

/// What a denial was for, named in its reason: the VFS paths of a filesystem capability,
/// or the values of an application-defined one. Empty for other capabilities.
fn denial_target(capability: &str, context: &serde_json::Value) -> String {
    let paths = filesystem_target(capability, context);
    if paths.is_empty() && is_application_defined(capability) {
        request_values(context)
    } else {
        paths
    }
}

/// The most values a denial names, and the most of each string value it shows.
const MAX_NAMED_VALUES: usize = 4;
const MAX_NAMED_VALUE_CHARS: usize = 64;

/// The scalar values of an application-defined capability's request, so a model can tell
/// the request refused (`customerId = "cus_globex"`) from the capability being refused,
/// and so a narrowed filter can be fixed from the message. Every one was put there by the
/// program or by a package it called. A built-in capability's values are left out: a
/// runtime library can fill them with what the program never saw, such as a redirect's
/// URL.
fn request_values(context: &serde_json::Value) -> String {
    let Some(fields) = context.as_object() else {
        return String::new();
    };
    let named: Vec<String> = fields
        .iter()
        .filter_map(|(name, value)| {
            let shown = match value {
                serde_json::Value::String(text) => {
                    let mut chars = text.chars();
                    let mut cut: String = chars.by_ref().take(MAX_NAMED_VALUE_CHARS).collect();
                    if chars.next().is_some() {
                        cut.push('…');
                    }
                    serde_json::Value::String(cut).to_string()
                }
                serde_json::Value::Number(_) | serde_json::Value::Bool(_) => value.to_string(),
                _ => return None,
            };
            Some(format!("{name} = {shown}"))
        })
        .collect();
    if named.is_empty() {
        return String::new();
    }
    let more = if named.len() > MAX_NAMED_VALUES {
        ", …"
    } else {
        ""
    };
    format!(
        " with {}{more}",
        named[..named.len().min(MAX_NAMED_VALUES)].join(", ")
    )
}

/// Whether `capability` is one an application's package declares: not one the runtime
/// gates itself, nor an MCP server's tool.
fn is_application_defined(capability: &str) -> bool {
    !capability.starts_with("mcp.") && capabilities::find_any(capability).is_none()
}

/// The normalized VFS paths a denial was for, so a narrowed filter can be fixed
/// from the message. The library may have probed a path on the program's
/// behalf, as `submilli:code` does for ignore files, so the program's own
/// arguments do not always name it. Empty for capabilities without VFS paths.
fn filesystem_target(capability: &str, context: &serde_json::Value) -> String {
    let path = |field: &&str| context.get(*field).and_then(serde_json::Value::as_str);
    match capabilities::vfs_path_fields(capability) {
        [single] => path(single).map_or_else(String::new, |path| format!(" on {path}")),
        [from, to] => match (path(from), path(to)) {
            (Some(from), Some(to)) => format!(" from {from} to {to}"),
            _ => String::new(),
        },
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allow_all_returns_allow() {
        let check = AllowAllCheck;
        let outcome = check.check("main", "test.com/op", &serde_json::json!({"foo": 1}));
        assert!(matches!(outcome, CheckOutcome::Allow { .. }));
    }

    #[test]
    fn default_check_is_allow_all() {
        let check = default_check();
        let outcome = check.check("main", "x", &serde_json::Value::Null);
        assert!(matches!(outcome, CheckOutcome::Allow { .. }));
    }
}
