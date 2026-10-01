//! `submilli blueprint lint <file>` — static validation of a blueprint file.
//! Runs offline; no server needed. Reports errors and exits non-zero.
//!
//! The checks live in [`submilli_blueprint::parse`], which validates: YAML
//! well-formedness, every `secrets:` entry has a supported source, every
//! `${secrets.X}` interpolation in `auth_proxy:` references a declared secret,
//! the blueprint name, and that all filters parse. Package artifact validation
//! loads each declared package's `capabilities.yaml`, and those of the packages
//! it depends on, whose calls run as their own callers: missing `requires:`
//! rules are errors. A dependency needs no `packages:` entry; that list is what
//! `main` may import. A `provides:` capability without a `main` rule is not a finding:
//! leaving it out is how a blueprint withholds it, and
//! `blueprint capability list --unconfigured` lists those. `default: allow`
//! parses, but inverts the security posture to allow-by-default, so it lints as
//! a warning.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Context;
use submilli_blueprint::{Action, Blueprint, DefaultAction, FilterExpr, PermissionRule};
use submilli_build::{CapabilitySchema, PackageStore};

use super::declared_packages;
use super::file::{has_capability_rule, has_matching_rule};
use super::package_secrets::missing_package_secret_warnings;

#[derive(clap::Args)]
pub struct Args {
    /// Add the rules declared packages and their dependencies require for
    /// their own calls.
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
    let mut errors = Vec::new();
    let mut unfixable_errors = Vec::new();
    let mut fixes = Vec::new();

    let packages = declared_packages::load(blueprint, &PackageStore::default());
    errors.extend(packages.errors.iter().cloned());
    unfixable_errors.extend(packages.errors.iter().cloned());

    let mut warnings = Vec::from_iter(default_allow_warning(blueprint));
    warnings.extend(unreachable_main_rule_warnings(blueprint));
    // An incomplete closure hides the dependencies past the failure, so a
    // caller block for one of them cannot be told from a stale one.
    if packages.errors.is_empty() {
        warnings.extend(stale_caller_block_warnings(
            blueprint,
            &packages.dependencies,
        ));
    }
    warnings.extend(missing_caller_block_warnings(blueprint));
    for (package, artifact) in &packages.artifacts {
        warnings.extend(missing_package_secret_warnings(
            blueprint,
            package,
            &artifact.capabilities,
        ));
        collect_requires_findings(
            blueprint,
            package,
            &artifact.capabilities,
            &mut warnings,
            &mut errors,
            &mut fixes,
        );
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

/// A rule a package's `requires:` needs for its own calls; `--fix` grants it.
#[derive(Clone, Debug, PartialEq)]
struct MissingRequiresRule {
    caller: String,
    capability: String,
    filter: Option<String>,
}

fn collect_requires_findings(
    blueprint: &Blueprint,
    package: &str,
    capabilities: &CapabilitySchema,
    warnings: &mut Vec<String>,
    errors: &mut Vec<String>,
    fixes: &mut Vec<MissingRequiresRule>,
) {
    for required in &capabilities.requires {
        if has_matching_rule(
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
        // A different rule for the same capability is the operator narrowing or
        // denying the package on purpose; `--fix` must not append the broad rule
        // behind it, which would match whatever the narrowed rule leaves out.
        if has_capability_rule(blueprint, package, &required.capability) {
            warnings.push(format!(
                "package `{package}` requires `{}`{filter}; `permissions.{package}` grants it differently, so the package's calls outside those rules are denied",
                required.capability
            ));
            continue;
        }
        errors.push(format!(
            "package `{package}` requires `{}`{filter}, but `permissions.{package}` has no matching rule",
            required.capability
        ));
        fixes.push(MissingRequiresRule {
            caller: package.to_string(),
            capability: required.capability.clone(),
            filter: required.filter.clone(),
        });
    }
}

fn apply_fixes(blueprint: &mut Blueprint, fixes: Vec<MissingRequiresRule>) {
    for fix in fixes {
        if has_matching_rule(
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
                action: Action::Allow,
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

/// `dependencies` are the undeclared packages declared ones depend on: their
/// caller blocks belong in the blueprint without a `packages:` entry.
fn stale_caller_block_warnings(
    blueprint: &Blueprint,
    dependencies: &BTreeSet<String>,
) -> Vec<String> {
    blueprint
        .permissions
        .keys()
        .filter(|caller| {
            is_registry_package(caller)
                && !blueprint.packages.contains(*caller)
                && !dependencies.contains(*caller)
        })
        .map(|caller| {
            format!(
                "`permissions:` has a caller block for `{caller}`, but `{caller}` is neither in `packages:` nor a dependency of a package there"
            )
        })
        .collect()
}

fn missing_caller_block_warnings(blueprint: &Blueprint) -> Vec<String> {
    if !blueprint.has_permission_policy() {
        return Vec::new();
    }
    blueprint
        .packages
        .iter()
        .filter(|package| !blueprint.permissions.contains_key(*package))
        .map(|package| {
            format!(
                "`packages:` declares `{package}`, but `permissions:` has no `{package}` caller block"
            )
        })
        .collect()
}

fn is_registry_package(caller: &str) -> bool {
    caller.starts_with('@') && !caller.starts_with("@mcp/")
}
