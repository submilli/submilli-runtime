//! Embedder-supplied security policy trait.
//!
//! Caller identity is fixed by the host-function binding's closure; Wasm code
//! cannot forge a different `caller` value.

use std::sync::Arc;

#[non_exhaustive]
pub enum CheckOutcome {
    Allow,
    Deny { reason: String },
}

pub trait SecurityCheck: Send + Sync {
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
}

/// Default policy: allows all calls, logging each to stderr so stdout carries
/// only the program's result.
pub struct AllowAllCheck;

impl SecurityCheck for AllowAllCheck {
    fn check(&self, caller: &str, capability: &str, context: &serde_json::Value) -> CheckOutcome {
        eprintln!("[security] caller={caller} capability={capability} context={context}");
        CheckOutcome::Allow
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
        assert!(matches!(outcome, CheckOutcome::Allow));
    }

    #[test]
    fn default_check_is_allow_all() {
        let check = default_check();
        let outcome = check.check("main", "x", &serde_json::Value::Null);
        assert!(matches!(outcome, CheckOutcome::Allow));
    }
}
