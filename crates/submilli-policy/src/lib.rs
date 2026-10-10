//! The Submilli policy engine: permission rules and their filter language,
//! session variables, and auth-proxy injection.
//!
//! Format-neutral: nothing here knows where a policy is written down. A
//! blueprint file is one source (`submilli-blueprint`); an embedder that keeps
//! policy elsewhere builds these types from its own and evaluates them the same
//! way.

mod auth_proxy;
mod filter;
#[cfg(feature = "engine")]
pub mod host;
mod permissions;
#[doc(hidden)]
pub mod serde_support;
mod variables;

pub use auth_proxy::{
    AuthError, AuthProxyPolicy, AuthProxyRule, AuthSpec, BasicAuth, Injections, SecretResolver,
    interpolate, resolve_injections, secret_refs,
};
pub use filter::{
    ComparisonFailure, FailureReason, FieldMatch, FilterEvaluation, FilterExpr, VarBindings,
};
pub use permissions::{
    Action, DefaultAction, NearMiss, PermissionRule, Policy, Resolution, ResolutionCause, RuleRef,
    Rules, explain, resolve, resolve_with_rule,
};
pub use variables::{VariableDecl, VariableError, resolve_variables};

/// Tooling over the filter language that is not part of the evaluation API:
/// quoting, name checks and expression structure, for editors and diffs.
#[doc(hidden)]
pub mod filter_syntax {
    pub use crate::filter::{is_field_name, is_valid_var_name, parse, quote_literal};
}
