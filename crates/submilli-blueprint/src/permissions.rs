//! The blueprint `permissions:` block — the per-caller capability rules the
//! semantic-security engine evaluates. Keyed by caller identifier (`main` for
//! the user script, package name for library code), each caller maps to an
//! ordered rule list walked top-to-bottom, first-match-wins. A caller absent
//! from the map — or a caller whose rules all miss — falls through to the
//! top-level `default`.
//!
//! Each rule's optional `filter` is a [`FilterExpr`] from [`crate::filter`],
//! parsed at registration time so a malformed filter fails when the blueprint is
//! added rather than at the first `check()`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::filter::{FilterExpr, VarBindings};
use crate::{BlueprintError, Fault, yaml_path};

/// Fall-through action when no rule matches. Absent in the blueprint, it
/// defaults to `Deny` (deny-by-default): a policy-free blueprint denies every
/// capability. `allow` flips the posture to allow-by-default — every capability
/// is permitted unless a rule denies it — for trusted deployments that prefer a
/// blocklist over an allowlist.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DefaultAction {
    #[default]
    Deny,
    Allow,
    AskHuman,
}

/// The action a matched rule (or the fall-through default) resolves to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Action {
    Allow,
    Deny,
    AskHuman,
}

impl From<DefaultAction> for Action {
    fn from(d: DefaultAction) -> Self {
        match d {
            DefaultAction::Deny => Action::Deny,
            DefaultAction::Allow => Action::Allow,
            DefaultAction::AskHuman => Action::AskHuman,
        }
    }
}

/// One capability rule. The capability name is matched verbatim (no wildcards);
/// flexibility lives in the optional `filter`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PermissionRule {
    pub capability: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter: Option<FilterExpr>,
    pub action: Action,
}

/// Structural checks beyond what serde already enforces (malformed filters are
/// rejected at deserialize time); this catches the empty caller / capability
/// cases serde can't express.
pub(crate) fn validate(
    permissions: &BTreeMap<String, Vec<PermissionRule>>,
) -> Result<(), BlueprintError> {
    for (caller, rules) in permissions {
        if caller.is_empty() {
            return Err(BlueprintError::InvalidPermissions(Fault::at(
                yaml_path!["permissions", caller],
                "caller identifier must not be empty",
            )));
        }
        for (i, rule) in rules.iter().enumerate() {
            if rule.capability.is_empty() {
                return Err(BlueprintError::InvalidPermissions(Fault::at(
                    yaml_path!["permissions", caller, i, "capability"],
                    format!("caller '{caller}': a rule has an empty capability name"),
                )));
            }
        }
    }
    Ok(())
}

/// Walk the rules for `caller` and return the resolved action. No caller block,
/// or no rule matching `(capability, ctx)`, falls through to `default`.
pub(crate) fn resolve(
    permissions: &BTreeMap<String, Vec<PermissionRule>>,
    default_action: DefaultAction,
    caller: &str,
    capability: &str,
    ctx: &serde_json::Value,
    vars: &VarBindings,
) -> Action {
    resolve_with_rule(permissions, default_action, caller, capability, ctx, vars).0
}

pub(crate) fn resolve_with_rule(
    permissions: &BTreeMap<String, Vec<PermissionRule>>,
    default_action: DefaultAction,
    caller: &str,
    capability: &str,
    ctx: &serde_json::Value,
    vars: &VarBindings,
) -> (Action, Option<usize>) {
    if let Some(rules) = permissions.get(caller) {
        for (index, rule) in rules.iter().enumerate() {
            if rule.capability == capability
                && rule
                    .filter
                    .as_ref()
                    .is_none_or(|f| f.matches_with(ctx, vars))
            {
                return (rule.action, Some(index));
            }
        }
    }
    (default_action.into(), None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn filter(s: &str) -> FilterExpr {
        crate::filter::parse(s).expect("valid filter")
    }

    #[test]
    fn resolve_first_match_wins() {
        let mut perms = BTreeMap::new();
        perms.insert(
            "main".to_string(),
            vec![
                PermissionRule {
                    capability: "stripe.com/charge".into(),
                    filter: Some(filter("amount < 500")),
                    action: Action::Allow,
                },
                PermissionRule {
                    capability: "stripe.com/charge".into(),
                    filter: None,
                    action: Action::AskHuman,
                },
            ],
        );
        let allow = resolve(
            &perms,
            DefaultAction::Deny,
            "main",
            "stripe.com/charge",
            &json!({ "amount": 100 }),
            &VarBindings::new(),
        );
        assert_eq!(allow, Action::Allow);
        let ask = resolve(
            &perms,
            DefaultAction::Deny,
            "main",
            "stripe.com/charge",
            &json!({ "amount": 900 }),
            &VarBindings::new(),
        );
        assert_eq!(ask, Action::AskHuman);
    }

    #[test]
    fn resolve_falls_through_to_default() {
        let perms = BTreeMap::new();
        // Caller absent entirely.
        assert_eq!(
            resolve(
                &perms,
                DefaultAction::Deny,
                "main",
                "x",
                &json!({}),
                &VarBindings::new()
            ),
            Action::Deny
        );
        assert_eq!(
            resolve(
                &perms,
                DefaultAction::AskHuman,
                "main",
                "x",
                &json!({}),
                &VarBindings::new()
            ),
            Action::AskHuman
        );
        assert_eq!(
            resolve(
                &perms,
                DefaultAction::Allow,
                "main",
                "x",
                &json!({}),
                &VarBindings::new()
            ),
            Action::Allow
        );
    }

    #[test]
    fn resolve_capability_is_exact() {
        let mut perms = BTreeMap::new();
        perms.insert(
            "main".to_string(),
            vec![PermissionRule {
                capability: "stripe.com/charge".into(),
                filter: None,
                action: Action::Allow,
            }],
        );
        // No wildcard: a sibling capability falls through to default.
        assert_eq!(
            resolve(
                &perms,
                DefaultAction::Deny,
                "main",
                "stripe.com/refund",
                &json!({}),
                &VarBindings::new()
            ),
            Action::Deny
        );
    }
}
