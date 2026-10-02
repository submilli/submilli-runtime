//! Package requirements checked by both local lint and server registration.

use submilli_blueprint::Blueprint;

use crate::{PackageStore, PackageStoreError, RequiredCapability};

#[derive(Debug)]
pub enum PackageValidationError {
    Store(PackageStoreError),
    MissingRule(String),
}

impl std::fmt::Display for PackageValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Store(error) => error.fmt(f),
            Self::MissingRule(message) => f.write_str(message),
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
    for artifact in artifacts {
        for required in &artifact.capabilities.requires {
            if let Some(message) =
                missing_required_rule(blueprint, &artifact.metadata.package_name, required)
            {
                return Err(PackageValidationError::MissingRule(message));
            }
        }
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
