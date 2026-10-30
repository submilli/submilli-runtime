//! [`PermissionCheck`]: the [`SecurityCheck`] an embedder installs to decide
//! capability calls by permission rules, as the CLI and server do.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::sync::Arc;

use interpreter::runtime::{
    Approver, AskRule, CheckOutcome, DecisionAction, DecisionCause, DecisionExplanation,
    FailureReasonRecord, FailureRecord, NearMissRecord, RuleCitation, SecurityCheck,
};
use interpreter::stdlib::capabilities;
use interpreter::stdlib::{OptionalPackage, Stdlib};

use crate::filter::{ComparisonFailure, FailureReason, VarBindings};
use crate::rules::{
    Action, Decision, NearMiss, Resolution, ResolutionCause, RuleRef, RuleSet, Rules,
};

/// A [`SecurityCheck`] that evaluates permission rules for each capability
/// check.
///
/// Deny-by-default: a capability is allowed only when a rule says so, and under
/// [`DefaultAction::Deny`](crate::DefaultAction::Deny) a rule set without rules
/// denies every capability. An allow default inverts this to allow-by-default.
/// `ask-human` and `ask-admin` answer [`CheckOutcome::Ask`], naming the rule;
/// a call that cannot ask its approver is denied with the reason given there.
///
/// Filesystem rules see absolute guest paths with `.` and `..` collapsed using
/// the same lexical resolver as VFS I/O. This also applies to both endpoints of
/// copy/move and the destination of HTTP downloads. It does not resolve symlinks.
pub struct PermissionCheck<R: ?Sized = RuleSet> {
    rules: Arc<R>,
    /// Caller-supplied `${vars.NAME}` bindings for this session, resolved at
    /// session init. Empty for callers that bind no variables.
    variables: Arc<VarBindings>,
}

impl<R: Rules + ?Sized> PermissionCheck<R> {
    /// A check over `rules` with no variable bindings.
    pub fn new(rules: Arc<R>) -> Self {
        Self::with_variables(rules, Arc::new(BTreeMap::new()))
    }

    /// As [`Self::new`], with the session's resolved variable bindings threaded
    /// into every `${vars.NAME}` filter operand.
    pub fn with_variables(rules: Arc<R>, variables: Arc<VarBindings>) -> Self {
        PermissionCheck { rules, variables }
    }
}

impl<R: Rules + Send + Sync + ?Sized + 'static> SecurityCheck for PermissionCheck<R> {
    fn audit_context<'a>(
        &self,
        capability: &str,
        context: &'a serde_json::Value,
        cwd: &str,
    ) -> Cow<'a, serde_json::Value> {
        filesystem_policy_context(capability, context, cwd).unwrap_or(Cow::Borrowed(context))
    }

    fn check(&self, caller: &str, capability: &str, context: &serde_json::Value) -> CheckOutcome {
        self.check_with_cwd(caller, capability, context, "/")
    }

    fn explain(
        &self,
        caller: &str,
        capability: &str,
        context: &serde_json::Value,
        cwd: &str,
    ) -> Option<DecisionExplanation> {
        let context = match filesystem_policy_context(capability, context, cwd) {
            Ok(context) => context,
            // `check_with_cwd` refuses these before resolution: no rule can grant them.
            Err(reason) => {
                return Some(DecisionExplanation {
                    action: DecisionAction::Deny,
                    cause: DecisionCause::RuntimeInvariant { reason },
                    near_misses: Vec::new(),
                });
            }
        };
        let resolution = self
            .rules
            .explain(caller, capability, &context, &self.variables);
        Some(explanation_of(resolution))
    }

    fn check_with_cwd(
        &self,
        caller: &str,
        capability: &str,
        context: &serde_json::Value,
        cwd: &str,
    ) -> CheckOutcome {
        let context = match filesystem_policy_context(capability, context, cwd) {
            Ok(context) => context,
            Err(reason) => return CheckOutcome::Deny { reason, rule: None },
        };
        let Decision { action, rule } =
            self.rules
                .resolve(caller, capability, &context, &self.variables);
        match action {
            Action::Allow => CheckOutcome::Allow { rule },
            Action::Deny => CheckOutcome::Deny {
                rule,
                reason: format!(
                    "policy denied {capability}{} for {caller}",
                    denial_target(capability, &context)
                ),
            },
            Action::AskHuman => self.ask(Approver::Human, caller, capability, &context, rule),
            Action::AskAdmin => self.ask(Approver::Admin, caller, capability, &context, rule),
        }
    }
}

impl<R: Rules + ?Sized> PermissionCheck<R> {
    /// The outcome of the rule at `rule` asking `approver`, with the reason a call
    /// that can't ask them is refused with.
    fn ask(
        &self,
        approver: Approver,
        caller: &str,
        capability: &str,
        context: &serde_json::Value,
        rule: Option<usize>,
    ) -> CheckOutcome {
        let reason = format!(
            "policy requires {} approval ({}) for {capability}{} (caller {caller}), \
             and approval can't be asked here",
            match approver {
                Approver::Human => "a person's",
                Approver::Admin => "an administrator's",
            },
            approver.action(),
            denial_target(capability, context)
        );
        // An ask comes from a rule: the default never asks.
        let Some(index) = rule else {
            return CheckOutcome::Deny { rule, reason };
        };
        let asked = self
            .rules
            .permissions()
            .get(caller)
            .and_then(|rules| rules.get(index));
        CheckOutcome::Ask {
            approver,
            rule: AskRule {
                caller: caller.to_owned(),
                index,
                name: asked.and_then(|rule| rule.name.clone()),
                filter: asked
                    .and_then(|rule| rule.filter.as_ref())
                    .map(ToString::to_string),
            },
            reason,
        }
    }
}

fn explanation_of(resolution: Resolution) -> DecisionExplanation {
    DecisionExplanation {
        action: match resolution.action {
            Action::Allow => DecisionAction::Allow,
            Action::Deny => DecisionAction::Deny,
            Action::AskHuman => DecisionAction::AskHuman,
            Action::AskAdmin => DecisionAction::AskAdmin,
        },
        cause: cause_of(resolution.cause),
        near_misses: resolution
            .near_misses
            .into_iter()
            .map(near_miss_record)
            .collect(),
    }
}

fn citation(rule: RuleRef) -> RuleCitation {
    RuleCitation {
        caller: rule.caller,
        index: rule.index,
        name: rule.name,
    }
}

fn cause_of(cause: ResolutionCause) -> DecisionCause {
    match cause {
        ResolutionCause::Rule(rule) => DecisionCause::Rule(citation(rule)),
        ResolutionCause::Default { caller_block } => DecisionCause::Default { caller_block },
    }
}

fn near_miss_record(miss: NearMiss) -> NearMissRecord {
    NearMissRecord {
        rule: citation(miss.rule),
        filter: miss.filter,
        failures: miss.failures.into_iter().map(failure_record).collect(),
    }
}

fn failure_record(failure: ComparisonFailure) -> FailureRecord {
    let reason = match failure.reason {
        FailureReason::FieldMissing => FailureReasonRecord::FieldMissing,
        FailureReason::VariableNotBound(name) => FailureReasonRecord::VariableNotBound(name),
        FailureReason::NotSatisfied => FailureReasonRecord::NotSatisfied,
    };
    FailureRecord {
        comparison: failure.comparison,
        actual: failure.actual,
        expected: failure.expected,
        reason,
        negated: failure.negated,
    }
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
    let every = OptionalPackage::ALL
        .iter()
        .fold(Stdlib::core(), |stdlib, package| stdlib.with(*package));
    !capability.starts_with("mcp.") && capabilities::find_for(every, capability).is_none()
}

/// The normalized VFS paths a denial was for, so a narrowed filter can be fixed
/// from the message. The library may have probed a path on the program's
/// behalf, as `submilli:code` does for ignore files, so the program's own
/// arguments do not always name it. Empty for capabilities without VFS paths.
fn filesystem_target(capability: &str, context: &serde_json::Value) -> String {
    let path = |field: &&str| context.get(*field).and_then(serde_json::Value::as_str);
    match filesystem_path_fields(capability) {
        [single] => path(single).map_or_else(String::new, |path| format!(" on {path}")),
        [from, to] => match (path(from), path(to)) {
            (Some(from), Some(to)) => format!(" from {from} to {to}"),
            _ => String::new(),
        },
        _ => String::new(),
    }
}

/// The context fields of a capability that hold VFS paths.
fn filesystem_path_fields(capability: &str) -> &'static [&'static str] {
    match capability {
        "fs.read" | "fs.write" | "fs.stat" | "fs.list" | "fs.mkdir" | "fs.remove" => &["path"],
        "fs.copy" | "fs.move" => &["from", "to"],
        "git.init" | "git.clone" | "git.fetch" | "git.commit" => &["path"],
        "http.download" => &["vfs_path"],
        _ => &[],
    }
}

/// Keep policy paths in the same guest namespace as the subsequent VFS operation.
/// Normalization is lexical and performs no I/O, so a denial cannot reveal whether
/// the target exists. URL paths and application-defined capabilities stay untouched.
fn filesystem_policy_context<'a>(
    capability: &str,
    context: &'a serde_json::Value,
    cwd: &str,
) -> Result<Cow<'a, serde_json::Value>, String> {
    let fields = filesystem_path_fields(capability);
    let mut normalized_context = Cow::Borrowed(context);
    for &field in fields {
        let path = context
            .get(field)
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| {
                format!("invalid {capability} context: {field} must be a VFS path string")
            })?;
        let normalized = interpreter::runtime::fs::guest_normalize(cwd, path)
            .map_err(|error| format!("invalid {capability} {field}: {error}"))?;
        if normalized != path {
            normalized_context.to_mut()[field] = serde_json::Value::String(normalized);
        }
    }
    Ok(normalized_context)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::{DefaultAction, Permissions};

    /// Rules from the `default:` and `permissions:` keys of a blueprint-shaped
    /// YAML document. Its `name:` and `variables:` are ignored; any other key
    /// is an error, so a misspelled key can't quietly leave deny-by-default
    /// with no rules.
    fn parse(yaml: &str) -> Result<RuleSet, String> {
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Document {
            #[serde(default, rename = "name")]
            _name: Option<String>,
            #[serde(default, rename = "variables")]
            _variables: Option<serde_yml::Value>,
            #[serde(default)]
            default: DefaultAction,
            #[serde(default)]
            permissions: Permissions,
        }
        let document: Document = serde_yml::from_str(yaml).map_err(|error| error.to_string())?;
        RuleSet::new(document.permissions, document.default).map_err(|error| error.to_string())
    }

    /// Call-site derivation normalizes the same fields from the catalog, so a
    /// derived `requires` filter names the path this check sees.
    #[test]
    fn working_directory_is_used_for_all_filesystem_policy_fields() {
        let rules = parse("name: cwd\ndefault: deny\npermissions:\n  main:\n    - {capability: fs.read, action: allow, filter: 'path glob \"/notes/*\"'}\n    - {capability: fs.move, action: allow, filter: 'from glob \"/notes/*\" and to glob \"/notes/*\"'}\n    - {capability: http.download, action: allow, filter: 'vfs_path glob \"/notes/*\"'}\n").unwrap();
        let policy = PermissionCheck::new(Arc::new(rules));
        for (capability, context) in [
            ("fs.read", serde_json::json!({"path":"a"})),
            ("fs.move", serde_json::json!({"from":"a", "to":"b"})),
            ("http.download", serde_json::json!({"vfs_path":"a"})),
        ] {
            assert!(matches!(
                policy.check_with_cwd("main", capability, &context, "/notes"),
                CheckOutcome::Allow { .. }
            ));
            assert!(matches!(
                policy.check_with_cwd("main", capability, &context, "/elsewhere"),
                CheckOutcome::Deny { .. }
            ));
        }
        assert!(matches!(
            policy.check_with_cwd(
                "main",
                "fs.read",
                &serde_json::json!({"path":"../private/a"}),
                "/notes"
            ),
            CheckOutcome::Deny { .. }
        ));
    }

    fn explained_rules() -> PermissionCheck {
        PermissionCheck::new(Arc::new(
            parse(
                r#"
name: explained
default: deny
permissions:
  main:
    - capability: http.get
      name: docs-only
      filter: 'host == "docs.example"'
      action: allow
    - capability: fs.write
      action: ask-human
  "@pied-piper/core": []
"#,
            )
            .unwrap(),
        ))
    }

    /// The explanation never disagrees with what enforcement did.
    fn assert_explains(
        policy: &PermissionCheck,
        caller: &str,
        capability: &str,
        context: &serde_json::Value,
    ) -> DecisionExplanation {
        let explanation = policy.explain(caller, capability, context, "/").unwrap();
        let outcome = policy.check_with_cwd(caller, capability, context, "/");
        assert_eq!(
            matches!(outcome, CheckOutcome::Allow { .. }),
            explanation.action == DecisionAction::Allow,
            "{capability}: {explanation:?}"
        );
        explanation
    }

    #[test]
    fn explanations_name_the_matched_rule_the_default_and_the_near_misses() {
        let policy = explained_rules();

        let matched = assert_explains(
            &policy,
            "main",
            "http.get",
            &serde_json::json!({"host": "docs.example"}),
        );
        assert_eq!(matched.action, DecisionAction::Allow);
        assert_eq!(
            matched.cause,
            DecisionCause::Rule(RuleCitation {
                caller: "main".into(),
                index: 0,
                name: Some("docs-only".into()),
            })
        );
        assert!(matched.near_misses.is_empty());

        let missed = assert_explains(
            &policy,
            "main",
            "http.get",
            &serde_json::json!({"host": "other.example"}),
        );
        assert_eq!(missed.action, DecisionAction::Deny);
        assert_eq!(missed.cause, DecisionCause::Default { caller_block: true });
        let [near] = missed.near_misses.as_slice() else {
            panic!("one near miss: {missed:?}");
        };
        assert_eq!(near.rule.name.as_deref(), Some("docs-only"));
        assert_eq!(
            near.failures[0].actual,
            Some(serde_json::json!("other.example"))
        );
        assert_eq!(near.failures[0].reason, FailureReasonRecord::NotSatisfied);

        let empty_block = assert_explains(
            &policy,
            "@pied-piper/core",
            "http.get",
            &serde_json::json!({}),
        );
        assert_eq!(
            empty_block.cause,
            DecisionCause::Default { caller_block: true }
        );
        let no_block = assert_explains(
            &policy,
            "@pied-piper/other",
            "http.get",
            &serde_json::json!({}),
        );
        assert_eq!(
            no_block.cause,
            DecisionCause::Default {
                caller_block: false
            }
        );
    }

    #[test]
    fn an_ask_rule_asks_its_approver_and_names_the_rule() {
        let policy = explained_rules();
        let context = serde_json::json!({"path": "/a"});
        let explanation = policy.explain("main", "fs.write", &context, "/").unwrap();
        assert_eq!(explanation.action, DecisionAction::AskHuman);
        assert!(matches!(explanation.cause, DecisionCause::Rule(_)));
        match policy.check_with_cwd("main", "fs.write", &context, "/") {
            CheckOutcome::Ask {
                approver,
                rule,
                reason,
            } => {
                assert_eq!(approver, Approver::Human);
                assert_eq!(
                    rule,
                    AskRule {
                        caller: "main".to_string(),
                        index: 1,
                        name: None,
                        filter: None,
                    }
                );
                assert!(reason.contains("can't be asked here"), "{reason}");
            }
            _ => panic!("ask-human asks"),
        }
        let admin = PermissionCheck::new(Arc::new(
            parse(
                "name: refunds\npermissions:\n  main:\n    - {capability: test.refund, name: large, filter: 'amount > 100', action: ask-admin}\n",
            )
            .unwrap(),
        ));
        match admin.check("main", "test.refund", &serde_json::json!({"amount": 250})) {
            CheckOutcome::Ask { approver, rule, .. } => {
                assert_eq!(approver, Approver::Admin);
                assert_eq!(rule.name.as_deref(), Some("large"));
                assert_eq!(rule.filter.as_deref(), Some("amount > 100"));
            }
            _ => panic!("ask-admin asks"),
        }
        assert!(matches!(
            admin.check("main", "test.refund", &serde_json::json!({"amount": 50})),
            CheckOutcome::Deny { .. }
        ));
    }

    #[test]
    fn a_path_that_cannot_be_normalized_is_a_runtime_invariant_with_its_reason() {
        let policy = PermissionCheck::new(Arc::new(parse("name: open\ndefault: allow\n").unwrap()));
        let context = serde_json::json!({"path": "/a\u{0}b"});
        // Enforcement is what it was: a denial with no rule, which reads as a default deny
        // to the server's audit and keeps doing so.
        match policy.check_with_cwd("main", "fs.read", &context, "/") {
            CheckOutcome::Deny { rule, reason } => {
                assert_eq!(rule, None);
                assert!(reason.starts_with("invalid fs.read path"), "{reason}");
            }
            _ => panic!("an unnormalizable path is refused even under default: allow"),
        }
        let explanation = policy.explain("main", "fs.read", &context, "/").unwrap();
        assert_eq!(explanation.action, DecisionAction::Deny);
        let DecisionCause::RuntimeInvariant { reason } = explanation.cause else {
            panic!("expected a runtime invariant: {explanation:?}");
        };
        assert!(reason.starts_with("invalid fs.read path"), "{reason}");
        assert!(reason.to_lowercase().contains("nul"), "{reason}");
        assert!(explanation.near_misses.is_empty());

        // A context without the field is the same kind of refusal.
        let missing = policy
            .explain("main", "fs.read", &serde_json::json!({}), "/")
            .unwrap();
        assert!(matches!(
            missing.cause,
            DecisionCause::RuntimeInvariant { .. }
        ));
    }

    #[test]
    fn normalized_policy_paths_are_vfs_path_fields_in_the_catalog() {
        use interpreter::stdlib::capabilities::{FieldNormalization, catalog};
        for capability in catalog().iter().flat_map(|group| group.capabilities) {
            for field in filesystem_path_fields(capability.name) {
                let normalization = capability
                    .filter_fields
                    .iter()
                    .find(|candidate| candidate.name == *field)
                    .map(|candidate| candidate.normalization);
                assert_eq!(
                    normalization,
                    Some(FieldNormalization::VfsPath),
                    "{}.{field}",
                    capability.name
                );
            }
        }
    }

    #[test]
    fn filesystem_policy_matches_normalized_guest_paths() {
        let rules = parse(
            r#"
name: tenant
variables:
  user: { required: true }
permissions:
  main:
    - capability: fs.read
      filter: 'path == "/${vars.user}" or path glob "/${vars.user}/*"'
      action: allow
    - capability: fs.write
      filter: 'path == "/${vars.user}" or path glob "/${vars.user}/*"'
      action: allow
    - capability: fs.stat
      filter: 'path == "/${vars.user}" or path glob "/${vars.user}/*"'
      action: allow
    - capability: fs.list
      filter: 'path == "/${vars.user}" or path glob "/${vars.user}/*"'
      action: allow
    - capability: fs.mkdir
      filter: 'path == "/${vars.user}" or path glob "/${vars.user}/*"'
      action: allow
    - capability: fs.remove
      filter: 'path == "/${vars.user}" or path glob "/${vars.user}/*"'
      action: allow
    - capability: fs.copy
      filter: 'from glob "/${vars.user}/*" and to glob "/${vars.user}/*"'
      action: allow
    - capability: fs.move
      filter: 'from glob "/${vars.user}/*" and to glob "/${vars.user}/*"'
      action: allow
    - capability: http.download
      filter: 'vfs_path glob "/${vars.user}/*" and url_path == "/../remote"'
      action: allow
"#,
        )
        .unwrap();
        let policy = PermissionCheck::with_variables(
            Arc::new(rules),
            Arc::new(BTreeMap::from([("user".into(), "ada".into())])),
        );
        for capability in [
            "fs.read",
            "fs.write",
            "fs.stat",
            "fs.list",
            "fs.mkdir",
            "fs.remove",
        ] {
            for path in [
                "/ada/file",
                "ada/file",
                "/ada/./file",
                "//ada//file",
                "/grace/../ada/file",
                "/ada/",
            ] {
                assert!(
                    matches!(
                        policy.check("main", capability, &serde_json::json!({"path": path})),
                        CheckOutcome::Allow { .. }
                    ),
                    "{capability}: {path}"
                );
            }
            for path in [
                "/grace/file",
                "/ada/../grace/file",
                "ada/../grace/file",
                "/ada/../../grace/file",
                "/ada/\0file",
            ] {
                assert!(
                    matches!(
                        policy.check("main", capability, &serde_json::json!({"path": path})),
                        CheckOutcome::Deny { .. }
                    ),
                    "{capability}: {path}"
                );
            }
        }
        for capability in ["fs.copy", "fs.move"] {
            assert!(matches!(
                policy.check(
                    "main",
                    capability,
                    &serde_json::json!({"from": "ada/./file", "to": "ada/new"})
                ),
                CheckOutcome::Allow { .. }
            ));
            for (from, to) in [
                ("/ada/../grace/file", "/ada/new"),
                ("/ada/file", "/ada/../grace/new"),
            ] {
                assert!(matches!(
                    policy.check(
                        "main",
                        capability,
                        &serde_json::json!({"from": from, "to": to})
                    ),
                    CheckOutcome::Deny { .. }
                ));
            }
        }
        for (path, allowed) in [("ada/download", true), ("/ada/../grace/download", false)] {
            assert_eq!(
                matches!(
                    policy.check(
                        "main",
                        "http.download",
                        &serde_json::json!({"vfs_path": path, "url_path": "/../remote"})
                    ),
                    CheckOutcome::Allow { .. }
                ),
                allowed
            );
        }
    }

    #[test]
    fn filesystem_denials_name_the_normalized_path() {
        let policy = PermissionCheck::new(Arc::new(
            parse(
                r#"
name: closed
permissions:
  main:
    - capability: fs.write
      action: ask-human
"#,
            )
            .unwrap(),
        ));
        let reason = |capability: &str, context: serde_json::Value| match policy
            .check("main", capability, &context)
        {
            CheckOutcome::Deny { reason, .. } | CheckOutcome::Ask { reason, .. } => reason,
            _ => panic!("{capability} not denied"),
        };
        assert_eq!(
            reason(
                "fs.stat",
                serde_json::json!({"path": "/repo/../.gitignore"})
            ),
            "policy denied fs.stat on /.gitignore for main"
        );
        assert_eq!(
            reason("fs.write", serde_json::json!({"path": "notes/a.md"})),
            "policy requires a person's approval (ask-human) for fs.write on /notes/a.md \
             (caller main), and approval can't be asked here"
        );
        assert_eq!(
            reason("fs.move", serde_json::json!({"from": "a", "to": "/b/./c"})),
            "policy denied fs.move from /a to /b/c for main"
        );
        assert_eq!(
            reason("http.download", serde_json::json!({"vfs_path": "dl"})),
            "policy denied http.download on /dl for main"
        );
        assert_eq!(
            reason("secrets.get", serde_json::json!({"name": "token"})),
            "policy denied secrets.get for main"
        );
    }

    #[test]
    fn an_application_capabilitys_denial_names_the_values_it_was_for() {
        let policy = PermissionCheck::new(Arc::new(parse("name: closed\n").unwrap()));
        let reason = |capability: &str, context: serde_json::Value| match policy
            .check("main", capability, &context)
        {
            CheckOutcome::Deny { reason, .. } => reason,
            _ => panic!("{capability} not denied"),
        };
        assert_eq!(
            reason(
                "pied-piper.example/charges.list",
                serde_json::json!({ "customerId": "cus_globex" })
            ),
            "policy denied pied-piper.example/charges.list with customerId = \"cus_globex\" for main"
        );
        // Scalars only, a few of them, each cut short.
        let long = "x".repeat(70);
        assert_eq!(
            reason(
                "pied-piper.example/refund",
                serde_json::json!({
                    "a": 1, "b": true, "c": { "nested": 1 }, "d": [1],
                    "e": long, "f": "y", "g": "z"
                })
            ),
            format!(
                "policy denied pied-piper.example/refund with a = 1, b = true, e = \"{}…\", f = \"y\", … \
                 for main",
                "x".repeat(64)
            )
        );
        assert_eq!(
            reason("pied-piper.example/ping", serde_json::json!({})),
            "policy denied pied-piper.example/ping for main"
        );
        // A built-in capability's values can come from the runtime rather than the
        // program, and an MCP tool's are its whole arguments: neither is named.
        assert_eq!(
            reason(
                "http.get",
                serde_json::json!({ "url": "https://x.test/?sig=1", "host": "x.test" })
            ),
            "policy denied http.get for main"
        );
        assert_eq!(
            reason("llm.call", serde_json::json!({ "model": "m" })),
            "policy denied llm.call for main"
        );
        assert_eq!(
            reason(
                "mcp.linear/create_issue",
                serde_json::json!({ "title": "t" })
            ),
            "policy denied mcp.linear/create_issue for main"
        );
    }

    #[test]
    fn filesystem_policy_blocklists_use_the_resolved_path() {
        let policy = PermissionCheck::new(Arc::new(
            parse(
                r#"
name: blocked
default: allow
permissions:
  main:
    - capability: fs.read
      filter: 'path glob "/grace/*"'
      action: deny
"#,
            )
            .unwrap(),
        ));
        assert!(matches!(
            policy.check(
                "main",
                "fs.read",
                &serde_json::json!({"path": "/ada/../grace/file"})
            ),
            CheckOutcome::Deny { .. }
        ));
        assert!(matches!(
            policy.check("main", "fs.read", &serde_json::json!({"path": "/ada/file"})),
            CheckOutcome::Allow { .. }
        ));
        for context in [
            serde_json::json!({}),
            serde_json::json!({"path": 42}),
            serde_json::json!({"path": "/../../grace/file"}),
        ] {
            assert!(matches!(
                policy.check("main", "fs.read", &context),
                CheckOutcome::Deny { .. }
            ));
        }
        assert!(matches!(
            policy.check(
                "main",
                "custom.read",
                &serde_json::json!({"path": "/../remote"})
            ),
            CheckOutcome::Allow { .. }
        ));
    }
}
