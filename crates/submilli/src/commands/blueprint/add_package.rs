//! `submilli blueprint add-package <pkg>` — add an already-installed package
//! to a local `blueprint.yaml` and generate its starter permission rules.
//!
//! The package gets a caller list from its `requires`, and so does every
//! package it depends on, whose calls run as their own callers. Only the named
//! package is listed under `packages:`: that list is what `main` may import,
//! and a dependency is reached only through the package that uses it.
//!
//! Which of the package's *provided* capabilities get `allow` rules under
//! `main` is a selection: `--capabilities` / `--all-capabilities` /
//! `--no-capabilities`, or an interactive multi-select on a TTY. Everything
//! unselected falls through to the blueprint's `default:` and needs no rule,
//! except under `default: allow`, where it gets an explicit `deny`.
//!
//! Rules the operator already has for a capability come first and win, so the
//! command keeps them instead of appending a rule that could never match.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use submilli_blueprint::{Action, Blueprint, DefaultAction, FilterExpr, PermissionRule};
use submilli_build::{Artifact, CapabilitySchema, PackageStore, ProvidedCapability};

use super::capability::{action_label, unruled_call_outcome};
use super::declared_packages;
use super::file::{has_capability_rule, has_matching_rule, has_unfiltered_capability_rule};
use super::package_secrets::missing_package_secret_warnings;

const DEFAULT_FILE: &str = "blueprint.yaml";

#[derive(clap::Args)]
#[command(group = clap::ArgGroup::new("selection")
    .args(["capabilities", "all_capabilities", "no_capabilities"]))]
pub struct Args {
    /// Package name to add, e.g. `@stripe/sdk`. The packages it depends on
    /// get caller rules too.
    package: String,
    /// Provided capabilities to allow for `main`, comma-separated.
    #[arg(long, value_delimiter = ',', value_name = "NAME,...")]
    capabilities: Vec<String>,
    /// Allow every capability the package provides.
    #[arg(long)]
    all_capabilities: bool,
    /// Allow none of the provided capabilities (the blueprint's `default:`
    /// decides them; under `default: allow` they get explicit `deny` rules).
    #[arg(long)]
    no_capabilities: bool,
    /// Blueprint file to edit (default: ./blueprint.yaml).
    #[arg(long)]
    blueprint: Option<PathBuf>,
}

pub fn execute(args: Args) -> Result<ExitCode> {
    match run(&args) {
        Ok(summaries) => {
            for summary in summaries {
                println!("✓ {summary}");
            }
            Ok(ExitCode::SUCCESS)
        }
        Err(e) => {
            eprintln!("error: {e:#}");
            Ok(ExitCode::from(1))
        }
    }
}

fn run(args: &Args) -> Result<Vec<String>> {
    reject_versioned_spec(&args.package)?;

    let path = args
        .blueprint
        .clone()
        .unwrap_or_else(|| PathBuf::from(DEFAULT_FILE));
    let yaml = fs::read_to_string(&path).with_context(|| {
        format!(
            "reading {} (run `submilli blueprint init` first?)",
            path.display()
        )
    })?;
    let mut blueprint =
        submilli_blueprint::parse(&yaml).with_context(|| format!("parsing {}", path.display()))?;

    if blueprint.packages.contains(&args.package) {
        bail!(
            "blueprint '{}' already declares package '{}'; grant `main` one of its provided capabilities with `submilli blueprint capability add <name>`",
            blueprint.name,
            args.package
        );
    }

    // Dependencies come before their dependents, so the named package is last.
    let closure = PackageStore::default()
        .load_closure([args.package.as_str()])
        .with_context(|| format!("loading package '{}'", args.package))?;
    let Some(named) = closure.last() else {
        bail!("loading package '{}' returned no artifact", args.package);
    };
    let selection = resolve_selection(args, &named.capabilities.provides)?;

    if !blueprint.has_permission_policy() {
        blueprint.default_action = Some(DefaultAction::Deny);
    }

    let mut warnings =
        missing_package_secret_warnings(&blueprint, &args.package, &named.capabilities);
    let addition = add_named_package(&mut blueprint, named, &selection)?;
    let mut summaries = vec![summary(
        &args.package,
        &path,
        &addition,
        blueprint.default_action,
    )];
    summaries.extend(add_dependency_callers(
        &mut blueprint,
        &closure,
        &mut warnings,
    )?);

    let updated = annotate_main_rules(
        &submilli_blueprint::to_yaml(&blueprint),
        &addition.main_comments,
    );
    submilli_blueprint::parse(&updated)
        .context("the resulting blueprint is invalid — not written")?;
    fs::write(&path, &updated).with_context(|| format!("writing {}", path.display()))?;
    for warning in warnings {
        eprintln!("warning: {}: {warning}", path.display());
    }

    Ok(summaries)
}

/// What adding the named package wrote, and what it left to the operator's
/// rules.
struct PackageAddition {
    main: RuleChanges,
    caller: RuleChanges,
    left_to_default: usize,
    /// The comment for each appended `main` rule, keyed by its index there.
    main_comments: BTreeMap<usize, String>,
}

/// Rules appended to one caller, and the capabilities that caller already
/// had rules for, which the command kept instead of appending behind them.
#[derive(Debug, Default)]
struct RuleChanges {
    appended: Vec<PermissionRule>,
    kept: Vec<String>,
}

/// Lists the package and appends its `main` and caller rules.
fn add_named_package(
    blueprint: &mut Blueprint,
    artifact: &Artifact,
    selection: &Selection,
) -> Result<PackageAddition> {
    let package = &artifact.metadata.package_name;
    let capabilities = &artifact.capabilities;
    let default_allows = blueprint.default_action.unwrap_or_default() == DefaultAction::Allow;

    let main_decides = |name: &str| {
        has_unfiltered_capability_rule(blueprint, interpreter::mangle::USER_PACKAGE, name)
    };
    let main = selected_provided_rules(capabilities, selection, default_allows, main_decides);
    let main_has_rule =
        |name: &str| has_capability_rule(blueprint, interpreter::mangle::USER_PACKAGE, name);
    let left_to_default = count_left_to_default(capabilities, &main.appended, main_has_rule);
    let package_has_rule = |name: &str| has_capability_rule(blueprint, package, name);
    let package_rules = required_rules(capabilities, package_has_rule)?;

    blueprint.packages.insert(package.clone());
    let mut main_comments = BTreeMap::new();
    if !main.appended.is_empty() {
        let main_rules = blueprint
            .permissions
            .entry(interpreter::mangle::USER_PACKAGE.to_string())
            .or_default();
        record_main_comments(
            package,
            &capabilities.provides,
            &main.appended,
            main_rules.len(),
            &mut main_comments,
        );
        main_rules.extend(main.appended.iter().cloned());
    }
    blueprint
        .permissions
        .entry(package.clone())
        .or_default()
        .extend(package_rules.appended.iter().cloned());

    Ok(PackageAddition {
        main,
        caller: package_rules,
        left_to_default,
        main_comments,
    })
}

/// Describes each rule appended to `main` from `first_index` on, keyed by its
/// index there, so only rules this command wrote are annotated.
fn record_main_comments(
    package: &str,
    provides: &[ProvidedCapability],
    appended: &[PermissionRule],
    first_index: usize,
    main_comments: &mut BTreeMap<usize, String>,
) {
    for (offset, rule) in appended.iter().enumerate() {
        let header = (offset == 0).then(|| format!("    # {package} provides:\n"));
        let provided = provides
            .iter()
            .find(|provided| provided.name == rule.capability);
        let comment = header.into_iter().chain(provided.map(provided_comment));
        main_comments.insert(first_index + offset, comment.collect());
    }
}

/// Writes a caller list from `requires` for each dependency in `closure`
/// (dependencies first, the named package last) that the blueprint doesn't
/// declare, and returns what it reports. An existing list is the operator's
/// and is kept; it is reported only when it doesn't cover what the dependency
/// requires. The missing secrets of each dependency reported go to `warnings`.
fn add_dependency_callers(
    blueprint: &mut Blueprint,
    closure: &[Artifact],
    warnings: &mut Vec<String>,
) -> Result<Vec<String>> {
    let mut summaries = Vec::new();
    let Some((_, dependencies)) = closure.split_last() else {
        return Ok(summaries);
    };
    // Walked backwards, every dependency follows a package that depends on it.
    for (index, dependency) in dependencies.iter().enumerate().rev() {
        let name = &dependency.metadata.package_name;
        if blueprint.packages.contains(name) {
            continue;
        }
        let dependent = first_dependent(&closure[index + 1..], name);
        let reported = if blueprint.permissions.contains_key(name) {
            kept_caller_list_summary(blueprint, dependency, dependent)
        } else {
            add_caller_list(blueprint, dependency, dependent)?
        };
        let Some(reported) = reported else {
            continue;
        };
        warnings.extend(missing_package_secret_warnings(
            blueprint,
            name,
            &dependency.capabilities,
        ));
        summaries.push(reported);
    }
    Ok(summaries)
}

/// `None` when the existing list covers every requirement as lint checks it.
fn kept_caller_list_summary(
    blueprint: &Blueprint,
    dependency: &Artifact,
    dependent: &str,
) -> Option<String> {
    let name = &dependency.metadata.package_name;
    let uncovered = uncovered_requirements(blueprint, name, &dependency.capabilities);
    if uncovered.is_empty() {
        return None;
    }
    let mut message =
        format!("kept the existing caller rules for {name}, a dependency of {dependent}");
    push_kept(
        &mut message,
        &uncovered,
        &format!(
            "they don't cover {} capabilities it requires; `submilli blueprint lint` reports them:",
            uncovered.len()
        ),
    );
    Some(message)
}

/// `None` when the dependency requires nothing, so needs no caller list.
fn add_caller_list(
    blueprint: &mut Blueprint,
    dependency: &Artifact,
    dependent: &str,
) -> Result<Option<String>> {
    let name = &dependency.metadata.package_name;
    // No caller list yet, so nothing to keep.
    let rules = required_rules(&dependency.capabilities, |_| false)?;
    if rules.appended.is_empty() {
        return Ok(None);
    }
    blueprint
        .permissions
        .insert(name.clone(), rules.appended.clone());
    let mut message = format!("added caller rules for {name}, a dependency of {dependent}");
    push_caller_rules(&mut message, name, &rules);
    Ok(Some(message))
}

/// Capabilities `package` requires that its caller list doesn't grant for
/// every call, each once: what `blueprint lint` reports for it.
fn uncovered_requirements(
    blueprint: &Blueprint,
    package: &str,
    capabilities: &CapabilitySchema,
) -> Vec<String> {
    let uncovered: BTreeSet<&str> = capabilities
        .requires
        .iter()
        .filter(|required| {
            !has_matching_rule(
                blueprint,
                package,
                &required.capability,
                required.filter.as_deref(),
            )
        })
        .map(|required| required.capability.as_str())
        .collect();
    uncovered.into_iter().map(str::to_string).collect()
}

/// The first of `dependents` that declares `name` as a dependency. Every
/// dependency in a closure has one, so the fallback names no package.
fn first_dependent<'a>(dependents: &'a [Artifact], name: &str) -> &'a str {
    dependents
        .iter()
        .find(|artifact| declared_packages::depends_on(artifact, name))
        .map_or("another package", |artifact| {
            artifact.metadata.package_name.as_str()
        })
}

#[derive(Debug)]
enum Selection {
    All,
    None,
    Some(Vec<String>),
}

fn resolve_selection(args: &Args, provides: &[ProvidedCapability]) -> Result<Selection> {
    if args.all_capabilities {
        return Ok(Selection::All);
    }
    if args.no_capabilities {
        return Ok(Selection::None);
    }
    if !args.capabilities.is_empty() {
        let known: Vec<&str> = provides.iter().map(|p| p.name.as_str()).collect();
        for name in &args.capabilities {
            if !known.contains(&name.as_str()) {
                bail!(
                    "the package does not provide capability '{name}'; it provides: {}",
                    known.join(", ")
                );
            }
        }
        return Ok(Selection::Some(args.capabilities.clone()));
    }
    if provides.is_empty() {
        return Ok(Selection::None);
    }
    if std::io::stdin().is_terminal() {
        prompt_selection(provides)
    } else {
        eprintln!(
            "note: no provided capabilities selected (non-interactive); grant later with \
             `submilli blueprint capability add`"
        );
        Ok(Selection::None)
    }
}

fn prompt_selection(provides: &[ProvidedCapability]) -> Result<Selection> {
    let items: Vec<String> = provides
        .iter()
        .map(|p| match &p.description {
            Some(description) => format!("{} — {description}", p.name),
            None => p.name.clone(),
        })
        .collect();
    let picked = dialoguer::MultiSelect::new()
        .with_prompt(
            "Select provided capabilities to allow for `main` (space toggles, enter confirms)",
        )
        .items(&items)
        .interact()
        .context("capability selection prompt")?;
    Ok(Selection::Some(
        picked
            .into_iter()
            .map(|i| provides[i].name.clone())
            .collect(),
    ))
}

fn reject_versioned_spec(package: &str) -> Result<()> {
    let Some((name, _)) = package.rsplit_once('@') else {
        return Ok(());
    };
    if !name.is_empty() {
        bail!(
            "blueprints do not support versioned packages yet; pass only the package name, e.g. `{name}`"
        );
    }
    Ok(())
}

/// The `main` rules the selection produces: selected → `allow`. Unselected
/// capabilities normally need no rule (the `default:` covers them), but when
/// the blueprint's effective default is `allow`, leaving them ruleless would
/// silently grant them — so they get explicit `deny` rules instead.
///
/// A capability `main` already has an unfiltered rule for keeps that rule,
/// selected or not: a rule appended behind it would never match. A filtered
/// rule leaves the calls it doesn't match to what follows, so those still get
/// one.
fn selected_provided_rules(
    capabilities: &CapabilitySchema,
    selection: &Selection,
    default_allows: bool,
    main_decides: impl Fn(&str) -> bool,
) -> RuleChanges {
    let mut changes = RuleChanges::default();
    for provided in &capabilities.provides {
        let selected = match selection {
            Selection::All => true,
            Selection::None => false,
            Selection::Some(names) => names.iter().any(|n| n == &provided.name),
        };
        if !selected && !default_allows {
            continue;
        }
        if main_decides(&provided.name) {
            if selected {
                changes.kept.push(provided.name.clone());
            }
            continue;
        }
        changes.appended.push(PermissionRule {
            name: None,
            capability: provided.name.clone(),
            filter: None,
            action: if selected {
                Action::Allow
            } else {
                Action::Deny
            },
        });
    }
    changes
}

/// Provided capabilities `main` has no rule for, neither one this command
/// adds nor an existing one: every call to them falls through to the
/// blueprint's `default:`. Matches what `capability list --unconfigured`
/// lists; a capability with only filtered rules is not claimed for the
/// default, since its matching calls are decided by those rules.
fn count_left_to_default(
    capabilities: &CapabilitySchema,
    main_rules: &[PermissionRule],
    main_has_rule: impl Fn(&str) -> bool,
) -> usize {
    capabilities
        .provides
        .iter()
        .filter(|provided| {
            !main_has_rule(&provided.name)
                && !main_rules
                    .iter()
                    .any(|rule| rule.capability == provided.name)
        })
        .count()
}

/// The caller rules a package's `requires` produces. A required capability
/// the package's caller list already rules on, with any filter or action, is
/// the operator granting it differently on purpose; appending the broad rule
/// behind it would match whatever that rule leaves out, so it is kept.
fn required_rules(
    capabilities: &CapabilitySchema,
    package_has_rule: impl Fn(&str) -> bool,
) -> Result<RuleChanges> {
    let mut changes = RuleChanges::default();
    for required in &capabilities.requires {
        if package_has_rule(&required.capability) {
            if !changes.kept.contains(&required.capability) {
                changes.kept.push(required.capability.clone());
            }
            continue;
        }
        // An unfiltered requirement already granted decides every call to the
        // capability; a narrower rule behind it would never match.
        let granted_whole = changes
            .appended
            .iter()
            .any(|rule| rule.capability == required.capability && rule.filter.is_none());
        if granted_whole {
            continue;
        }
        let filter = required
            .filter
            .as_deref()
            .map(|raw| raw.parse::<FilterExpr>().map_err(anyhow::Error::msg))
            .transpose()
            .with_context(|| {
                format!(
                    "package capability schema emitted an invalid filter for `{}`",
                    required.capability
                )
            })?;
        changes.appended.push(PermissionRule {
            name: None,
            capability: required.capability.clone(),
            filter,
            action: Action::Allow,
        });
    }
    Ok(changes)
}

/// Puts each comment above the `main` rule at its key's index. Rules start
/// with their `- capability:` line, in the order `to_yaml` writes them.
fn annotate_main_rules(yaml: &str, comments: &BTreeMap<usize, String>) -> String {
    let mut out = String::new();
    let mut in_main = false;
    let mut rule_index = 0;

    for line in yaml.lines() {
        let trimmed = line.trim_start();
        if line == "  main:" {
            in_main = true;
        } else if (in_main
            && line.starts_with("  ")
            && !line.starts_with("    ")
            && trimmed.ends_with(':'))
            || !line.starts_with(' ')
        {
            in_main = false;
        }

        if in_main && trimmed.starts_with("- capability:") {
            if let Some(comment) = comments.get(&rule_index) {
                out.push_str(comment);
            }
            rule_index += 1;
        }

        out.push_str(line);
        out.push('\n');
    }

    out
}

fn provided_comment(provided: &ProvidedCapability) -> String {
    let description = provided
        .description
        .as_deref()
        .map_or(String::new(), |description| format!(" - {description}"));
    let mut comment = format!("    # {}{}\n", provided.name, description);
    if !provided.fields.is_empty() {
        let fields = provided
            .fields
            .iter()
            .map(|(name, field)| format!("{name}: {}", field.ty))
            .collect::<Vec<_>>()
            .join(", ");
        comment.push_str(&format!("    #   {{ {fields} }}\n"));
    }
    comment
}

fn summary(
    package: &str,
    path: &Path,
    addition: &PackageAddition,
    default: Option<DefaultAction>,
) -> String {
    let mut message = format!("added {package} to {}", path.display());
    let main = &addition.main;
    if !main.appended.is_empty() {
        message.push_str(&format!(
            "\n  added {} rules to caller `main`:",
            main.appended.len()
        ));
        for rule in &main.appended {
            message.push_str(&format!(
                "\n    {} {}",
                action_label(rule.action),
                rule.capability
            ));
        }
    }
    push_kept(
        &mut message,
        &main.kept,
        &format!(
            "`main` already has rules for {} selected capabilities; kept them, since the first matching rule wins:",
            main.kept.len()
        ),
    );
    if addition.left_to_default > 0 {
        message.push_str(&format!(
            "\n  {} provided capabilities not selected; {}",
            addition.left_to_default,
            unruled_call_outcome(default)
        ));
    }
    push_caller_rules(&mut message, package, &addition.caller);
    message
}

fn push_caller_rules(message: &mut String, package: &str, rules: &RuleChanges) {
    if rules.appended.is_empty() && !rules.kept.is_empty() {
        push_kept(
            message,
            &rules.kept,
            &format!("`{package}` already has rules for every capability it requires; kept them:"),
        );
        return;
    }
    message.push_str(&format!(
        "\n  added {} rules to caller `{package}` (default allow):",
        rules.appended.len()
    ));
    for rule in &rules.appended {
        message.push_str(&format!("\n    allow {}", rule.capability));
        if let Some(filter) = &rule.filter {
            message.push_str(&format!(" (filter: {filter})"));
        }
    }
    push_kept(
        message,
        &rules.kept,
        &format!(
            "`{package}` already has rules for {} required capabilities; kept them:",
            rules.kept.len()
        ),
    );
}

fn push_kept(message: &mut String, kept: &[String], heading: &str) {
    if kept.is_empty() {
        return;
    }
    message.push_str(&format!("\n  {heading}"));
    for capability in kept {
        message.push_str(&format!("\n    {capability}"));
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use submilli_build::{ProvidedCapability, ProvidedField, RequiredCapability};

    use super::*;

    /// A package that calls a capability both with a literal and with a
    /// computed argument requires it unfiltered and filtered; the filtered rule
    /// behind the unfiltered one would never match.
    #[test]
    fn required_rules_skip_what_an_unfiltered_rule_already_grants() {
        let schema = CapabilitySchema {
            requires: vec![
                RequiredCapability {
                    capability: "secrets.get".to_string(),
                    filter: None,
                },
                RequiredCapability {
                    capability: "secrets.get".to_string(),
                    filter: Some("name == \"A_KEY\"".to_string()),
                },
            ],
            ..CapabilitySchema::default()
        };

        let rules = required_rules(&schema, |_| false).expect("filters parse");

        assert_eq!(rules.appended.len(), 1);
        assert!(rules.appended[0].filter.is_none());
    }

    #[test]
    fn rejects_versioned_package_specs() {
        let err = reject_versioned_spec("@acme/sdk@1.2.3").expect_err("version rejected");

        assert!(
            err.to_string()
                .contains("do not support versioned packages")
        );
    }

    #[test]
    fn accepts_unversioned_scoped_package_names() {
        reject_versioned_spec("@acme/sdk").expect("package name accepted");
    }

    fn provided(name: &str) -> ProvidedCapability {
        ProvidedCapability {
            name: name.to_string(),
            description: None,
            fields: BTreeMap::new(),
        }
    }

    fn schema(provides: Vec<ProvidedCapability>) -> CapabilitySchema {
        CapabilitySchema {
            namespace: "acme.com".to_string(),
            provides,
            requires: Vec::new(),
        }
    }

    #[test]
    fn selection_maps_to_rules() {
        let schema = schema(vec![
            provided("acme.com/charge"),
            provided("acme.com/refund"),
        ]);

        let all = selected_provided_rules(&schema, &Selection::All, false, |_| false).appended;
        assert_eq!(all.len(), 2);
        assert!(all.iter().all(|r| r.action == Action::Allow));

        let none = selected_provided_rules(&schema, &Selection::None, false, |_| false);
        assert!(none.appended.is_empty() && none.kept.is_empty());

        let some = selected_provided_rules(
            &schema,
            &Selection::Some(vec!["acme.com/charge".to_string()]),
            false,
            |_| false,
        )
        .appended;
        assert_eq!(some.len(), 1);
        assert_eq!(some[0].capability, "acme.com/charge");
        assert_eq!(some[0].action, Action::Allow);
    }

    #[test]
    fn unselected_get_explicit_deny_under_default_allow() {
        let schema = schema(vec![
            provided("acme.com/charge"),
            provided("acme.com/refund"),
        ]);
        let rules = selected_provided_rules(
            &schema,
            &Selection::Some(vec!["acme.com/charge".to_string()]),
            true,
            |_| false,
        )
        .appended;
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0].action, Action::Allow);
        assert_eq!(rules[1].capability, "acme.com/refund");
        assert_eq!(rules[1].action, Action::Deny);
    }

    #[test]
    fn unselected_with_an_existing_main_rule_get_no_shadowed_deny() {
        let schema = schema(vec![
            provided("acme.com/charge"),
            provided("acme.com/refund"),
        ]);
        let rules = selected_provided_rules(&schema, &Selection::None, true, |name| {
            name == "acme.com/refund"
        });
        assert!(rules.kept.is_empty(), "unselected rules are not reported");
        let rules = rules.appended;
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].capability, "acme.com/charge");
        assert_eq!(rules[0].action, Action::Deny);
    }

    /// A filtered rule decides only the calls it matches; the rest would reach
    /// `default: allow` without the appended `deny`.
    #[test]
    fn unselected_with_only_a_filtered_main_rule_still_get_a_deny() {
        let blueprint: submilli_blueprint::Blueprint =
            submilli_blueprint::parse(
                "name: x\ndefault: allow\npermissions:\n  main:\n    - capability: acme.com/refund\n      filter: customer == \"vip\"\n      action: deny\n",
            )
            .expect("parses");
        let schema = schema(vec![provided("acme.com/refund")]);
        let main_decides = |name: &str| {
            has_unfiltered_capability_rule(&blueprint, interpreter::mangle::USER_PACKAGE, name)
        };

        let rules = selected_provided_rules(&schema, &Selection::None, true, main_decides).appended;

        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].capability, "acme.com/refund");
        assert_eq!(rules[0].action, Action::Deny);
    }

    /// SUB-1228: an `allow` appended behind an existing unfiltered rule never
    /// matches, so the selected capability keeps the operator's rule.
    #[test]
    fn selected_with_an_existing_main_rule_keeps_it() {
        let schema = schema(vec![
            provided("acme.com/charge"),
            provided("acme.com/refund"),
        ]);
        for default_allows in [false, true] {
            let rules = selected_provided_rules(&schema, &Selection::All, default_allows, |name| {
                name == "acme.com/charge"
            });

            assert_eq!(rules.kept, vec!["acme.com/charge".to_string()]);
            assert_eq!(rules.appended.len(), 1);
            assert_eq!(rules.appended[0].capability, "acme.com/refund");
            assert_eq!(rules.appended[0].action, Action::Allow);
        }
    }

    #[test]
    fn required_rules_keep_a_callers_existing_rules() {
        let schema = CapabilitySchema {
            requires: vec![
                RequiredCapability {
                    capability: "http.get".to_string(),
                    filter: Some("host == \"a.com\"".to_string()),
                },
                RequiredCapability {
                    capability: "http.get".to_string(),
                    filter: Some("host == \"b.com\"".to_string()),
                },
                RequiredCapability {
                    capability: "secrets.get".to_string(),
                    filter: None,
                },
            ],
            ..CapabilitySchema::default()
        };

        let rules = required_rules(&schema, |name| name == "http.get").expect("filters parse");

        assert_eq!(rules.kept, vec!["http.get".to_string()]);
        assert_eq!(rules.appended.len(), 1);
        assert_eq!(rules.appended[0].capability, "secrets.get");
    }

    #[test]
    fn capabilities_flag_rejects_unknown_names() {
        let args = Args {
            package: "@acme/sdk".to_string(),
            capabilities: vec!["acme.com/nope".to_string()],
            all_capabilities: false,
            no_capabilities: false,
            blueprint: None,
        };
        let err = resolve_selection(&args, &[provided("acme.com/charge")]).unwrap_err();
        assert!(err.to_string().contains("does not provide"), "{err}");
        assert!(err.to_string().contains("acme.com/charge"), "{err}");
    }

    #[test]
    fn annotates_main_rules_by_index() {
        let yaml = "name: x\npermissions:\n  '@acme/sdk':\n  - capability: acme.com/charge\n    action: allow\n  main:\n  - capability: acme.com/charge\n    action: deny\n  - capability: acme.com/charge\n    action: allow\n";
        let comments = BTreeMap::from([(1, "    # added\n".to_string())]);

        let annotated = annotate_main_rules(yaml, &comments);

        assert!(
            annotated.contains("    action: deny\n    # added\n  - capability: acme.com/charge\n    action: allow\n"),
            "{annotated}"
        );
        assert_eq!(annotated.matches("# added").count(), 1, "{annotated}");
        submilli_blueprint::parse(&annotated).expect("comments keep yaml parseable");
    }

    #[test]
    fn provided_comment_lists_description_and_fields() {
        let provided = ProvidedCapability {
            name: "acme.com/charge".to_string(),
            description: Some("Charge a customer.".to_string()),
            fields: BTreeMap::from([(
                "customer".to_string(),
                ProvidedField {
                    ty: "string".to_string(),
                    description: None,
                },
            )]),
        };

        assert_eq!(
            provided_comment(&provided),
            "    # acme.com/charge - Charge a customer.\n    #   { customer: string }\n"
        );
    }
}
