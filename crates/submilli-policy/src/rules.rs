//! Permission rules: the per-caller capability rules a check evaluates. Keyed
//! by caller identifier (`main` for the user script, package name for library
//! code), each caller maps to an ordered rule list walked top-to-bottom,
//! first-match-wins. A caller absent from the map, or a caller whose rules all
//! miss, falls through to the default action.
//!
//! Each rule's optional `filter` is a [`FilterExpr`], parsed when the rule is
//! deserialized so a malformed filter fails when the rules are loaded rather
//! than at the first check.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::filter::{ComparisonFailure, FilterExpr, Truth, VarBindings};

/// Rules per caller identifier, each list in evaluation order.
pub type Permissions = BTreeMap<String, Vec<PermissionRule>>;

/// Fall-through action when no rule matches. It defaults to `Deny`
/// (deny-by-default): a rule set without rules denies every capability.
/// `allow` flips the posture to allow-by-default — every capability is
/// permitted unless a rule denies it — for trusted deployments that prefer a
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
    /// Needs a person's approval. `PermissionCheck` (the `engine` feature)
    /// treats it as a denial until calls can suspend for approval.
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
    /// The capability the rule decides, such as `http.get`.
    pub capability: String,
    /// The condition on the call's context; a rule without one matches every
    /// call to its capability.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter: Option<FilterExpr>,
    pub action: Action,
}

/// Why a rule set is malformed, beyond what deserializing it already rejects
/// (a malformed filter, an unknown field). Says which caller and rule, as data,
/// so a caller can point at the rule in its own source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleError {
    /// A caller block's identifier is empty.
    EmptyCaller { caller: String },
    /// The rule at zero-based `index` in `caller`'s block names no capability.
    EmptyCapability { caller: String, index: usize },
    /// The rule at zero-based `index` in `caller`'s block has an empty name.
    EmptyName { caller: String, index: usize },
}

impl fmt::Display for RuleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RuleError::EmptyCaller { .. } => f.write_str("caller identifier must not be empty"),
            RuleError::EmptyCapability { caller, .. } => {
                write!(f, "caller '{caller}': a rule has an empty capability name")
            }
            RuleError::EmptyName { caller, .. } => {
                write!(f, "caller '{caller}': a rule has an empty name")
            }
        }
    }
}

impl std::error::Error for RuleError {}

/// Check what deserializing `permissions` can't: no empty caller identifier,
/// capability name or rule name. Reports the first problem found.
pub fn validate_rules(permissions: &Permissions) -> Result<(), RuleError> {
    for (caller, rules) in permissions {
        if caller.is_empty() {
            return Err(RuleError::EmptyCaller {
                caller: caller.clone(),
            });
        }
        for (index, rule) in rules.iter().enumerate() {
            if rule.capability.is_empty() {
                return Err(RuleError::EmptyCapability {
                    caller: caller.clone(),
                    index,
                });
            }
            if rule.name.as_deref() == Some("") {
                return Err(RuleError::EmptyName {
                    caller: caller.clone(),
                    index,
                });
            }
        }
    }
    Ok(())
}

/// The outcome of resolving one capability call against a rule set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Decision {
    pub action: Action,
    /// Zero-based index of the deciding rule within the caller's block;
    /// `None` when the default decided.
    pub rule: Option<usize>,
}

/// A source of permission rules and the default they fall through to, and the
/// evaluation over them. Implemented by [`RuleSet`]; a type that already holds
/// its rules (such as a blueprint) implements the two accessors to be checked
/// without copying them.
pub trait Rules {
    /// Rules per caller identifier.
    fn permissions(&self) -> &Permissions;

    /// The action when no rule matches.
    fn default_action(&self) -> DefaultAction;

    /// Resolve a capability call by `caller` with `context`, the JSON object
    /// the capability's check reports. `vars` binds `${vars.NAME}` operands.
    /// No parsing happens here, so it is cheap to call per check.
    fn resolve(
        &self,
        caller: &str,
        capability: &str,
        context: &serde_json::Value,
        vars: &VarBindings,
    ) -> Decision {
        resolve_with_rule(
            self.permissions(),
            self.default_action(),
            caller,
            capability,
            context,
            vars,
        )
    }

    /// [`Self::resolve`] plus the reasoning: the deciding rule or default, and
    /// the rules that named the capability but whose filters rejected the
    /// call. `action` always equals what [`Self::resolve`] returns.
    fn explain(
        &self,
        caller: &str,
        capability: &str,
        context: &serde_json::Value,
        vars: &VarBindings,
    ) -> Resolution {
        explain_resolution(
            self.permissions(),
            self.default_action(),
            caller,
            capability,
            context,
            vars,
        )
    }

    /// What a call by `caller` to `capability` could resolve to, before its
    /// context is fully known: documentation decides from this whether to
    /// show an operation. `known` holds the context fields known ahead of the
    /// call.
    fn possible_actions(
        &self,
        caller: &str,
        capability: &str,
        known: &serde_json::Value,
    ) -> PossibleActions {
        possible_actions(
            self.permissions(),
            self.default_action(),
            caller,
            capability,
            known,
        )
    }
}

/// An owned, validated set of permission rules with its default action.
#[derive(Debug, Clone, PartialEq)]
pub struct RuleSet {
    permissions: Permissions,
    default_action: DefaultAction,
}

impl RuleSet {
    /// A rule set from rules per caller and the default action, validated
    /// with [`validate_rules`].
    pub fn new(permissions: Permissions, default_action: DefaultAction) -> Result<Self, RuleError> {
        validate_rules(&permissions)?;
        Ok(RuleSet {
            permissions,
            default_action,
        })
    }
}

impl Rules for RuleSet {
    fn permissions(&self) -> &Permissions {
        &self.permissions
    }

    fn default_action(&self) -> DefaultAction {
        self.default_action
    }
}

/// Walk the rules for `caller` and return the [`Decision`]: the resolved
/// action and the deciding rule's index. No caller block, or no rule matching
/// `(capability, ctx)`, falls through to `default_action` with no index.
fn resolve_with_rule(
    permissions: &Permissions,
    default_action: DefaultAction,
    caller: &str,
    capability: &str,
    ctx: &serde_json::Value,
    vars: &VarBindings,
) -> Decision {
    if let Some(rules) = permissions.get(caller) {
        for (index, rule) in rules.iter().enumerate() {
            if rule.capability == capability
                && rule
                    .filter
                    .as_ref()
                    .is_none_or(|f| f.matches_with(ctx, vars))
            {
                return Decision {
                    action: rule.action,
                    rule: Some(index),
                };
            }
        }
    }
    Decision {
        action: default_action.into(),
        rule: None,
    }
}

/// A resolved decision with the reasoning behind it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Resolution {
    /// Identical to what [`Rules::resolve`] returns for the same inputs.
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

fn explain_resolution(
    permissions: &Permissions,
    default_action: DefaultAction,
    caller: &str,
    capability: &str,
    ctx: &serde_json::Value,
    vars: &VarBindings,
) -> Resolution {
    let Decision {
        action,
        rule: matched,
    } = resolve_with_rule(permissions, default_action, caller, capability, ctx, vars);
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

/// The actions a call could resolve to under partial knowledge, from
/// [`Rules::possible_actions`].
#[derive(Debug, Clone, PartialEq)]
pub struct PossibleActions {
    /// Where the walk ends: the first rule whose filter holds, or the
    /// default.
    pub decided: Action,
    /// Rules before that whose filter depends on what is not known yet, in
    /// rule order: the first that holds decides instead. Trailing ones that
    /// would decide the same as [`Self::decided`] are left out.
    pub conditions: Vec<ActionCondition>,
}

impl PossibleActions {
    /// Some call could proceed, possibly after a person approves it.
    pub fn is_possible(&self) -> bool {
        self.decided != Action::Deny
            || self
                .conditions
                .iter()
                .any(|condition| condition.action != Action::Deny)
    }
}

/// A rule that applies only when its filter holds, as written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionCondition {
    pub action: Action,
    pub filter: String,
}

/// First-match resolution where a filter may be unknown. A rule whose filter
/// holds ends the walk; one whose filter is unknown may match, so its action
/// is possible and the walk goes on; one whose filter fails is skipped. A walk
/// no rule ends also reaches the default.
fn possible_actions(
    permissions: &Permissions,
    default_action: DefaultAction,
    caller: &str,
    capability: &str,
    known: &serde_json::Value,
) -> PossibleActions {
    let mut conditions = Vec::new();
    let mut decided = Action::from(default_action);
    let rules = permissions.get(caller).into_iter().flatten();
    for rule in rules.filter(|rule| rule.capability == capability) {
        let Some(filter) = &rule.filter else {
            decided = rule.action;
            break;
        };
        match filter.evaluate_partial(known) {
            Truth::False => continue,
            Truth::Unknown => conditions.push(ActionCondition {
                action: rule.action,
                filter: filter.to_string(),
            }),
            Truth::True => {
                decided = rule.action;
                break;
            }
        }
    }
    let relevant = conditions
        .iter()
        .rposition(|condition: &ActionCondition| condition.action != decided)
        .map_or(0, |last| last + 1);
    conditions.truncate(relevant);
    PossibleActions {
        decided,
        conditions,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn resolve(
        permissions: &Permissions,
        default_action: DefaultAction,
        caller: &str,
        capability: &str,
        ctx: &serde_json::Value,
        vars: &VarBindings,
    ) -> Action {
        resolve_with_rule(permissions, default_action, caller, capability, ctx, vars).action
    }

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
        let resolution = explain_resolution(&perms, default, "main", capability, &ctx, bindings);
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
                "pied-piper.example/charges.list",
                Some("customerId == ${vars.customerId}"),
                Action::Allow,
            )],
            DefaultAction::Deny,
            "pied-piper.example/charges.list",
            json!({ "customerId": "hooli" }),
            &vars(&[("customerId", "pied_piper")]),
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
        assert_eq!(failure.actual, Some(json!("hooli")));
        assert_eq!(failure.expected.as_deref(), Some("pied_piper"));
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
        let resolution = explain_resolution(
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

    #[test]
    fn rule_name_is_optional_and_unknown_fields_still_rejected() {
        let rules: Vec<PermissionRule> = serde_yml::from_str(
            "- capability: a\n  action: allow\n- name: n\n  capability: b\n  action: deny\n",
        )
        .expect("rules parse");
        assert_eq!(rules[0].name, None);
        assert_eq!(rules[1].name.as_deref(), Some("n"));
        let reserialized = serde_yml::to_string(&rules).expect("serialize");
        assert!(!reserialized.contains("name: null"), "{reserialized}");
        assert!(
            serde_yml::from_str::<Vec<PermissionRule>>(
                "- capability: a\n  action: allow\n  label: x\n"
            )
            .is_err()
        );
    }

    #[test]
    fn empty_rule_name_is_invalid() {
        let perms = BTreeMap::from([(
            "main".to_string(),
            vec![rule(Some(""), "a", None, Action::Allow)],
        )]);
        assert!(validate_rules(&perms).is_err());
    }

    #[test]
    fn a_rule_set_is_validated_when_built() {
        let empty_capability = BTreeMap::from([(
            "main".to_string(),
            vec![
                rule(None, "a", None, Action::Allow),
                rule(None, "", None, Action::Allow),
            ],
        )]);
        assert_eq!(
            RuleSet::new(empty_capability, DefaultAction::Deny),
            Err(RuleError::EmptyCapability {
                caller: "main".to_string(),
                index: 1,
            })
        );
        let rules = RuleSet::new(Permissions::new(), DefaultAction::AskHuman).unwrap();
        assert_eq!(
            rules.resolve("main", "a", &json!({}), &VarBindings::new()),
            Decision {
                action: Action::AskHuman,
                rule: None,
            }
        );
    }

    fn possible(
        rules: Vec<PermissionRule>,
        default_action: DefaultAction,
        known: serde_json::Value,
    ) -> PossibleActions {
        let permissions = BTreeMap::from([("main".to_string(), rules)]);
        possible_actions(&permissions, default_action, "main", "http.request", &known)
    }

    #[test]
    fn possible_actions_follow_the_default_without_rules() {
        let denied = possible(Vec::new(), DefaultAction::Deny, json!({}));
        assert!(!denied.is_possible());
        let allowed = possible(Vec::new(), DefaultAction::Allow, json!({}));
        assert_eq!(allowed.decided, Action::Allow);
        assert!(allowed.conditions.is_empty());
        let asked = possible(Vec::new(), DefaultAction::AskHuman, json!({}));
        assert_eq!(asked.decided, Action::AskHuman);
        assert!(asked.is_possible());
    }

    #[test]
    fn unknown_filter_makes_its_action_possible_with_a_condition() {
        let get_only = possible(
            vec![rule(
                None,
                "http.request",
                Some("method == \"GET\""),
                Action::Allow,
            )],
            DefaultAction::Deny,
            json!({}),
        );
        assert!(get_only.is_possible());
        assert_eq!(get_only.decided, Action::Deny);
        assert_eq!(
            get_only.conditions,
            vec![ActionCondition {
                action: Action::Allow,
                filter: "method == \"GET\"".to_string(),
            }]
        );
    }

    #[test]
    fn known_filter_decides_like_resolution() {
        let rules = vec![rule(
            None,
            "http.request",
            Some("method == \"GET\""),
            Action::Allow,
        )];
        let get = possible(
            rules.clone(),
            DefaultAction::Deny,
            json!({ "method": "GET" }),
        );
        assert_eq!(get.decided, Action::Allow);
        assert!(get.conditions.is_empty());
        let post = possible(rules, DefaultAction::Deny, json!({ "method": "POST" }));
        assert!(!post.is_possible());
    }

    #[test]
    fn earlier_unconditional_deny_shadows_a_grant() {
        let shadowed = possible(
            vec![
                rule(None, "http.request", None, Action::Deny),
                rule(None, "http.request", None, Action::Allow),
            ],
            DefaultAction::Allow,
            json!({}),
        );
        assert!(!shadowed.is_possible());
    }

    #[test]
    fn ask_human_rule_is_possible_and_other_callers_do_not_count() {
        let asked = possible(
            vec![rule(None, "http.request", None, Action::AskHuman)],
            DefaultAction::Deny,
            json!({}),
        );
        assert_eq!(asked.decided, Action::AskHuman);

        let permissions = BTreeMap::from([(
            "@acme/tools".to_string(),
            vec![rule(None, "http.request", None, Action::Allow)],
        )]);
        let main = possible_actions(
            &permissions,
            DefaultAction::Deny,
            "main",
            "http.request",
            &json!({}),
        );
        assert!(!main.is_possible());
    }

    #[test]
    fn an_uncertain_deny_before_an_uncertain_grant_stays_a_condition() {
        let narrowed = possible(
            vec![
                rule(
                    None,
                    "http.request",
                    Some("host == \"internal\""),
                    Action::Deny,
                ),
                rule(
                    None,
                    "http.request",
                    Some("method == \"GET\""),
                    Action::Allow,
                ),
                rule(None, "http.request", Some("path == \"/x\""), Action::Deny),
            ],
            DefaultAction::Deny,
            json!({}),
        );
        let actions = narrowed
            .conditions
            .iter()
            .map(|condition| condition.action)
            .collect::<Vec<_>>();
        assert_eq!(actions, [Action::Deny, Action::Allow]);
    }
}
