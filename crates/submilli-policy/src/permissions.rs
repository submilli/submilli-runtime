//! Permission rules — the per-caller capability rules the semantic-security
//! engine evaluates, the blueprint's `permissions:` block. Keyed by caller identifier (`main` for
//! the user script, package name for library code), each caller maps to an
//! ordered rule list walked top-to-bottom, first-match-wins. A caller absent
//! from the map — or a caller whose rules all miss — falls through to the
//! top-level `default`.
//!
//! Each rule's optional `filter` is a [`FilterExpr`] from [`crate::filter`],
//! parsed when the rules are read, so a malformed filter fails then rather than
//! at the first `check()`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::filter::{ComparisonFailure, FilterExpr, VarBindings};

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
    /// Label decisions cite this rule by. Unique within a caller block
    /// (`blueprint lint` enforces it); the rule's position identifies it
    /// otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub capability: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter: Option<FilterExpr>,
    pub action: Action,
}

/// Every caller's rules, by caller identifier.
pub type Rules = BTreeMap<String, Vec<PermissionRule>>;

/// A complete permission policy: the per-caller rules and what applies when
/// none matches.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Policy {
    pub rules: Rules,
    pub default: DefaultAction,
}

impl Policy {
    /// See [`resolve`].
    pub fn resolve(
        &self,
        caller: &str,
        capability: &str,
        ctx: &serde_json::Value,
        vars: &VarBindings,
    ) -> Action {
        resolve(&self.rules, self.default, caller, capability, ctx, vars)
    }

    /// See [`resolve_with_rule`].
    pub fn resolve_with_rule(
        &self,
        caller: &str,
        capability: &str,
        ctx: &serde_json::Value,
        vars: &VarBindings,
    ) -> (Action, Option<usize>) {
        resolve_with_rule(&self.rules, self.default, caller, capability, ctx, vars)
    }

    /// See [`explain`].
    pub fn explain(
        &self,
        caller: &str,
        capability: &str,
        ctx: &serde_json::Value,
        vars: &VarBindings,
    ) -> Resolution {
        explain(&self.rules, self.default, caller, capability, ctx, vars)
    }
}

/// Walk the rules for `caller` and return the resolved action. No caller block,
/// or no rule matching `(capability, ctx)`, falls through to `default`.
pub fn resolve(
    permissions: &Rules,
    default_action: DefaultAction,
    caller: &str,
    capability: &str,
    ctx: &serde_json::Value,
    vars: &VarBindings,
) -> Action {
    resolve_with_rule(permissions, default_action, caller, capability, ctx, vars).0
}

/// [`resolve`], with the index of the deciding rule in the caller's block, or
/// `None` when the default decided.
pub fn resolve_with_rule(
    permissions: &Rules,
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

/// A resolved decision with the reasoning behind it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Resolution {
    /// Identical to what [`resolve`] returns for the same inputs.
    pub action: Action,
    pub cause: ResolutionCause,
    /// Rules for the capability, ahead of the deciding rule (or all of them
    /// when the default decided), whose filters rejected the call.
    pub near_misses: Vec<NearMiss>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case", tag = "kind")]
pub enum ResolutionCause {
    Rule(RuleRef),
    /// No rule matched. `caller_block` is whether the caller has any rules.
    Default {
        caller_block: bool,
    },
}

/// A rule located by caller block and zero-based position, with its name when
/// it has one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RuleRef {
    pub caller: String,
    pub index: usize,
    pub name: Option<String>,
}

/// A rule that named the right capability but whose filter rejected the call.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NearMiss {
    pub rule: RuleRef,
    /// The rule's whole filter, from its `Display` impl.
    pub filter: String,
    pub failures: Vec<ComparisonFailure>,
}

/// [`resolve`], with the reasoning: the deciding rule or default, and the rules
/// for the capability whose filters rejected the call.
pub fn explain(
    permissions: &Rules,
    default_action: DefaultAction,
    caller: &str,
    capability: &str,
    ctx: &serde_json::Value,
    vars: &VarBindings,
) -> Resolution {
    let (action, matched) =
        resolve_with_rule(permissions, default_action, caller, capability, ctx, vars);
    let rules = permissions.get(caller);
    let rule_ref = |index: usize, rule: &PermissionRule| RuleRef {
        caller: caller.to_string(),
        index,
        name: rule.name.clone(),
    };
    let cause = match matched.and_then(|index| Some((index, rules?.get(index)?))) {
        Some((index, rule)) => ResolutionCause::Rule(rule_ref(index, rule)),
        None => ResolutionCause::Default {
            caller_block: rules.is_some(),
        },
    };
    let walked = matched.unwrap_or(usize::MAX);
    let near_misses = rules
        .into_iter()
        .flatten()
        .enumerate()
        .take_while(|(index, _)| *index < walked)
        .filter(|(_, rule)| rule.capability == capability)
        .filter_map(|(index, rule)| {
            let filter = rule.filter.as_ref()?;
            let evaluation = filter.explain_with(ctx, vars);
            (!evaluation.matched).then(|| NearMiss {
                rule: rule_ref(index, rule),
                filter: filter.to_string(),
                failures: evaluation.failures,
            })
        })
        .collect();
    Resolution {
        action,
        cause,
        near_misses,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn filter(s: &str) -> FilterExpr {
        crate::filter::parse(s).expect("valid filter")
    }

    /// The shape an embedder that keeps policy outside a blueprint builds: rules
    /// whose filters it parsed from its own format, and a default.
    #[test]
    fn a_policy_built_by_hand_scopes_by_session_variable() {
        let policy = Policy {
            rules: BTreeMap::from([(
                "main".to_string(),
                vec![PermissionRule {
                    name: Some("own tenant".to_string()),
                    capability: "agent.run".to_string(),
                    filter: Some("agent == ${vars.agent}".parse().expect("filter")),
                    action: Action::Allow,
                }],
            )]),
            default: DefaultAction::Deny,
        };
        let vars = VarBindings::from([("agent".to_string(), "researcher".to_string())]);
        let ctx = |agent: &str| json!({ "agent": agent });
        assert_eq!(
            policy.resolve_with_rule("main", "agent.run", &ctx("researcher"), &vars),
            (Action::Allow, Some(0))
        );
        assert_eq!(
            policy.resolve("main", "agent.run", &ctx("deployer"), &vars),
            Action::Deny
        );
        let explained = policy.explain("main", "agent.run", &ctx("deployer"), &vars);
        assert_eq!(explained.near_misses.len(), 1);
    }

    #[test]
    fn resolve_first_match_wins() {
        let mut perms = BTreeMap::new();
        perms.insert(
            "main".to_string(),
            vec![
                PermissionRule {
                    name: None,
                    capability: "stripe.com/charge".into(),
                    filter: Some(filter("amount < 500")),
                    action: Action::Allow,
                },
                PermissionRule {
                    name: None,
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
                name: None,
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

    fn rule(
        name: Option<&str>,
        capability: &str,
        f: Option<&str>,
        action: Action,
    ) -> PermissionRule {
        PermissionRule {
            name: name.map(str::to_string),
            capability: capability.into(),
            filter: f.map(filter),
            action,
        }
    }

    fn vars(pairs: &[(&str, &str)]) -> VarBindings {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn explain_main(
        rules: Vec<PermissionRule>,
        default: DefaultAction,
        capability: &str,
        ctx: serde_json::Value,
        bindings: &VarBindings,
    ) -> Resolution {
        let perms = BTreeMap::from([("main".to_string(), rules)]);
        let resolution = explain(&perms, default, "main", capability, &ctx, bindings);
        // The explanation never disagrees with enforcement.
        assert_eq!(
            resolution.action,
            resolve(&perms, default, "main", capability, &ctx, bindings)
        );
        resolution
    }

    #[test]
    fn explain_filter_miss_is_a_near_miss_under_default() {
        let resolution = explain_main(
            vec![rule(
                Some("own-customer"),
                "acme.com/charges.list",
                Some("customerId == ${vars.customerId}"),
                Action::Allow,
            )],
            DefaultAction::Deny,
            "acme.com/charges.list",
            json!({ "customerId": "cus_initech" }),
            &vars(&[("customerId", "cus_northwind")]),
        );
        assert_eq!(resolution.action, Action::Deny);
        assert_eq!(
            resolution.cause,
            ResolutionCause::Default { caller_block: true }
        );
        let [miss] = resolution.near_misses.as_slice() else {
            panic!("expected one near miss: {resolution:?}");
        };
        assert_eq!(miss.rule.index, 0);
        assert_eq!(miss.rule.name.as_deref(), Some("own-customer"));
        assert_eq!(miss.filter, "customerId == ${vars.customerId}");
        let [failure] = miss.failures.as_slice() else {
            panic!("expected one failure: {miss:?}");
        };
        assert_eq!(failure.comparison, "customerId == ${vars.customerId}");
        assert_eq!(failure.actual, Some(json!("cus_initech")));
        assert_eq!(failure.expected.as_deref(), Some("cus_northwind"));
    }

    #[test]
    fn explain_second_rule_match_cites_index_and_name() {
        let resolution = explain_main(
            vec![
                rule(
                    None,
                    "stripe.com/charge",
                    Some("amount < 500"),
                    Action::Allow,
                ),
                rule(Some("big-charges"), "stripe.com/charge", None, Action::Deny),
            ],
            DefaultAction::Allow,
            "stripe.com/charge",
            json!({ "amount": 900 }),
            &VarBindings::new(),
        );
        assert_eq!(resolution.action, Action::Deny);
        assert_eq!(
            resolution.cause,
            ResolutionCause::Rule(RuleRef {
                caller: "main".into(),
                index: 1,
                name: Some("big-charges".into()),
            })
        );
        // The earlier rule that rejected the call is still reported.
        assert_eq!(resolution.near_misses.len(), 1);
        assert_eq!(resolution.near_misses[0].rule.index, 0);
        assert_eq!(resolution.near_misses[0].rule.name, None);
    }

    #[test]
    fn explain_without_caller_block_reports_it() {
        let perms = BTreeMap::new();
        let resolution = explain(
            &perms,
            DefaultAction::Deny,
            "main",
            "x",
            &json!({}),
            &VarBindings::new(),
        );
        assert_eq!(resolution.action, Action::Deny);
        assert_eq!(
            resolution.cause,
            ResolutionCause::Default {
                caller_block: false
            }
        );
        assert!(resolution.near_misses.is_empty());
    }

    #[test]
    fn explain_ask_human_stays_distinct_and_names_the_rule() {
        let resolution = explain_main(
            vec![rule(
                Some("review-refunds"),
                "stripe.com/refund",
                None,
                Action::AskHuman,
            )],
            DefaultAction::Deny,
            "stripe.com/refund",
            json!({}),
            &VarBindings::new(),
        );
        assert_eq!(resolution.action, Action::AskHuman);
        let ResolutionCause::Rule(rule) = &resolution.cause else {
            panic!("expected a rule cause: {resolution:?}");
        };
        assert_eq!(rule.name.as_deref(), Some("review-refunds"));
    }

    #[test]
    fn explain_unbound_variable_failure() {
        let resolution = explain_main(
            vec![rule(None, "x", Some("id == ${vars.x}"), Action::Allow)],
            DefaultAction::Deny,
            "x",
            json!({ "id": "a" }),
            &VarBindings::new(),
        );
        assert_eq!(
            resolution.near_misses[0].failures[0].reason,
            crate::filter::FailureReason::VariableNotBound("x".into())
        );
    }

    #[test]
    fn explain_ignores_other_capabilities() {
        let resolution = explain_main(
            vec![rule(None, "a", Some("n == 1"), Action::Allow)],
            DefaultAction::Deny,
            "b",
            json!({ "n": 2 }),
            &VarBindings::new(),
        );
        assert!(resolution.near_misses.is_empty());
    }
}
