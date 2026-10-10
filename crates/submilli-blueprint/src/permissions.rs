//! The blueprint `permissions:` block. The rules and their evaluation live in
//! `submilli-policy`; this checks what serde cannot, with the YAML path of the
//! offending key.

use std::collections::BTreeMap;

use submilli_policy::PermissionRule;

use crate::{BlueprintError, Fault, yaml_path};

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
            if rule.name.as_deref() == Some("") {
                return Err(BlueprintError::InvalidPermissions(Fault::at(
                    yaml_path!["permissions", caller, i, "name"],
                    format!("caller '{caller}': a rule has an empty name"),
                )));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use submilli_policy::Action;

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
            vec![PermissionRule {
                name: Some(String::new()),
                capability: "a".into(),
                filter: None,
                action: Action::Allow,
            }],
        )]);
        assert!(validate(&perms).is_err());
    }
}
