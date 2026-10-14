//! Package requirements checked by both local lint and server registration,
//! and the context fields a blueprint's rule filters test that their
//! capability's check doesn't report (see [`unreported_fields`]).

use std::collections::{BTreeMap, BTreeSet};

use interpreter::stdlib::capabilities::{self, Capability};
use submilli_blueprint::Blueprint;
use submilli_build::{Artifact, PackageStore, PackageStoreError, RequiredCapability};
// Re-exported so blueprint tooling finds the whole field check here.
pub use submilli_policy::{unreported_field_problem, unreported_fields};

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

/// The context fields `capability`'s check reports: from the catalog for the
/// standard library, a declared MCP server, or an HTTP method only
/// `http.request` takes; and from `capabilities.yaml` for each loaded package
/// that provides it. A package may check a standard library name itself, with
/// its own context, so the two sources combine.
/// `None` when nothing known provides `capability`.
pub fn reported_fields<'a>(
    blueprint: &Blueprint,
    artifacts: &'a BTreeMap<String, Artifact>,
    capability: &str,
) -> Option<BTreeSet<&'a str>> {
    let is_mcp_server = capability
        .strip_prefix("mcp.")
        .is_some_and(|server| blueprint.mcp.contains_key(server));
    let catalog_name = if is_mcp_server {
        "mcp.<server>"
    } else {
        capability
    };
    let cataloged = capabilities::find_gating(catalog_name);
    let providers: Vec<_> = artifacts
        .values()
        .flat_map(|artifact| &artifact.capabilities.provides)
        .filter(|entry| entry.name == capability)
        .collect();
    if cataloged.is_none() && providers.is_empty() {
        return None;
    }
    let mut fields: BTreeSet<&str> = cataloged
        .into_iter()
        .flat_map(Capability::field_names)
        .collect();
    fields.extend(
        providers
            .into_iter()
            .flat_map(|entry| entry.fields.keys().map(String::as_str)),
    );
    Some(fields)
}
