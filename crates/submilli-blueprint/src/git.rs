//! Blueprint-controlled Git identity and session-variable interpolation.
use serde::{Deserialize, Serialize};

use crate::{Blueprint, BlueprintError, Fault, VarBindings, yaml_path};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GitConfig {
    pub identity: GitIdentity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GitIdentity {
    pub name: String,
    pub email: String,
}

impl GitConfig {
    /// Resolve once against the session's already-validated variable bindings.
    pub fn resolve(&self, variables: &VarBindings) -> Result<Self, BlueprintError> {
        let resolve = |field, value: &str| -> Result<String, BlueprintError> {
            let result = expand(field, value, |name| {
                variables.get(name).cloned().ok_or_else(|| {
                    fault(field, format!("variable '${{vars.{name}}}' has no value"))
                })
            })?;
            validate_value(field, &result)?;
            Ok(result)
        };
        Ok(Self {
            identity: GitIdentity {
                name: resolve("name", &self.identity.name)?,
                email: resolve("email", &self.identity.email)?,
            },
            username: self
                .username
                .as_deref()
                .map(|v| resolve("username", v))
                .transpose()?,
        })
    }
}

pub(crate) fn validate(blueprint: &Blueprint) -> Result<(), BlueprintError> {
    let Some(git) = &blueprint.git else {
        return Ok(());
    };
    for (field, value) in [
        ("name", Some(git.identity.name.as_str())),
        ("email", Some(git.identity.email.as_str())),
        ("username", git.username.as_deref()),
    ] {
        let Some(value) = value else {
            continue;
        };
        validate_value(field, value)?;
        expand(field, value, |name| {
            if !blueprint.variables.contains_key(name) {
                return Err(fault(
                    field,
                    format!("references undeclared variable '${{vars.{name}}}'"),
                ));
            }
            Ok(String::new())
        })?;
    }
    Ok(())
}

fn expand(
    field: &str,
    value: &str,
    mut lookup: impl FnMut(&str) -> Result<String, BlueprintError>,
) -> Result<String, BlueprintError> {
    let mut rest = value;
    let mut output = String::new();
    while let Some(start) = rest.find("${") {
        output.push_str(&rest[..start]);
        let tail = &rest[start + 2..];
        let end = tail
            .find('}')
            .ok_or_else(|| fault(field, "unterminated variable reference"))?;
        let name = tail[..end]
            .strip_prefix("vars.")
            .filter(|name| !name.is_empty())
            .ok_or_else(|| fault(field, "only ${vars.NAME} references are supported"))?;
        output.push_str(&lookup(name)?);
        rest = &tail[end + 1..];
    }
    output.push_str(rest);
    Ok(output)
}

fn validate_value(field: &str, value: &str) -> Result<(), BlueprintError> {
    if value.trim().is_empty()
        || value.chars().any(char::is_control)
        || (field == "username" && value.contains(':'))
        || (field != "username" && value.contains(['<', '>']))
    {
        return Err(fault(
            field,
            "must be nonempty and contain no control characters or identity delimiters",
        ));
    }
    Ok(())
}

fn fault(field: &str, message: impl Into<String>) -> BlueprintError {
    let path = if field == "username" {
        yaml_path!["git", field]
    } else {
        yaml_path!["git", "identity", field]
    };
    BlueprintError::InvalidGit(Fault::at(path, message))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn templates_use_session_values_without_recursive_expansion() {
        let bp = crate::parse("name: test\nvariables:\n  author: { required: true }\ngit:\n  identity:\n    name: 'Agent ${vars.author}'\n    email: agent@example.com\n").unwrap();
        let bindings = VarBindings::from([("author".into(), "${vars.other}".into())]);
        let git = bp.git.unwrap();
        assert_eq!(
            git.resolve(&bindings).unwrap().identity.name,
            "Agent ${vars.other}"
        );
        assert!(git.resolve(&VarBindings::new()).is_err());
    }

    #[test]
    fn identity_and_declared_references_are_required() {
        for block in [
            "{}",
            "{ identity: { name: '', email: a@b } }",
            "{ identity: { name: '${vars.missing}', email: a@b } }",
        ] {
            assert!(crate::parse(&format!("name: test\ngit: {block}\n")).is_err());
        }
        assert!(crate::parse("name: test").unwrap().git.is_none());
    }
}
