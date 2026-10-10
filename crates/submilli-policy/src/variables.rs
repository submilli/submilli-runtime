//! Session variables: operator-declared, caller-supplied named values bound once
//! per session and referenced as `${vars.NAME}` in permission filters. Unlike a
//! secret (whose value the program never sees, injected only into outbound
//! auth), a variable *is* the value, matched against in a filter to scope a
//! capability to per-session data (e.g. a tenant id).

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

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
}

impl fmt::Display for VariableError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VariableError::MissingRequired(name) => {
                write!(f, "required variable '{name}' was not supplied")
            }
            VariableError::Unknown(name) => {
                write!(f, "variable '{name}' is not declared in the blueprint")
            }
        }
    }
}

impl std::error::Error for VariableError {}

/// Bind caller-supplied values against the declarations: fill defaults, reject a
/// missing/empty `required` variable, reject any supplied name that isn't
/// declared. Returns the resolved bindings filter evaluation reads. An optional
/// variable with no value and no default is simply omitted — an absent
/// `${vars.NAME}` is a non-match at eval, never a trap.
pub fn resolve_variables(
    decls: &BTreeMap<String, VariableDecl>,
    supplied: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, String>, VariableError> {
    for name in supplied.keys() {
        if !decls.contains_key(name) {
            return Err(VariableError::Unknown(name.clone()));
        }
    }
    let mut resolved = BTreeMap::new();
    for (name, decl) in decls {
        match supplied.get(name) {
            Some(value) if !value.is_empty() => {
                resolved.insert(name.clone(), value.clone());
            }
            _ => {
                if decl.required {
                    return Err(VariableError::MissingRequired(name.clone()));
                }
                if let Some(default) = &decl.default {
                    resolved.insert(name.clone(), default.clone());
                }
            }
        }
    }
    Ok(resolved)
}
