#![doc = include_str!("../README.md")]

#[cfg(feature = "engine")]
mod availability;
#[cfg(feature = "engine")]
mod check;
mod fields;
mod filter;
mod package_name;
mod rules;

#[cfg(feature = "engine")]
pub use availability::RulesAvailability;
#[cfg(feature = "engine")]
pub use check::PermissionCheck;
pub use fields::{unreported_field_problem, unreported_fields};
pub use filter::{
    ComparisonFailure, FailureReason, FieldMatch, FilterEvaluation, FilterExpr, Truth, VarBindings,
    is_field_name, is_valid_var_name, quote_literal,
};
pub use package_name::validate_scoped_name;
pub use rules::{
    Action, ActionCondition, Decision, DefaultAction, NearMiss, PermissionRule, Permissions,
    PossibleActions, Resolution, ResolutionCause, RuleError, RuleRef, RuleSet, Rules,
    validate_rules,
};
