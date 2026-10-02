//! Package requirements checked by both local lint and server registration.

use std::collections::BTreeMap;
use submilli_blueprint::Blueprint;

mod filter_fields;
pub use filter_fields::{reported_fields, unreported_field_problem, unreported_fields};

use crate::{Artifact, PackageStore, PackageStoreError, RequiredCapability};

#[derive(Debug)]
pub enum PackageValidationError {
    Store(PackageStoreError),
    MissingRule(String),
    InvalidFilter(submilli_blueprint::Fault),
}

impl std::fmt::Display for PackageValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Store(error) => error.fmt(f),
            Self::MissingRule(message) => f.write_str(message),
            Self::InvalidFilter(problem) => f.write_str(&problem.message),
        }
    }
}

/// Check the same dependency closure and required caller rules as local lint.
/// A narrowed or denied rule is an operator decision, not a missing rule.
pub fn validate_packages(
    blueprint: &Blueprint,
    store: &PackageStore,
) -> Result<(), PackageValidationError> {
    let artifacts = store
        .load_closure(blueprint.packages.iter().map(String::as_str))
        .map_err(PackageValidationError::Store)?;
    for artifact in &artifacts {
        for required in &artifact.capabilities.requires {
            if let Some(message) =
                missing_required_rule(blueprint, &artifact.metadata.package_name, required)
            {
                return Err(PackageValidationError::MissingRule(message));
            }
        }
    }
    let artifacts = artifacts
        .into_iter()
        .map(|artifact| (artifact.metadata.package_name.clone(), artifact))
        .collect();
    if let Some(problem) = unreported_field_errors(blueprint, &artifacts)
        .into_iter()
        .next()
    {
        return Err(PackageValidationError::InvalidFilter(problem));
    }
    Ok(())
}

pub fn missing_required_rule(
    blueprint: &Blueprint,
    package: &str,
    required: &RequiredCapability,
) -> Option<String> {
    if blueprint.permissions.get(package).is_some_and(|rules| {
        rules
            .iter()
            .any(|rule| rule.capability == required.capability)
    }) {
        return None;
    }
    let filter = required
        .filter
        .as_deref()
        .map_or(String::new(), |filter| format!(" with filter `{filter}`"));
    Some(format!(
        "package `{package}` requires `{}`{filter}, but `permissions.{package}` has no matching rule",
        required.capability
    ))
}

/// One error per field a rule's filter tests that its capability's check
/// doesn't report. A rule for a capability nothing loaded provides has no
/// field list to check against, so it is skipped.
pub fn unreported_field_errors(
    blueprint: &Blueprint,
    artifacts: &BTreeMap<String, Artifact>,
) -> Vec<submilli_blueprint::Fault> {
    let mut errors = Vec::new();
    for (caller, rules) in &blueprint.permissions {
        for (index, rule) in rules.iter().enumerate() {
            let position = index.saturating_add(1);
            let Some(filter) = &rule.filter else {
                continue;
            };
            let Some(reported) = reported_fields(blueprint, artifacts, &rule.capability) else {
                continue;
            };
            for field in unreported_fields(filter, &reported) {
                errors.push(submilli_blueprint::Fault::at(
                    submilli_blueprint::yaml_path!["permissions", caller, index, "filter"],
                    format!(
                        "`permissions.{caller}` rule {position} for `{}` {}",
                        rule.capability,
                        unreported_field_problem(field, &reported)
                    ),
                ));
            }
        }
    }
    errors
}
