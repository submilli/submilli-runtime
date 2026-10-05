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
//! a warning. So does a rule that follows an unfiltered rule for the same
//! capability under the same caller: the first match decides, so it never
//! matches. So does a rule for a capability name nothing the caller can
//! reach provides, such as a misspelling; an `http.<method>` name the catalog
//! lacks is told apart, since `http.request` gates it. A filter that tests a
//! field the capability's check doesn't report is an error: a condition on a
//! missing field is false for every call, and true under `not`, whatever the
//! call's arguments.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Context;
use submilli_blueprint::{Action, Blueprint, DefaultAction, FilterExpr, PermissionRule};
use submilli_build::{CapabilitySchema, PackageStore};

use super::capability::action_label;
use super::capability_names::{
    UnlistedName, did_you_mean, http_method_scope, http_misspelling, unlisted_name,
};
use super::declared_packages::{self, DeclaredPackages};
use super::file::{has_capability_rule, has_matching_rule};
use super::package_secrets::missing_package_secret_warnings;
use submilli_build::blueprint_validation::unreported_field_errors;

#[derive(clap::Args)]
pub struct Args {
    /// Add the rules declared packages and their dependencies require for
    /// their own calls.
    #[arg(long)]
    fix: bool,

    /// Fail on blueprint warnings; also enabled by SUBMILLI_DENY_WARNINGS=1.
    #[arg(long)]
    deny_warnings: bool,

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
    let mut findings = inspect_blueprint(blueprint);
    for warning in &findings.warnings {
        eprintln!("warning: {}: {warning}", args.file.display());
    }
    if args.fix && !findings.fixes.is_empty() {
        apply_fixes(blueprint, findings.fixes)?;
        let updated = serde_yml::to_string(blueprint).context("serializing fixed blueprint")?;
        fs::write(&args.file, updated)
            .with_context(|| format!("writing {}", args.file.display()))?;
        eprintln!("fixed {}", args.file.display());
        let reported_warnings = findings.warnings;
        findings = inspect_blueprint(blueprint);
        for warning in &findings.warnings {
            if !reported_warnings.contains(warning) {
                eprintln!("warning: {}: {warning}", args.file.display());
            }
        }
    }
    let denied = (args.deny_warnings || submilli_build::deny_warnings_from_env())
        && !findings.warnings.is_empty();
    for error in &findings.errors {
        eprintln!("error: {}: {error}", args.file.display());
    }
    if denied {
        eprintln!(
            "error: {}",
            submilli_build::warning_denial_message(findings.warnings.len())
        );
    }
    if !findings.errors.is_empty() || denied {
        return Ok(ExitCode::from(1));
    }
    println!("✓ {} is valid", args.file.display());
    Ok(ExitCode::SUCCESS)
}

struct LintFindings {
    errors: Vec<String>,
    warnings: Vec<String>,
    fixes: Vec<MissingRequiresRule>,
}

fn inspect_blueprint(blueprint: &Blueprint) -> LintFindings {
    let mut errors = Vec::new();
    let mut fixes = Vec::new();

    let packages = declared_packages::load(blueprint, &PackageStore::default());
    errors.extend(packages.errors.iter().cloned());
    // A package that failed to load may report fields of its own, even for a
    // standard-library name, so no field list is known to be complete.
    if packages.errors.is_empty() {
        errors.extend(
            unreported_field_errors(blueprint, &packages.artifacts)
                .into_iter()
                .map(|problem| problem.message),
        );
    }

    errors.extend(duplicate_rule_name_errors(blueprint));

    let mut warnings = Vec::from_iter(default_allow_warning(blueprint));
    warnings.extend(unreachable_main_rule_warnings(blueprint));
    warnings.extend(shadowed_rule_warnings(blueprint));
    // An incomplete closure hides the dependencies past the failure, so a
    // caller block for one of them cannot be told from a stale one, and a
    // name the missing package provides cannot be told from a misspelling.
    if packages.errors.is_empty() {
        warnings.extend(stale_caller_block_warnings(
            blueprint,
            &packages.dependencies,
        ));
        warnings.extend(unlisted_capability_warnings(blueprint, &packages));
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

    LintFindings {
        errors,
        warnings,
        fixes,
    }
}

/// Decisions cite a rule by its name, so a name must pick out one rule within
/// its caller block. The same name under different callers is unambiguous.
fn duplicate_rule_name_errors(blueprint: &Blueprint) -> Vec<String> {
    let mut errors = Vec::new();
    for (caller, rules) in &blueprint.permissions {
        let mut first_seen: BTreeMap<&str, usize> = BTreeMap::new();
        for (position, rule) in (1..).zip(rules) {
            let Some(name) = rule.name.as_deref() else {
                continue;
            };
            let first = *first_seen.entry(name).or_insert(position);
            if first != position {
                errors.push(format!(
                    "`permissions.{caller}` rules {first} and {position} are both named `{name}`; \
                     rule names must be unique within a caller block"
                ));
            }
        }
    }
    errors
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
        if let Some(message) = submilli_build::blueprint_validation::missing_required_rule(
            blueprint, package, required,
        ) {
            errors.push(message);
        }
        fixes.push(MissingRequiresRule {
            caller: package.to_string(),
            capability: required.capability.clone(),
            filter: required.filter.clone(),
        });
    }
}

fn apply_fixes(blueprint: &mut Blueprint, fixes: Vec<MissingRequiresRule>) -> anyhow::Result<()> {
    for fix in fixes {
        if has_matching_rule(
            blueprint,
            &fix.caller,
            &fix.capability,
            fix.filter.as_deref(),
        ) {
            continue;
        }
        let filter = fix
            .filter
            .as_deref()
            .map(parse_filter)
            .transpose()
            .with_context(|| {
                format!(
                    "invalid capability filter for `{}` required by `{}`",
                    fix.capability, fix.caller
                )
            })?;
        blueprint
            .permissions
            .entry(fix.caller)
            .or_default()
            .push(PermissionRule {
                name: None,
                capability: fix.capability,
                filter,
                action: Action::Allow,
            });
    }
    Ok(())
}

fn parse_filter(filter: &str) -> Result<FilterExpr, serde_yml::Error> {
    serde_yml::from_str(&serde_yml::to_string(filter)?)
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

/// A caller's first rule matching a call decides it, so a rule with no filter
/// decides every call to its capability, and a later rule for that capability
/// under the same caller never matches, whatever either action is. Rules for
/// a capability the runtime refuses to `main` are never consulted there at
/// all, which `unreachable_main_rule_warnings` reports instead.
fn shadowed_rule_warnings(blueprint: &Blueprint) -> Vec<String> {
    let mut warnings = Vec::new();
    for (caller, rules) in &blueprint.permissions {
        let mut deciding: BTreeMap<&str, (usize, Action)> = BTreeMap::new();
        for (position, rule) in (1..).zip(rules) {
            if caller == interpreter::mangle::USER_PACKAGE && is_refused_to_main(&rule.capability) {
                continue;
            }
            if let Some((first, action)) = deciding.get(rule.capability.as_str()) {
                warnings.push(format!(
                    "`permissions.{caller}` rule {position} for `{}` ({}) never matches: rule \
                     {first} (`{}`, no filter) decides every call to it first",
                    rule.capability,
                    describe_rule(rule),
                    action_label(*action)
                ));
            } else if rule.filter.is_none() {
                deciding.insert(&rule.capability, (position, rule.action));
            }
        }
    }
    warnings
}

fn is_refused_to_main(capability: &str) -> bool {
    interpreter::stdlib::capabilities::find(capability)
        .is_some_and(|known| known.main_denial.is_some())
}

/// The rule's action and filter, as a reader finds it in the file.
fn describe_rule(rule: &PermissionRule) -> String {
    match &rule.filter {
        Some(filter) => format!("`{}` with filter `{filter}`", action_label(rule.action)),
        None => format!("`{}`, no filter", action_label(rule.action)),
    }
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
        .filter(|caller| is_stale_caller(blueprint, dependencies, caller))
        .map(|caller| {
            format!(
                "`permissions:` has a caller block for `{caller}`, but `{caller}` is neither in `packages:` nor a dependency of a package there"
            )
        })
        .collect()
}

fn is_stale_caller(blueprint: &Blueprint, dependencies: &BTreeSet<String>, caller: &str) -> bool {
    is_registry_package(caller)
        && !blueprint.packages.contains(caller)
        && !dependencies.contains(caller)
}

/// Rules for a name nothing lists for the caller never match, except an HTTP
/// method `http.request` gates, which matches only that method. A warning
/// rather than an error, like `capability add --force`: the policy engine
/// matches names verbatim, so the rule stays valid. A stale caller block's
/// rules are left to its own warning: its package's names are unknown once it
/// is gone.
fn unlisted_capability_warnings(blueprint: &Blueprint, packages: &DeclaredPackages) -> Vec<String> {
    let mut warnings = Vec::new();
    for (caller, rules) in &blueprint.permissions {
        if is_stale_caller(blueprint, &packages.dependencies, caller) {
            continue;
        }
        for (position, rule) in (1..).zip(rules) {
            let Some(unlisted) = unlisted_name(blueprint, packages, caller, &rule.capability)
            else {
                continue;
            };
            let rule_label = format!(
                "`permissions.{caller}` rule {position} for `{}`",
                rule.capability
            );
            warnings.push(match unlisted {
                UnlistedName::DependencyOnly {
                    dependency,
                    dependents,
                } => format!(
                    "{rule_label} never matches: `{dependency}` provides it, and only the \
                     packages that depend on it can call it; move the rule under {}, or add \
                     `{dependency}` to `packages:`",
                    dependents
                        .iter()
                        .map(|dependent| format!("`permissions.{dependent}`"))
                        .collect::<Vec<_>>()
                        .join(" or ")
                ),
                UnlistedName::UncatalogedHttpMethod { method } => {
                    format!("{rule_label} {}", http_method_scope(&method))
                }
                UnlistedName::MisspelledHttpOperation { intended, method } => {
                    format!("{rule_label} {}", http_misspelling(intended, &method))
                }
                UnlistedName::Unknown { near } => format!(
                    "{rule_label} never matches: no standard-library operation, declared \
                     package, or declared MCP server provides it{}",
                    did_you_mean(&near)
                ),
            });
        }
    }
    warnings
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

#[cfg(test)]
mod fix_failure_tests {
    use super::*;

    #[test]
    fn malformed_required_filter_returns_context_instead_of_panicking() {
        let mut blueprint = submilli_blueprint::parse("name: strict\n").unwrap();
        let original = blueprint.permissions.clone();
        let error = apply_fixes(
            &mut blueprint,
            vec![MissingRequiresRule {
                caller: "@acme/package".into(),
                capability: "fs.read".into(),
                filter: Some("path ==".into()),
            }],
        )
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("invalid capability filter for `fs.read` required by `@acme/package`")
        );
        assert_eq!(blueprint.permissions, original);
    }
}
