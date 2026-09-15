//! `submilli blueprint add-package <pkg>` — add an already-installed package
//! to a local `blueprint.yaml` and generate its starter permission rules.
//!
//! Which of the package's *provided* capabilities get `allow` rules under
//! `main` is a selection: `--capabilities` / `--all-capabilities` /
//! `--no-capabilities`, or an interactive multi-select on a TTY. Everything
//! unselected is covered by `default: deny` and needs no rule.

use std::fs;
use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use submilli_blueprint::{Action, DefaultAction, FilterExpr, PermissionRule};
use submilli_build::{CapabilitySchema, PackageStore, ProvidedCapability};

use super::capability::action_label;
use super::package_secrets::missing_package_secret_warnings;

const DEFAULT_FILE: &str = "blueprint.yaml";

#[derive(clap::Args)]
#[command(group = clap::ArgGroup::new("selection")
    .args(["capabilities", "all_capabilities", "no_capabilities"]))]
pub struct Args {
    /// Package name to add, e.g. `@stripe/sdk`.
    package: String,
    /// Provided capabilities to allow for `main`, comma-separated.
    #[arg(long, value_delimiter = ',', value_name = "NAME,...")]
    capabilities: Vec<String>,
    /// Allow every capability the package provides.
    #[arg(long)]
    all_capabilities: bool,
    /// Allow none of the provided capabilities (`default: deny` covers them).
    #[arg(long)]
    no_capabilities: bool,
    /// Blueprint file to edit (default: ./blueprint.yaml).
    #[arg(long)]
    blueprint: Option<PathBuf>,
}

pub fn execute(args: Args) -> Result<ExitCode> {
    match run(&args) {
        Ok(message) => {
            println!("✓ {message}");
            Ok(ExitCode::SUCCESS)
        }
        Err(e) => {
            eprintln!("error: {e:#}");
            Ok(ExitCode::from(1))
        }
    }
}

fn run(args: &Args) -> Result<String> {
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
            "blueprint '{}' already declares package '{}'",
            blueprint.name,
            args.package
        );
    }

    let artifact = PackageStore::default()
        .load(&args.package)
        .with_context(|| format!("loading package '{}'", args.package))?;
    let capabilities = artifact.capabilities;
    let warnings = missing_package_secret_warnings(&blueprint, &args.package, &capabilities);

    let selection = resolve_selection(args, &capabilities.provides)?;

    blueprint.packages.insert(args.package.clone());
    if !blueprint.has_permission_policy() {
        blueprint.default_action = Some(DefaultAction::Deny);
    }
    let default_allows = blueprint.default_action.unwrap_or_default() == DefaultAction::Allow;

    let main_rules = selected_provided_rules(&capabilities, &selection, default_allows);
    let package_rules = required_rules(&capabilities)?;

    if !main_rules.is_empty() {
        blueprint
            .permissions
            .entry("main".to_string())
            .or_default()
            .extend(main_rules.clone());
    }
    blueprint
        .permissions
        .entry(args.package.clone())
        .or_default()
        .extend(package_rules.clone());

    let updated = annotate_provided_rules(
        &submilli_blueprint::to_yaml(&blueprint),
        &args.package,
        &capabilities.provides,
    );
    submilli_blueprint::parse(&updated)
        .context("the resulting blueprint is invalid — not written")?;
    fs::write(&path, &updated).with_context(|| format!("writing {}", path.display()))?;
    for warning in warnings {
        eprintln!("warning: {}: {warning}", path.display());
    }

    let not_selected = capabilities.provides.len() - main_rules.len();
    Ok(summary(
        &args.package,
        &path,
        &main_rules,
        &package_rules,
        not_selected,
    ))
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
/// capabilities normally need no rule (`default: deny` covers them), but when
/// the blueprint's effective default is `allow`, leaving them ruleless would
/// silently grant them — so they get explicit `deny` rules instead.
fn selected_provided_rules(
    capabilities: &CapabilitySchema,
    selection: &Selection,
    default_allows: bool,
) -> Vec<PermissionRule> {
    capabilities
        .provides
        .iter()
        .filter_map(|provided| {
            let selected = match selection {
                Selection::All => true,
                Selection::None => false,
                Selection::Some(names) => names.iter().any(|n| n == &provided.name),
            };
            let action = if selected {
                Action::Allow
            } else if default_allows {
                Action::Deny
            } else {
                return None;
            };
            Some(PermissionRule {
                capability: provided.name.clone(),
                filter: None,
                action,
            })
        })
        .collect()
}

fn required_rules(capabilities: &CapabilitySchema) -> Result<Vec<PermissionRule>> {
    capabilities
        .requires
        .iter()
        .map(|required| {
            Ok(PermissionRule {
                capability: required.capability.clone(),
                filter: required
                    .filter
                    .as_deref()
                    .map(|raw| raw.parse::<FilterExpr>().map_err(anyhow::Error::msg))
                    .transpose()
                    .with_context(|| {
                        format!(
                            "package capability schema emitted an invalid filter for `{}`",
                            required.capability
                        )
                    })?,
                action: Action::Allow,
            })
        })
        .collect()
}

fn annotate_provided_rules(yaml: &str, package: &str, provides: &[ProvidedCapability]) -> String {
    if provides.is_empty() {
        return yaml.to_string();
    }

    let mut pending = provides.iter().collect::<Vec<_>>();
    let mut out = String::new();
    let mut in_main = false;
    let mut saw_package_header = false;

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
            let raw = trimmed.trim_start_matches("- capability:").trim();
            let capability = unquote_yaml_scalar(raw);
            if let Some(index) = pending
                .iter()
                .position(|provided| provided.name == capability)
            {
                let provided = pending.remove(index);
                if !saw_package_header {
                    out.push_str(&format!("    # {package} provides:\n"));
                    saw_package_header = true;
                }
                out.push_str(&provided_comment(provided));
            }
        }

        out.push_str(line);
        out.push('\n');
    }

    out
}

fn unquote_yaml_scalar(raw: &str) -> String {
    raw.trim_matches('"').trim_matches('\'').to_string()
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
    path: &std::path::Path,
    main_rules: &[PermissionRule],
    package_rules: &[PermissionRule],
    not_selected: usize,
) -> String {
    let mut message = format!("added {package} to {}", path.display());
    if !main_rules.is_empty() {
        message.push_str(&format!(
            "\n  added {} rules to caller `main`:",
            main_rules.len()
        ));
        for rule in main_rules {
            message.push_str(&format!(
                "\n    {} {}",
                action_label(rule.action),
                rule.capability
            ));
        }
    }
    if not_selected > 0 {
        message.push_str(&format!(
            "\n  {not_selected} provided capabilities not selected — denied by `default: deny`"
        ));
    }
    message.push_str(&format!(
        "\n  added {} rules to caller `{package}` (default allow):",
        package_rules.len()
    ));
    for rule in package_rules {
        message.push_str(&format!("\n    allow {}", rule.capability));
        if let Some(filter) = &rule.filter {
            message.push_str(&format!(" (filter: {filter})"));
        }
    }
    message
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use submilli_build::{ProvidedCapability, ProvidedField};

    use super::*;

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

        let all = selected_provided_rules(&schema, &Selection::All, false);
        assert_eq!(all.len(), 2);
        assert!(all.iter().all(|r| r.action == Action::Allow));

        assert!(selected_provided_rules(&schema, &Selection::None, false).is_empty());

        let some = selected_provided_rules(
            &schema,
            &Selection::Some(vec!["acme.com/charge".to_string()]),
            false,
        );
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
        );
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0].action, Action::Allow);
        assert_eq!(rules[1].capability, "acme.com/refund");
        assert_eq!(rules[1].action, Action::Deny);
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
    fn annotates_provided_rules() {
        let yaml = "name: x\npermissions:\n  main:\n    - capability: acme.com/charge\n      action: allow\n";
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

        let annotated = annotate_provided_rules(yaml, "@acme/sdk", &[provided]);

        assert!(annotated.contains("# @acme/sdk provides:"));
        assert!(annotated.contains("# acme.com/charge - Charge a customer."));
        assert!(annotated.contains("#   { customer: string }"));
        submilli_blueprint::parse(&annotated).expect("comments keep yaml parseable");
    }
}
