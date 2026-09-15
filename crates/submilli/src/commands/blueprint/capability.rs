//! `submilli blueprint capability {list,add,remove}` — browse every capability
//! a blueprint can gate (stdlib, declared packages, declared MCP servers) and
//! edit the `permissions:` block without hand-writing YAML.
//!
//! `add`/`remove` rewrite the file from the parsed form, so YAML comments are
//! not preserved.

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Result, bail};
use clap::Subcommand;
use interpreter::stdlib::capabilities;
use submilli_blueprint::{Action, Blueprint, DefaultAction, FilterExpr, PermissionRule};
use submilli_build::PackageStore;

use super::file::{blueprint_path, load, write};

#[derive(Subcommand)]
pub enum CapabilityCmd {
    /// List the capabilities available to the blueprint, annotated with the
    /// permission rules already present. Offline — no server needed.
    List(ListArgs),
    /// Append a permission rule to the `permissions:` block. Rewrites the
    /// file; YAML comments are not preserved.
    Add(AddArgs),
    /// Remove every rule for a capability. Rewrites the file; YAML comments
    /// are not preserved.
    Remove(RemoveArgs),
}

pub fn execute(cmd: CapabilityCmd) -> Result<ExitCode> {
    let result = match cmd {
        CapabilityCmd::List(args) => list(&args),
        CapabilityCmd::Add(args) => add(&args),
        CapabilityCmd::Remove(args) => remove(&args),
    };
    match result {
        Ok(message) => {
            println!("{message}");
            Ok(ExitCode::SUCCESS)
        }
        Err(e) => {
            eprintln!("error: {e:#}");
            Ok(ExitCode::from(1))
        }
    }
}

#[derive(clap::Args)]
pub struct ListArgs {
    /// Show one library only: a stdlib module (`submilli:fs`), a declared
    /// package, or a declared MCP server name.
    library: Option<String>,
    /// Blueprint file to read (default: ./blueprint.yaml). Without a readable
    /// blueprint the stdlib catalog is still listed.
    #[arg(long)]
    blueprint: Option<PathBuf>,
}

#[derive(clap::Args)]
pub struct AddArgs {
    /// Capability name, e.g. `fs.read` or `mcp.linear`.
    capability: String,
    /// The rule's action. Adding a capability under `default: deny` grants it,
    /// so the default is `allow`.
    #[arg(long, value_enum, default_value_t = ActionArg::Allow)]
    action: ActionArg,
    /// Filter expression constraining the rule, e.g. 'path glob "*.csv"'.
    #[arg(long)]
    filter: Option<String>,
    /// Caller id the rule applies to (`main` is the user script; a package
    /// name gates that package's own calls).
    #[arg(long, default_value = "main")]
    caller: String,
    /// Add the rule even if the capability name isn't known to the stdlib
    /// catalog, a declared package, or a declared MCP server.
    #[arg(long)]
    force: bool,
    /// Blueprint file to edit (default: ./blueprint.yaml).
    #[arg(long)]
    blueprint: Option<PathBuf>,
}

#[derive(clap::Args)]
pub struct RemoveArgs {
    /// Capability name whose rules to remove.
    capability: String,
    /// Caller id to remove the rules from.
    #[arg(long, default_value = "main")]
    caller: String,
    /// Blueprint file to edit (default: ./blueprint.yaml).
    #[arg(long)]
    blueprint: Option<PathBuf>,
}

/// Local mirror of [`Action`]: the blueprint crate stays clap-free.
#[derive(Clone, Copy, clap::ValueEnum)]
enum ActionArg {
    Allow,
    Deny,
    AskHuman,
}

impl From<ActionArg> for Action {
    fn from(a: ActionArg) -> Self {
        match a {
            ActionArg::Allow => Action::Allow,
            ActionArg::Deny => Action::Deny,
            ActionArg::AskHuman => Action::AskHuman,
        }
    }
}

pub(super) fn action_label(action: Action) -> &'static str {
    match action {
        Action::Allow => "allow",
        Action::Deny => "deny",
        Action::AskHuman => "ask-human",
    }
}

/// One listed capability; `example` is `None` for package-provided entries
/// (their schema carries typed fields, not example filters).
struct Entry {
    name: String,
    summary: String,
    fields: String,
    example: Option<String>,
    /// Why the runtime refuses this capability to `main`, so listing a
    /// capability tells the operator it is package-only before they try to
    /// grant it and hit [`refuse_if_never_grantable_to_main`].
    main_denial: Option<&'static str>,
}

/// One library worth of entries: a stdlib module, a declared package, or a
/// declared MCP server.
struct Source {
    name: String,
    entries: Vec<Entry>,
}

fn list(args: &ListArgs) -> Result<String> {
    let path = blueprint_path(&args.blueprint);
    let blueprint = match load(&path) {
        Ok(bp) => Some(bp),
        Err(e) => {
            eprintln!("warning: {e:#} — listing the stdlib catalog only");
            None
        }
    };

    let sources = collect_sources(blueprint.as_ref());
    let sources = match &args.library {
        None => sources,
        Some(library) => {
            let names: Vec<String> = sources.iter().map(|s| s.name.clone()).collect();
            let filtered: Vec<Source> =
                sources.into_iter().filter(|s| s.name == *library).collect();
            if filtered.is_empty() {
                bail!(
                    "unknown library '{library}'; available: {}",
                    names.join(", ")
                );
            }
            filtered
        }
    };

    Ok(render(&sources, blueprint.as_ref()).trim_end().to_string())
}

fn collect_sources(blueprint: Option<&Blueprint>) -> Vec<Source> {
    let mut sources = Vec::new();
    for group in capabilities::catalog() {
        let entries: Vec<Entry> = group
            .capabilities
            .iter()
            .filter(|c| !c.is_template())
            .map(stdlib_entry)
            .collect();
        if !entries.is_empty() {
            sources.push(Source {
                name: group.module.to_string(),
                entries,
            });
        }
    }

    // The templated `mcp.<server>` capability concretizes per declared server;
    // with no blueprint (or no servers) the template itself is shown.
    if let Some(template) = capabilities::find("mcp.<server>") {
        let servers: Vec<&String> = blueprint
            .map(|b| b.mcp.keys().collect())
            .unwrap_or_default();
        if servers.is_empty() {
            let mut entry = stdlib_entry(template);
            entry
                .summary
                .push_str(" (declare a server with `blueprint add-mcp` to concretize)");
            sources.push(Source {
                name: "@mcp".to_string(),
                entries: vec![entry],
            });
        } else {
            for server in servers {
                let mut entry = stdlib_entry(template);
                entry.name = format!("mcp.{server}");
                sources.push(Source {
                    name: server.clone(),
                    entries: vec![entry],
                });
            }
        }
    }

    if let Some(bp) = blueprint {
        let store = PackageStore::default();
        for package in &bp.packages {
            match store.load(package) {
                Ok(artifact) => sources.push(Source {
                    name: package.clone(),
                    entries: artifact
                        .capabilities
                        .provides
                        .iter()
                        .map(provided_entry)
                        .collect(),
                }),
                Err(e) => eprintln!("warning: skipping declared package '{package}': {e}"),
            }
        }
    }

    sources
}

fn stdlib_entry(cap: &capabilities::Capability) -> Entry {
    Entry {
        name: cap.name.to_string(),
        summary: cap.summary.to_string(),
        fields: cap
            .filter_fields
            .iter()
            .map(|f| format!("{}: {}", f.name, f.ty))
            .collect::<Vec<_>>()
            .join(", "),
        example: Some(cap.example_filter.to_string()),
        main_denial: cap.main_denial,
    }
}

fn provided_entry(provided: &submilli_build::ProvidedCapability) -> Entry {
    Entry {
        name: provided.name.clone(),
        summary: provided.description.clone().unwrap_or_default(),
        fields: provided
            .fields
            .iter()
            .map(|(name, field)| format!("{name}: {}", field.ty))
            .collect::<Vec<_>>()
            .join(", "),
        example: None,
        main_denial: None,
    }
}

fn render(sources: &[Source], blueprint: Option<&Blueprint>) -> String {
    let mut out = String::new();
    if let Some(bp) = blueprint {
        out.push_str(&format!(
            "default: {} (rules below are first-match-wins)\n",
            action_label(bp.default_action.unwrap_or_default().into())
        ));
    }
    for source in sources {
        out.push_str(&format!("\n{}\n", source.name));
        for entry in &source.entries {
            out.push_str(&format!("  {} — {}\n", entry.name, entry.summary));
            if !entry.fields.is_empty() {
                out.push_str(&format!("      fields: {}\n", entry.fields));
            }
            if let Some(example) = &entry.example {
                out.push_str(&format!("      example filter: {example}\n"));
            }
            if let Some(reason) = entry.main_denial {
                out.push_str(&format!("      not available to main: {reason}\n"));
            }
            let Some(bp) = blueprint else { continue };
            for (caller, rules) in &bp.permissions {
                for rule in rules.iter().filter(|r| r.capability == entry.name) {
                    let filter = rule
                        .filter
                        .as_ref()
                        .map(|f| format!(" (filter: {f})"))
                        .unwrap_or_default();
                    out.push_str(&format!(
                        "      rule[{caller}]: {}{filter}\n",
                        action_label(rule.action)
                    ));
                }
            }
        }
    }
    out
}

fn add(args: &AddArgs) -> Result<String> {
    let path = blueprint_path(&args.blueprint);
    let mut blueprint = load(&path)?;

    let filter = args
        .filter
        .as_deref()
        .map(|raw| {
            raw.parse::<FilterExpr>()
                .map_err(|e| anyhow::anyhow!("invalid --filter expression:\n{e}"))
        })
        .transpose()?;

    refuse_if_never_grantable_to_main(&args.capability, &args.caller)?;
    if !args.force {
        validate_name(&blueprint, &args.capability)?;
    }

    let action = Action::from(args.action);
    let rule = PermissionRule {
        capability: args.capability.clone(),
        filter,
        action,
    };
    let had_policy = blueprint.has_permission_policy();
    let rules = blueprint
        .permissions
        .entry(args.caller.clone())
        .or_default();
    if rules.contains(&rule) {
        bail!(
            "an identical rule for '{}' already exists under caller '{}'",
            args.capability,
            args.caller
        );
    }
    let shadowed = rules.iter().any(|r| r.capability == args.capability);
    rules.push(rule.clone());
    if !had_policy {
        blueprint.default_action = Some(DefaultAction::Deny);
    }
    write(&path, &blueprint)?;

    let filter_text = rule
        .filter
        .as_ref()
        .map(|f| format!(" (filter: {f})"))
        .unwrap_or_default();
    let mut message = format!(
        "✓ added {} {}{filter_text} to caller '{}' in {}",
        action_label(action),
        args.capability,
        args.caller,
        path.display()
    );
    if let Some(cap) = capabilities::find(&args.capability) {
        message.push_str(&format!("\n  {}", cap.summary));
        if !cap.filter_fields.is_empty() {
            message.push_str(&format!(
                "\n  filter fields: {}",
                cap.filter_fields
                    .iter()
                    .map(|f| format!("{}: {}", f.name, f.ty))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
    }
    if shadowed {
        message.push_str(&format!(
            "\n  note: earlier rules for '{}' exist under '{}' — the first matching rule wins",
            args.capability, args.caller
        ));
    }
    Ok(message)
}

/// Refuses a rule the runtime would never consult. Checked ahead of, and
/// independently of, `--force`: that flag exists to bypass *unknown-capability*
/// validation, and letting it through here would write a dead rule and print
/// success.
fn refuse_if_never_grantable_to_main(capability: &str, caller: &str) -> Result<()> {
    if caller != interpreter::mangle::USER_PACKAGE {
        return Ok(());
    }
    let Some(reason) = capabilities::find(capability).and_then(|cap| cap.main_denial) else {
        return Ok(());
    };
    bail!(
        "'{capability}' cannot be granted to caller '{caller}': {reason}\n  \
         add it under the package instead: \
         `submilli blueprint capability add {capability} --caller <package>`"
    );
}

/// A capability name is accepted when the stdlib catalog, a declared package,
/// or a declared MCP server provides it. `--force` bypasses this — the policy
/// engine itself matches names verbatim and doesn't care.
fn validate_name(blueprint: &Blueprint, name: &str) -> Result<()> {
    let known = known_capabilities(blueprint);
    if known.iter().any(|k| k == name) {
        return Ok(());
    }
    let near: Vec<String> = suggestions(&known, name);
    let hint = if near.is_empty() {
        String::new()
    } else {
        format!("; did you mean: {}?", near.join(", "))
    };
    bail!(
        "unknown capability '{name}'{hint}\n  \
         `submilli blueprint capability list` shows every known capability; \
         pass --force to add the rule anyway"
    );
}

fn known_capabilities(blueprint: &Blueprint) -> Vec<String> {
    let mut names: Vec<String> = capabilities::catalog()
        .iter()
        .flat_map(|g| g.capabilities)
        .filter(|c| !c.is_template())
        .map(|c| c.name.to_string())
        .collect();
    names.extend(blueprint.mcp.keys().map(|server| format!("mcp.{server}")));
    let store = PackageStore::default();
    for package in &blueprint.packages {
        match store.load(package) {
            Ok(artifact) => names.extend(
                artifact
                    .capabilities
                    .provides
                    .iter()
                    .map(|p| p.name.clone()),
            ),
            Err(_) => eprintln!(
                "warning: could not load declared package '{package}' — its capabilities were not checked"
            ),
        }
    }
    names
}

/// Names sharing the input's `module.` prefix, or containing it as a
/// substring — enough to catch `fs.raed` and `http.download` typos.
fn suggestions(known: &[String], input: &str) -> Vec<String> {
    let prefix = input.split('.').next().unwrap_or(input);
    known
        .iter()
        .filter(|k| k.starts_with(&format!("{prefix}.")) || k.contains(input))
        .take(5)
        .cloned()
        .collect()
}

fn remove(args: &RemoveArgs) -> Result<String> {
    let path = blueprint_path(&args.blueprint);
    let mut blueprint = load(&path)?;
    let Some(rules) = blueprint.permissions.get_mut(&args.caller) else {
        bail!(
            "no rules for caller '{}' in {}",
            args.caller,
            path.display()
        );
    };
    let before = rules.len();
    rules.retain(|r| r.capability != args.capability);
    let removed = before - rules.len();
    if removed == 0 {
        bail!(
            "no rule for capability '{}' under caller '{}' in {}",
            args.capability,
            args.caller,
            path.display()
        );
    }
    if rules.is_empty() {
        blueprint.permissions.remove(&args.caller);
    }
    write(&path, &blueprint)?;
    Ok(format!(
        "✓ removed {removed} rule(s) for '{}' from caller '{}' in {}",
        args.capability,
        args.caller,
        path.display()
    ))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use super::*;

    fn temp_blueprint(body: &str) -> (tempfile::TempDir, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("blueprint.yaml");
        fs::write(&path, body).unwrap();
        (tmp, path)
    }

    fn reload(path: &Path) -> Blueprint {
        submilli_blueprint::parse(&fs::read_to_string(path).unwrap()).unwrap()
    }

    fn add_args(capability: &str, path: &Path) -> AddArgs {
        AddArgs {
            capability: capability.into(),
            action: ActionArg::Allow,
            filter: None,
            caller: "main".into(),
            force: false,
            blueprint: Some(path.to_path_buf()),
        }
    }

    #[test]
    fn add_then_list_round_trip() {
        let (_tmp, path) = temp_blueprint("name: t\n");
        let mut args = add_args("fs.read", &path);
        args.filter = Some("path glob \"*.csv\"".into());
        add(&args).unwrap();

        let bp = reload(&path);
        assert_eq!(bp.default_action, Some(DefaultAction::Deny));
        let rule = &bp.permissions["main"][0];
        assert_eq!(rule.capability, "fs.read");
        assert_eq!(rule.action, Action::Allow);
        assert_eq!(
            rule.filter.as_ref().unwrap().to_string(),
            "path glob \"*.csv\""
        );

        let out = list(&ListArgs {
            library: None,
            blueprint: Some(path.clone()),
        })
        .unwrap();
        assert!(out.contains("default: deny"), "{out}");
        assert!(
            out.contains("rule[main]: allow (filter: path glob \"*.csv\")"),
            "{out}"
        );
    }

    #[test]
    fn list_scopes_to_one_library() {
        let (_tmp, path) = temp_blueprint("name: t\n");
        let out = list(&ListArgs {
            library: Some("submilli:fs".into()),
            blueprint: Some(path.clone()),
        })
        .unwrap();
        assert!(out.contains("fs.read"), "{out}");
        assert!(!out.contains("http.get"), "{out}");

        let err = list(&ListArgs {
            library: Some("nope".into()),
            blueprint: Some(path),
        })
        .unwrap_err();
        assert!(err.to_string().contains("available:"), "{err}");
    }

    #[test]
    fn list_concretizes_mcp_servers() {
        let (_tmp, path) = temp_blueprint("name: t\nmcp:\n  linear:\n    url: https://x/mcp\n");
        let out = list(&ListArgs {
            library: Some("linear".into()),
            blueprint: Some(path),
        })
        .unwrap();
        assert!(out.contains("mcp.linear"), "{out}");
    }

    #[test]
    fn add_rejects_unknown_names_unless_forced() {
        let (_tmp, path) = temp_blueprint("name: t\n");
        let err = add(&add_args("fs.raed", &path)).unwrap_err();
        assert!(err.to_string().contains("unknown capability"), "{err}");
        assert!(err.to_string().contains("fs.read"), "{err}");

        let mut forced = add_args("acme.com/charge", &path);
        forced.force = true;
        add(&forced).unwrap();
        assert_eq!(
            reload(&path).permissions["main"][0].capability,
            "acme.com/charge"
        );
    }

    #[test]
    fn add_validates_mcp_only_when_declared() {
        let (_tmp, path) = temp_blueprint("name: t\n");
        let err = add(&add_args("mcp.linear", &path)).unwrap_err();
        assert!(err.to_string().contains("unknown capability"), "{err}");

        let (_tmp2, declared) =
            temp_blueprint("name: t\nmcp:\n  linear:\n    url: https://x/mcp\n");
        add(&add_args("mcp.linear", &declared)).unwrap();
        assert_eq!(
            reload(&declared).permissions["main"][0].capability,
            "mcp.linear"
        );
    }

    #[test]
    fn add_refuses_a_capability_main_can_never_hold() {
        let (_tmp, path) = temp_blueprint("name: t\n");

        let err = add(&add_args("secrets.get", &path)).unwrap_err();
        let message = err.to_string();
        assert!(
            message.contains("cannot be granted to caller 'main'"),
            "{message}"
        );
        assert!(message.contains("--caller <package>"), "{message}");
        // Not `!path.exists() || …`: that disjunct is vacuously satisfiable, so
        // a change that stopped writing the blueprint at all would keep this
        // green while proving nothing about the rule being withheld.
        assert!(
            path.exists(),
            "the refusal must leave the blueprint in place"
        );
        assert!(
            reload(&path).permissions.is_empty(),
            "no rule may be written for a capability main can never hold"
        );
    }

    /// `--force` bypasses *unknown-capability* validation. Letting it through
    /// here would write a rule the runtime never consults and print success.
    #[test]
    fn force_does_not_override_the_main_carve_out() {
        let (_tmp, path) = temp_blueprint("name: t\n");
        let mut forced = add_args("secrets.get", &path);
        forced.force = true;

        let err = add(&forced).unwrap_err();
        assert!(
            err.to_string()
                .contains("cannot be granted to caller 'main'"),
            "{err}"
        );
    }

    #[test]
    fn add_allows_the_same_capability_under_a_package_caller() {
        let (_tmp, path) = temp_blueprint("name: t\n");
        let mut package = add_args("secrets.get", &path);
        package.caller = "@acme/sdk".into();

        add(&package).unwrap();

        assert_eq!(
            reload(&path).permissions["@acme/sdk"][0].capability,
            "secrets.get"
        );
    }

    #[test]
    fn duplicate_rule_is_rejected_and_shadowing_is_noted() {
        let (_tmp, path) = temp_blueprint("name: t\n");
        add(&add_args("fs.read", &path)).unwrap();
        let err = add(&add_args("fs.read", &path)).unwrap_err();
        assert!(err.to_string().contains("identical rule"), "{err}");

        let mut deny = add_args("fs.read", &path);
        deny.action = ActionArg::Deny;
        let message = add(&deny).unwrap();
        assert!(message.contains("first matching rule wins"), "{message}");
        assert_eq!(reload(&path).permissions["main"].len(), 2);
    }

    #[test]
    fn remove_drops_all_matching_rules_and_empty_callers() {
        let (_tmp, path) = temp_blueprint("name: t\n");
        add(&add_args("fs.read", &path)).unwrap();
        let mut deny = add_args("fs.read", &path);
        deny.action = ActionArg::Deny;
        add(&deny).unwrap();

        let message = remove(&RemoveArgs {
            capability: "fs.read".into(),
            caller: "main".into(),
            blueprint: Some(path.clone()),
        })
        .unwrap();
        assert!(message.contains("removed 2 rule(s)"), "{message}");
        assert!(!reload(&path).permissions.contains_key("main"));
    }

    #[test]
    fn remove_without_a_match_is_an_error() {
        let (_tmp, path) = temp_blueprint("name: t\n");
        let err = remove(&RemoveArgs {
            capability: "fs.read".into(),
            caller: "main".into(),
            blueprint: Some(path),
        })
        .unwrap_err();
        assert!(err.to_string().contains("no rules for caller"), "{err}");
    }

    #[test]
    fn add_defaults_to_the_main_caller_and_allow() {
        let (_tmp, path) = temp_blueprint("name: t\n");
        add(&add_args("http.get", &path)).unwrap();
        let bp = reload(&path);
        assert_eq!(bp.permissions["main"][0].action, Action::Allow);
    }
}
