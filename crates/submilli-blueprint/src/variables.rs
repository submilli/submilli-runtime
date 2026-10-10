//! The blueprint `variables:` block — operator-declared, caller-supplied named
//! values bound once per session and referenced as `${vars.NAME}` in permission
//! filters. Unlike a secret (whose value the script never sees, injected only
//! into outbound auth), a variable *is* the value, matched against in a filter
//! to scope a capability to per-session data (e.g. a tenant id).
//!
//! The same declare → supply → validate-at-init channel the harness secret
//! provider will reuse: [`resolve_variables`] is the shared routine both the
//! REST `/v1/execute` path and the MCP `initialize` path run before a session is
//! allowed to open.

use crate::{Blueprint, BlueprintError, Fault, yaml_path};

/// Parse-time structural checks: `required`+`default` is contradictory, and
/// every `${vars.NAME}` a permission filter references must name a declared
/// variable. Declared-but-unused variables are allowed (no warning channel;
/// matches how `secrets:` treats unused declarations).
pub(crate) fn validate_variables(blueprint: &Blueprint) -> Result<(), BlueprintError> {
    for (name, decl) in &blueprint.variables {
        if decl.required && decl.default.is_some() {
            return Err(BlueprintError::InvalidVariables(Fault::at(
                yaml_path!["variables", name],
                format!(
                    "variable '{name}': `required: true` and `default:` are mutually exclusive"
                ),
            )));
        }
    }
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::{VariableDecl, VariableError, parse, resolve_variables};

    fn decls(yaml: &str) -> BTreeMap<String, VariableDecl> {
        parse(yaml).expect("valid blueprint").variables
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
        let yaml =
            "name: x\nvariables:\n  region:\n    default: us\n  tenant:\n    required: true\n";
        let bp = parse(yaml).unwrap();
        let reparsed = parse(&crate::to_yaml(&bp)).unwrap();
        assert_eq!(bp.variables, reparsed.variables);
    }
}
