//! `submilli blueprint lint <file>` — static validation of a blueprint file.
//! Runs offline; no server needed. Reports errors and exits non-zero.
//!
//! The checks live in [`submilli_blueprint::parse`], which validates: YAML
//! well-formedness, every `secrets:` entry has a supported source, every
//! `${secrets.X}` interpolation in `auth_proxy:` references a declared secret,
//! the blueprint name, and that all filters parse. Package artifact validation
//! loads each declared package's `capabilities.yaml`: missing `provides:` rules
//! are warnings; missing `requires:` rules are errors. `default: allow` parses,
//! but inverts the security posture to allow-by-default, so it lints as a
//! warning.

use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Context;
use submilli_blueprint::{Action, Blueprint, DefaultAction, FilterExpr, PermissionRule};
use submilli_build::{CapabilitySchema, PackageStore};

use super::package_secrets::missing_package_secret_warnings;

#[derive(clap::Args)]
pub struct Args {
    /// Add missing package capability rules to the blueprint file.
    #[arg(long)]
    fix: bool,

    /// Path to the blueprint YAML file to lint.
    file: PathBuf,
}

pub fn execute(args: Args) -> anyhow::Result<ExitCode> {
    let yaml = fs::read_to_string(&args.file)
        .with_context(|| format!("reading {}", args.file.display()))?;

    match submilli_blueprint::parse(&yaml) {
        Ok(mut bp) => lint_blueprint(args, &mut bp),
        Err(err) => {
            eprintln!("error: {}: {err}", args.file.display());
            Ok(ExitCode::from(1))
        }
    }
}

fn lint_blueprint(args: Args, blueprint: &mut Blueprint) -> anyhow::Result<ExitCode> {
    let store = PackageStore::default();
    let mut warnings = Vec::from_iter(default_allow_warning(blueprint));
    warnings.extend(unreachable_main_rule_warnings(blueprint));
    warnings.extend(package_permission_warnings(blueprint));
    let mut errors = Vec::new();
    let mut unfixable_errors = Vec::new();
    let mut fixes = Vec::new();

    for package in &blueprint.packages {
        match store.load(package) {
            Ok(artifact) => {
                warnings.extend(missing_package_secret_warnings(
                    blueprint,
                    package,
                    &artifact.capabilities,
                ));
                collect_capability_findings(
                    blueprint,
                    package,
                    &artifact.capabilities,
                    &mut warnings,
                    &mut errors,
                    &mut fixes,
                );
            }
            Err(err) => {
                let error = format!("cannot validate package `{package}` capabilities: {err}");
                errors.push(error.clone());
                unfixable_errors.push(error);
            }
        }
    }

    for warning in &warnings {
        eprintln!("warning: {}: {warning}", args.file.display());
    }
    if args.fix && !fixes.is_empty() {
        apply_fixes(blueprint, fixes);
        let updated = submilli_blueprint::to_yaml(blueprint);
        fs::write(&args.file, updated)
            .with_context(|| format!("writing {}", args.file.display()))?;
        eprintln!("fixed {}", args.file.display());
        errors = unfixable_errors;
    }
    if errors.is_empty() {
        println!("✓ {} is valid", args.file.display());
        Ok(ExitCode::SUCCESS)
    } else {
        for error in errors {
            eprintln!("error: {}: {error}", args.file.display());
        }
        Ok(ExitCode::from(1))
    }
}

#[derive(Clone, Debug, PartialEq)]
struct MissingRule {
    caller: String,
    capability: String,
    filter: Option<String>,
    action: Action,
}

fn collect_capability_findings(
    blueprint: &Blueprint,
    package: &str,
    capabilities: &CapabilitySchema,
    warnings: &mut Vec<String>,
    errors: &mut Vec<String>,
    fixes: &mut Vec<MissingRule>,
) {
    for provided in &capabilities.provides {
        if has_capability_rule(blueprint, "main", &provided.name) {
            continue;
        }
        warnings.push(format!(
            "package `{package}` provides `{}`, but `permissions.main` has no matching rule",
            provided.name
        ));
        fixes.push(MissingRule {
            caller: "main".to_string(),
            capability: provided.name.clone(),
            filter: None,
            action: Action::AskHuman,
        });
    }

    for required in &capabilities.requires {
        if has_rule(
            blueprint,
            package,
            &required.capability,
            required.filter.as_deref(),
        ) {
            continue;
        }
        let filter = required
            .filter
            .as_deref()
            .map_or(String::new(), |filter| format!(" with filter `{filter}`"));
        errors.push(format!(
            "package `{package}` requires `{}`{filter}, but `permissions.{package}` has no matching rule",
            required.capability
        ));
        fixes.push(MissingRule {
            caller: package.to_string(),
            capability: required.capability.clone(),
            filter: required.filter.clone(),
            action: Action::Allow,
        });
    }
}

fn has_rule(blueprint: &Blueprint, caller: &str, capability: &str, filter: Option<&str>) -> bool {
    let Some(rules) = blueprint.permissions.get(caller) else {
        return false;
    };
    rules.iter().any(|rule| {
        rule.capability == capability
            && (rule.filter.is_none()
                || rule.filter.as_ref().map(ToString::to_string).as_deref() == filter)
    })
}

fn has_capability_rule(blueprint: &Blueprint, caller: &str, capability: &str) -> bool {
    blueprint
        .permissions
        .get(caller)
        .is_some_and(|rules| rules.iter().any(|rule| rule.capability == capability))
}

fn apply_fixes(blueprint: &mut Blueprint, fixes: Vec<MissingRule>) {
    for fix in fixes {
        if has_rule(
            blueprint,
            &fix.caller,
            &fix.capability,
            fix.filter.as_deref(),
        ) {
            continue;
        }
        blueprint
            .permissions
            .entry(fix.caller)
            .or_default()
            .push(PermissionRule {
                capability: fix.capability,
                filter: fix
                    .filter
                    .as_deref()
                    .map(parse_filter)
                    .transpose()
                    .expect("compiler-emitted capability filters must parse as blueprint filters"),
                action: fix.action,
            });
    }
}

fn parse_filter(filter: &str) -> Result<FilterExpr, serde_yml::Error> {
    serde_yml::from_str(&serde_yml::to_string(filter).expect("filter string serializes"))
}

/// Warning rather than error: the form stays valid, but it inverts the posture
/// — every capability without an explicit rule resolves to allow.
fn default_allow_warning(blueprint: &Blueprint) -> Option<String> {
    (blueprint.default_action == Some(DefaultAction::Allow)).then(|| {
        "`default: allow` makes every capability allowed unless explicitly denied; \
         prefer `default: deny` (or `ask-human`) with explicit `allow` rules for the \
         capabilities the script actually needs"
            .to_string()
    })
}

/// Rules under `main:` for a capability the runtime refuses to `main` outright.
/// The migration path for blueprints written before the carve-out: the rule
/// parses and looks effective, but the runtime never consults it.
fn unreachable_main_rule_warnings(blueprint: &Blueprint) -> Vec<String> {
    let Some(rules) = blueprint.permissions.get(interpreter::mangle::USER_PACKAGE) else {
        return Vec::new();
    };
    rules
        .iter()
        .filter_map(|rule| {
            let reason = interpreter::stdlib::capabilities::find(&rule.capability)?.main_denial?;
            Some(format!(
                "`{}` under `main:` has no effect — {reason}",
                rule.capability,
            ))
        })
        .collect()
}

fn package_permission_warnings(blueprint: &Blueprint) -> Vec<String> {
    let mut warnings = Vec::new();

    for caller in blueprint.permissions.keys() {
        if is_registry_package(caller) && !blueprint.packages.contains(caller) {
            warnings.push(format!(
                "`permissions:` has a caller block for `{caller}`, but `{caller}` is absent from `packages:`"
            ));
        }
    }

    if blueprint.has_permission_policy() {
        for package in &blueprint.packages {
            if !blueprint.permissions.contains_key(package) {
                warnings.push(format!(
                    "`packages:` declares `{package}`, but `permissions:` has no `{package}` caller block"
                ));
            }
        }
    }

    warnings
}

fn is_registry_package(caller: &str) -> bool {
    caller.starts_with('@') && !caller.starts_with("@mcp/")
}
