//! Operator-declared configuration, resolved once when a session opens.
//! Bindings scope `${vars.NAME}` permission filters and are readable by packages
//! through `submilli:variables`. Alias names take their target's resolved value;
//! generated programs cannot supply or mutate bindings.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::{Blueprint, BlueprintError, Fault, yaml_path};

/// One declared variable. `required` and `default` are mutually exclusive: a
/// required variable with a default is contradictory (rejected at parse).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VariableDecl {
    /// The session is rejected at init if this variable is missing or empty.
    #[serde(default, skip_serializing_if = "is_false")]
    pub required: bool,
    /// Value bound when the caller supplies none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
    /// Resolve this name from another declared variable; direct bindings are refused.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// A failure binding caller-supplied variables against the blueprint's
/// declarations, surfaced at session init (REST → 400, MCP → failed
/// `initialize`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VariableError {
    /// A `required` variable was not supplied (or supplied empty).
    MissingRequired(String),
    /// A supplied variable name is not declared in the blueprint.
    Unknown(String),
    /// Alias names can only be supplied through their targets.
    AliasBinding(String),
    /// Invalid declarations can also reach the resolver through the Rust API.
    InvalidDeclaration { name: String, message: String },
}

impl fmt::Display for VariableError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VariableError::MissingRequired(name) => {
                write!(f, "required variable '{name}' was not supplied")
            }
            VariableError::AliasBinding(name) => {
                write!(f, "variable '{name}' is an alias; bind its target instead")
            }
            VariableError::InvalidDeclaration { name, message } => {
                write!(f, "variable '{name}': {message}")
            }
            VariableError::Unknown(name) => {
                write!(f, "variable '{name}' is not declared in the blueprint")
            }
        }
    }
}

impl std::error::Error for VariableError {}

/// Parse-time structural checks: `required`+`default` is contradictory, and
/// every `${vars.NAME}` a permission filter references must name a declared
/// variable. Declared-but-unused variables are allowed (no warning channel;
/// matches how `secrets:` treats unused declarations).
pub(crate) fn validate_variables(blueprint: &Blueprint) -> Result<(), BlueprintError> {
    resolution_order(&blueprint.variables).map_err(|error| {
        let path = match &error {
            VariableError::InvalidDeclaration { name, .. } => yaml_path!["variables", name],
            _ => yaml_path!["variables"],
        };
        BlueprintError::InvalidVariables(Fault::at(path, error.to_string()))
    })?;
    for (caller, rules) in &blueprint.permissions {
        for (i, rule) in rules.iter().enumerate() {
            let Some(filter) = &rule.filter else { continue };
            for name in filter.var_refs() {
                if !blueprint.variables.contains_key(name) {
                    return Err(BlueprintError::InvalidVariables(Fault::at(
                        yaml_path!["permissions", caller, i, "filter"],
                        format!(
                            "permissions for '{caller}': filter references undeclared variable \
                             '${{vars.{name}}}'"
                        ),
                    )));
                }
            }
        }
    }
    Ok(())
}

/// Bind caller-supplied values against the declarations: fill defaults, reject a
/// missing/empty `required` variable, reject any supplied name that isn't
/// declared. Returns the resolved bindings filter evaluation reads. An optional
/// variable with no value and no default is simply omitted — an absent
/// `${vars.NAME}` is a non-match at eval, never a trap.
pub fn resolve_variables(
    decls: &BTreeMap<String, VariableDecl>,
    supplied: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, String>, VariableError> {
    let order = resolution_order(decls)?;
    for name in supplied.keys() {
        let Some(decl) = decls.get(name) else {
            return Err(VariableError::Unknown(name.clone()));
        };
        if decl.alias.is_some() {
            return Err(VariableError::AliasBinding(name.clone()));
        }
    }
    let mut resolved = BTreeMap::<String, String>::new();
    for name in order {
        // The order contains only keys from this immutable declaration map.
        let decl = decls
            .get(name)
            .expect("resolution order contains declared names");
        let value = if let Some(target) = &decl.alias {
            resolved.get(target).cloned()
        } else {
            supplied
                .get(name)
                .filter(|value| !value.is_empty())
                .cloned()
                .or_else(|| decl.default.clone())
        };
        if decl.required && value.as_ref().is_none_or(String::is_empty) {
            return Err(VariableError::MissingRequired(name.to_owned()));
        }
        if let Some(value) = value {
            resolved.insert(name.to_owned(), value);
        }
    }
    Ok(resolved)
}

/// Validate a saved snapshot without applying new defaults or treating resolved
/// aliases as caller overrides. Existing session values remain authoritative.
pub fn validate_resolved_variables(
    decls: &BTreeMap<String, VariableDecl>,
    bindings: &BTreeMap<String, String>,
) -> Result<(), VariableError> {
    resolution_order(decls)?;
    for name in bindings.keys() {
        if !decls.contains_key(name) {
            return Err(VariableError::Unknown(name.clone()));
        }
    }
    for (name, decl) in decls {
        if decl.required && bindings.get(name).is_none_or(String::is_empty) {
            return Err(VariableError::MissingRequired(name.clone()));
        }
    }
    Ok(())
}

/// Target-first order, computed iteratively so an operator-supplied alias chain
/// cannot exhaust the native stack. Finished paths are reused across roots.
fn resolution_order(decls: &BTreeMap<String, VariableDecl>) -> Result<Vec<&str>, VariableError> {
    let invalid = |name: &str, message: String| VariableError::InvalidDeclaration {
        name: name.to_owned(),
        message,
    };
    for (name, decl) in decls {
        if decl.required && decl.default.is_some() {
            return Err(invalid(
                name,
                "`required: true` and `default:` are mutually exclusive".into(),
            ));
        }
        if let Some(target) = &decl.alias {
            if decl.default.is_some() {
                return Err(invalid(
                    name,
                    "`alias:` and `default:` are mutually exclusive".into(),
                ));
            }
            if !decls.contains_key(target) {
                return Err(invalid(
                    name,
                    format!("alias target '{target}' is not declared"),
                ));
            }
        }
    }
    let mut finished = BTreeSet::new();
    let mut order = Vec::new();
    for root in decls.keys() {
        let mut path = Vec::new();
        let mut visiting = BTreeSet::new();
        let mut current = root.as_str();
        while !finished.contains(current) {
            if !visiting.insert(current) {
                return Err(invalid(
                    current,
                    "alias cycle; choose a target outside the cycle".into(),
                ));
            }
            path.push(current);
            // Roots come from decls, and all targets were checked above. The map
            // is immutable throughout traversal, with no intervening callbacks.
            let decl = decls.get(current).expect("alias target was validated");
            let Some(target) = &decl.alias else { break };
            current = target;
        }
        for name in path.into_iter().rev() {
            finished.insert(name);
            order.push(name);
        }
    }
    Ok(order)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;

    fn decls(yaml: &str) -> BTreeMap<String, VariableDecl> {
        parse(yaml).expect("valid blueprint").variables
    }

    #[test]
    fn aliases_resolve_target_first_and_satisfy_required() {
        let d = decls(
            "name: x\nvariables:\n  A: { alias: B, required: true }\n  B: { alias: USER_ID }\n  USER_ID: { required: true }\n",
        );
        let supplied = BTreeMap::from([("USER_ID".into(), "user-1".into())]);
        let out = resolve_variables(&d, &supplied).unwrap();
        assert_eq!(out["A"], "user-1");
        assert_eq!(out["B"], "user-1");
        validate_resolved_variables(&d, &out).unwrap();
        assert!(matches!(
            resolve_variables(&d, &out),
            Err(VariableError::AliasBinding(_))
        ));
    }

    #[test]
    fn aliases_follow_defaults_and_optional_absence() {
        let d = decls(
            "name: x\nvariables:\n  A: { alias: B }\n  B: { default: configured }\n  C: { alias: D }\n  D: {}\n",
        );
        let out = resolve_variables(&d, &BTreeMap::new()).unwrap();
        assert_eq!(out["A"], "configured");
        assert!(!out.contains_key("C"));
        let supplied = BTreeMap::from([("B".into(), "override".into())]);
        assert_eq!(resolve_variables(&d, &supplied).unwrap()["A"], "override");
    }

    #[test]
    fn required_alias_rejects_missing_or_empty_target() {
        for target in ["{}", "{ default: '' }"] {
            let d = decls(&format!(
                "name: x\nvariables:\n  A: {{ alias: B, required: true }}\n  B: {target}\n"
            ));
            assert_eq!(
                resolve_variables(&d, &BTreeMap::new()),
                Err(VariableError::MissingRequired("A".into()))
            );
        }
    }

    #[test]
    fn rejects_invalid_alias_graphs_and_conflicting_defaults() {
        for declarations in [
            "  A: { alias: A }\n",
            "  A: { alias: B }\n  B: { alias: C }\n  C: { alias: A }\n",
            "  A: { alias: MISSING }\n",
            "  A: { alias: B, default: x }\n  B: {}\n",
        ] {
            assert!(parse(&format!("name: x\nvariables:\n{declarations}")).is_err());
        }
    }

    #[test]
    fn direct_alias_bindings_are_rejected_even_when_empty() {
        let d = decls("name: x\nvariables:\n  A: { alias: B }\n  B: {}\n");
        for value in ["", "override"] {
            assert_eq!(
                resolve_variables(&d, &BTreeMap::from([("A".into(), value.into())])),
                Err(VariableError::AliasBinding("A".into()))
            );
        }
    }

    #[test]
    fn alias_snapshots_keep_values_when_defaults_change() {
        let d = decls("name: x\nvariables:\n  A: { alias: B }\n  B: { default: first }\n");
        let snapshot = resolve_variables(&d, &BTreeMap::new()).unwrap();
        let changed = decls("name: x\nvariables:\n  A: { alias: B }\n  B: { default: second }\n");
        validate_resolved_variables(&changed, &snapshot).unwrap();
        assert_eq!(snapshot["A"], "first");
    }

    #[test]
    fn fills_default_when_absent() {
        let d = decls("name: x\nvariables:\n  region:\n    default: \"us\"\n");
        let out = resolve_variables(&d, &BTreeMap::new()).unwrap();
        assert_eq!(out.get("region").map(String::as_str), Some("us"));
    }

    #[test]
    fn supplied_overrides_default() {
        let d = decls("name: x\nvariables:\n  region:\n    default: \"us\"\n");
        let supplied = BTreeMap::from([("region".to_string(), "eu".to_string())]);
        let out = resolve_variables(&d, &supplied).unwrap();
        assert_eq!(out.get("region").map(String::as_str), Some("eu"));
    }

    #[test]
    fn missing_required_is_rejected() {
        let d = decls("name: x\nvariables:\n  tenant:\n    required: true\n");
        assert_eq!(
            resolve_variables(&d, &BTreeMap::new()),
            Err(VariableError::MissingRequired("tenant".into()))
        );
    }

    #[test]
    fn empty_required_is_rejected() {
        let d = decls("name: x\nvariables:\n  tenant:\n    required: true\n");
        let supplied = BTreeMap::from([("tenant".to_string(), String::new())]);
        assert_eq!(
            resolve_variables(&d, &supplied),
            Err(VariableError::MissingRequired("tenant".into()))
        );
    }

    #[test]
    fn unknown_supplied_name_is_rejected() {
        let d = decls("name: x\nvariables:\n  tenant:\n    required: true\n");
        let supplied = BTreeMap::from([
            ("tenant".to_string(), "t1".to_string()),
            ("bogus".to_string(), "x".to_string()),
        ]);
        assert_eq!(
            resolve_variables(&d, &supplied),
            Err(VariableError::Unknown("bogus".into()))
        );
    }

    #[test]
    fn optional_without_default_is_omitted() {
        let d = decls("name: x\nvariables:\n  note: {}\n");
        let out = resolve_variables(&d, &BTreeMap::new()).unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn required_with_default_is_rejected_at_parse() {
        let err = parse("name: x\nvariables:\n  t:\n    required: true\n    default: \"d\"\n")
            .expect_err("contradictory");
        assert!(matches!(err, BlueprintError::InvalidVariables(_)));
    }

    #[test]
    fn undeclared_var_ref_in_filter_is_rejected() {
        let err = parse(
            "name: x\npermissions:\n  main:\n    - capability: db.query\n      \
             filter: userId == ${vars.tenant}\n      action: allow\n",
        )
        .expect_err("undeclared var");
        assert!(matches!(err, BlueprintError::InvalidVariables(_)));
    }

    #[test]
    fn declared_var_ref_in_filter_parses() {
        parse(
            "name: x\nvariables:\n  tenant:\n    required: true\npermissions:\n  main:\n    \
             - capability: db.query\n      filter: userId == ${vars.tenant}\n      action: allow\n",
        )
        .expect("declared var ok");
    }

    #[test]
    fn round_trips_through_yaml() {
        let yaml = "name: x\nvariables:\n  region:\n    default: us\n  tenant:\n    required: true\n  alias: { alias: tenant, required: true }\n";
        let bp = parse(yaml).unwrap();
        let reparsed = parse(&crate::to_yaml(&bp)).unwrap();
        assert_eq!(bp.variables, reparsed.variables);
    }
}
