//! Embedder-supplied security policy trait.
//!
//! Caller identity is fixed by the host-function binding's closure; Wasm code
//! cannot forge a different `caller` value.

use std::sync::Arc;

use super::decision::{CallSite, DecisionExplanation, DecisionRecorder};

#[non_exhaustive]
pub enum CheckOutcome {
    Allow { rule: Option<usize> },
    Deny { reason: String, rule: Option<usize> },
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
