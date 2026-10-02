//! `submilli blueprint capability {list,add,remove}` — browse every capability
//! a blueprint can gate (stdlib, declared packages and the packages they
//! depend on, declared MCP servers) and edit the `permissions:` block without
//! hand-writing YAML.
//!
//! `add`/`remove` rewrite the file from the parsed form, so YAML comments are
//! not preserved.

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::Subcommand;
use interpreter::stdlib::capabilities;
use submilli_blueprint::{Action, Blueprint, DefaultAction, FilterExpr, PermissionRule};
use submilli_build::PackageStore;

use super::declared_packages;
use super::file::{blueprint_path, has_capability_rule, load, write};

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
    /// package or a package one depends on, or a declared MCP server name.
    library: Option<String>,
    /// Blueprint file to read (default: ./blueprint.yaml). Without a readable
    /// blueprint the stdlib catalog is still listed.
    #[arg(long)]
    blueprint: Option<PathBuf>,
    /// Show only the capabilities declared packages provide that have no rule
    /// under `main` (a filtered rule or a `deny` counts as a rule), and the
    /// default they fall through to. Requires a readable blueprint.
    #[arg(long)]
    unconfigured: bool,
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
    if args.unconfigured {
        return list_unconfigured(args);
    }
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

/// Omitting a provided capability from `main` is how a blueprint withholds
/// it, so this view lists rule presence, not what the script calls. An
/// explicit `deny` is a rule and is not listed.
fn list_unconfigured(args: &ListArgs) -> Result<String> {
    let path = blueprint_path(&args.blueprint);
    let blueprint = load(&path)?;
    let packages: Vec<&String> = match &args.library {
        None => blueprint.packages.iter().collect(),
        Some(library) if blueprint.packages.contains(library) => vec![library],
        Some(library) => bail!(
            "'{library}' is not a package {} declares; --unconfigured lists declared packages: {}",
            path.display(),
            declared_packages(&blueprint)
        ),
    };
    let sources = unconfigured_sources(&blueprint, &packages)?;

    let mut out = format!(
        "Provided capabilities with no rule under `main`; {}.\n\n",
        unruled_call_outcome(blueprint.default_action)
    );
    if blueprint.packages.is_empty() {
        out.push_str(&format!("none: {} declares no packages", path.display()));
    } else if sources.is_empty() {
        let none = match &args.library {
            Some(library) => {
                format!("none: `{library}` provides no capability without a rule under `main`")
            }
            None => "none: no declared package provides a capability without a rule under `main`"
                .to_string(),
        };
        out.push_str(&none);
    } else {
        // Rules other callers hold for these names are not `main`'s; showing
        // them under this heading would read as configuration.
        out.push_str(&render(&sources, None));
    }
    Ok(out.trim_end().to_string())
}

/// Unlike the full listing, a package that fails to load is an error:
/// skipping it would under-report what the blueprint leaves out.
fn unconfigured_sources(blueprint: &Blueprint, packages: &[&String]) -> Result<Vec<Source>> {
    let store = PackageStore::default();
    let mut sources = Vec::new();
    for package in packages {
        let artifact = store
            .load(package)
            .with_context(|| format!("loading declared package '{package}'"))?;
        let entries: Vec<Entry> = artifact
            .capabilities
            .provides
            .iter()
            .filter(|provided| {
                !has_capability_rule(blueprint, interpreter::mangle::USER_PACKAGE, &provided.name)
            })
            .map(provided_entry)
            .collect();
        if !entries.is_empty() {
            sources.push(Source {
                name: (*package).clone(),
                entries,
            });
        }
    }
    Ok(sources)
}

fn declared_packages(blueprint: &Blueprint) -> String {
    if blueprint.packages.is_empty() {
        return "none".to_string();
    }
    blueprint
        .packages
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(", ")
}

/// What a call with no matching rule resolves to. Enforcement falls through
/// to `deny` when `default:` is unset, even with no `permissions:` block.
pub(super) fn unruled_call_outcome(default: Option<DefaultAction>) -> &'static str {
    match default {
        None => "`default:` is unset, so calls to them are denied",
        Some(DefaultAction::Deny) => "`default: deny` denies calls to them",
        Some(DefaultAction::Allow) => "`default: allow` allows calls to them",
        Some(DefaultAction::AskHuman) => {
            "`default: ask-human` applies to them, which currently denies calls"
        }
    }
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

    // A dependency's provided capabilities are what its dependents require,
    // so their caller lists name them too.
    if let Some(bp) = blueprint {
        for (package, artifact) in load_packages(bp).artifacts {
            sources.push(Source {
                name: package,
                entries: artifact
                    .capabilities
                    .provides
                    .iter()
                    .map(provided_entry)
                    .collect(),
            });
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
    for source in sources {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&format!("{}\n", source.name));
        for entry in &source.entries {
            if entry.summary.is_empty() {
                out.push_str(&format!("  {}\n", entry.name));
            } else {
                out.push_str(&format!("  {} — {}\n", entry.name, entry.summary));
            }
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
        validate_name(&blueprint, &args.caller, &args.capability)?;
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
    // MCP scaffolding ends in an unconditional deny. Insert an explicit grant
    // before that fallback so `add-mcp` followed by `capability add` works,
    // while preserving earlier filtered rules and the fallback for other tools.
    let position = if args.capability.starts_with("mcp.") && action == Action::Allow {
        rules.iter().position(|r| {
            r.capability == args.capability && r.filter.is_none() && r.action == Action::Deny
        })
    } else {
        None
    }
    .unwrap_or(rules.len());
    let shadowed = rules[..position]
        .iter()
        .any(|r| r.capability == args.capability);
    let unreachable =
        unreachable_after_insert(rules, position, &rule, &args.caller, &remove_command(args));
    rules.insert(position, rule.clone());
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
    match unreachable {
        Some(warning) => message.push_str(&format!("\n  warning: {warning}")),
        None if shadowed => message.push_str(&format!(
            "\n  note: earlier rules for '{}' exist under '{}' — the first matching rule wins",
            args.capability, args.caller
        )),
        None => {}
    }
    Ok(message)
}

/// What inserting `rule` at index `position` leaves unable to match, if
/// anything: the first matching rule decides, so a rule after an unfiltered
/// rule for the same capability never matches. Rule numbers in the warning are
/// 1-based positions after the insert, as lint reports them.
fn unreachable_after_insert(
    rules: &[PermissionRule],
    position: usize,
    rule: &PermissionRule,
    caller: &str,
    remove_command: &str,
) -> Option<String> {
    let same_capability = |other: &PermissionRule| other.capability == rule.capability;
    let (before, after) = rules.split_at(position.min(rules.len()));
    let fix = format!(
        "delete or narrow it in the file, or run `{remove_command}` and add the rules again in \
         order"
    );
    if let Some((number, deciding)) = (1..)
        .zip(before)
        .find(|(_, other)| same_capability(other) && other.filter.is_none())
    {
        return Some(format!(
            "`permissions.{caller}` rule {number} for `{}` (`{}`, no filter) decides every call \
             first, so this rule never matches; {fix}",
            rule.capability,
            action_label(deciding.action)
        ));
    }
    if rule.filter.is_some() {
        return None;
    }
    // After the insert, the rules that followed `position` sit one further on.
    let later: Vec<String> = (before.len() + 2..)
        .zip(after)
        .filter(|(_, other)| same_capability(other))
        .map(|(number, _)| number.to_string())
        .collect();
    let (noun, verb) = if later.len() == 1 {
        ("rule", "matches")
    } else {
        ("rules", "match")
    };
    (!later.is_empty()).then(|| {
        format!(
            "this rule has no filter, so `permissions.{caller}` {noun} {} for `{}` after it never \
             {verb}; {fix}",
            later.join(", "),
            rule.capability
        )
    })
}

/// The `capability remove` invocation that edits the same file and caller as
/// this `capability add`.
fn remove_command(args: &AddArgs) -> String {
    let mut command = format!("submilli blueprint capability remove {}", args.capability);
    if args.caller != interpreter::mangle::USER_PACKAGE {
        command.push_str(&format!(" --caller {}", args.caller));
    }
    if let Some(path) = &args.blueprint {
        command.push_str(&format!(" --blueprint {}", path.display()));
    }
    command
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

/// A capability name is accepted when the stdlib catalog, a declared MCP
/// server, or a package `caller` can reach provides it: for `main`, a declared
/// package; for a package, also a package a declared one depends on. `--force`
/// bypasses this — the policy engine itself matches names verbatim and doesn't
/// care.
fn validate_name(blueprint: &Blueprint, caller: &str, name: &str) -> Result<()> {
    let packages = load_packages(blueprint);
    let known = known_capabilities(blueprint, &packages, caller);
    if known.iter().any(|k| k == name) {
        return Ok(());
    }
    if let Some(dependency) = dependency_providing(&packages, name) {
        let callers = dependents_of(&packages, dependency)
            .into_iter()
            .map(|dependent| format!("`--caller {dependent}`"))
            .collect::<Vec<_>>()
            .join(" or ");
        bail!(
            "'{name}' is provided by `{dependency}`, which only the packages that depend on it \
             can call; `{caller}` cannot import it\n  \
             grant it to one of them with {callers}, or declare `{dependency}` \
             with `submilli blueprint add-package {dependency}`"
        );
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

/// Every capability name `caller` may hold a rule for.
fn known_capabilities(
    blueprint: &Blueprint,
    packages: &declared_packages::DeclaredPackages,
    caller: &str,
) -> Vec<String> {
    let mut names: Vec<String> = capabilities::catalog()
        .iter()
        .flat_map(|g| g.capabilities)
        .filter(|c| !c.is_template())
        .map(|c| c.name.to_string())
        .collect();
    names.extend(blueprint.mcp.keys().map(|server| format!("mcp.{server}")));
    // `main` imports only declared packages, so a `main` rule for what only a
    // dependency provides could never match.
    let reaches_dependencies = caller != interpreter::mangle::USER_PACKAGE;
    for (package, artifact) in &packages.artifacts {
        if packages.dependencies.contains(package) && !reaches_dependencies {
            continue;
        }
        names.extend(
            artifact
                .capabilities
                .provides
                .iter()
                .map(|p| p.name.clone()),
        );
    }
    names
}

/// The undeclared dependency that provides `name`, if any.
fn dependency_providing<'a>(
    packages: &'a declared_packages::DeclaredPackages,
    name: &str,
) -> Option<&'a str> {
    packages
        .artifacts
        .iter()
        .filter(|(package, _)| packages.dependencies.contains(*package))
        .find(|(_, artifact)| {
            artifact
                .capabilities
                .provides
                .iter()
                .any(|provided| provided.name == name)
        })
        .map(|(package, _)| package.as_str())
}

/// The loaded packages that depend on `dependency` directly.
fn dependents_of<'a>(
    packages: &'a declared_packages::DeclaredPackages,
    dependency: &str,
) -> Vec<&'a str> {
    packages
        .artifacts
        .iter()
        .filter(|(_, artifact)| declared_packages::depends_on(artifact, dependency))
        .map(|(package, _)| package.as_str())
        .collect()
}

/// The checks this command makes are best-effort, so a package that fails to
/// load is a warning.
fn load_packages(blueprint: &Blueprint) -> declared_packages::DeclaredPackages {
    let packages = declared_packages::load(blueprint, &PackageStore::default());
    for error in &packages.errors {
        eprintln!("warning: {error}; listing and checking only what loaded");
    }
    packages
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
            unconfigured: false,
        })
        .unwrap();
        assert!(out.starts_with("submilli:fs\n"), "{out}");
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
            unconfigured: false,
        })
        .unwrap();
        assert!(out.contains("fs.read"), "{out}");
        assert!(!out.contains("http.get"), "{out}");

        let err = list(&ListArgs {
            library: Some("nope".into()),
            blueprint: Some(path),
            unconfigured: false,
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
            unconfigured: false,
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
    fn mcp_allow_is_inserted_before_the_scaffold_deny() {
        let (_tmp, path) = temp_blueprint(
            "name: t\nmcp:\n  x:\n    url: https://example.com/mcp\npermissions:\n  main:\n    - capability: mcp.x\n      action: deny\n",
        );
        let mut args = add_args("mcp.x", &path);
        args.filter = Some("tool == \"read\"".into());
        add(&args).unwrap();
        let blueprint = reload(&path);
        let rules = &blueprint.permissions["main"];
        assert_eq!(rules[0].action, Action::Allow);
        assert!(rules[0].filter.is_some());
        assert_eq!(rules[1].action, Action::Deny);
    }

    #[test]
    fn duplicate_rule_is_rejected_and_shadowing_is_noted() {
        let (_tmp, path) = temp_blueprint("name: t\n");
        let mut filtered = add_args("fs.read", &path);
        filtered.filter = Some("path == \"/x\"".into());
        add(&filtered).unwrap();
        let err = add(&filtered).unwrap_err();
        assert!(err.to_string().contains("identical rule"), "{err}");

        let mut deny = add_args("fs.read", &path);
        deny.action = ActionArg::Deny;
        let message = add(&deny).unwrap();
        assert!(message.contains("first matching rule wins"), "{message}");
        assert_eq!(reload(&path).permissions["main"].len(), 2);
    }

    /// SUB-1258: the rule is still written, but the output says it can never
    /// match, as `blueprint lint` will.
    #[test]
    fn a_rule_behind_an_unfiltered_rule_is_warned_about() {
        let (_tmp, path) = temp_blueprint("name: t\n");
        add(&add_args("fs.read", &path)).unwrap();

        let mut narrowed = add_args("fs.read", &path);
        narrowed.filter = Some("path == \"/x\"".into());
        narrowed.action = ActionArg::Deny;
        let message = add(&narrowed).unwrap();

        assert!(
            message.contains(
                "warning: `permissions.main` rule 1 for `fs.read` (`allow`, no filter) decides every call first, so this rule never matches"
            ),
            "{message}"
        );
        assert_eq!(reload(&path).permissions["main"].len(), 2);
    }

    /// The suggested `remove` edits the same caller and file as the `add`.
    #[test]
    fn the_unreachable_warning_names_the_callers_remove_command() {
        let (_tmp, path) = temp_blueprint("name: t\n");
        let mut whole = add_args("fs.read", &path);
        whole.caller = "@acme/x".into();
        add(&whole).unwrap();

        let mut narrowed = add_args("fs.read", &path);
        narrowed.caller = "@acme/x".into();
        narrowed.filter = Some("path == \"/x\"".into());
        let message = add(&narrowed).unwrap();

        assert!(
            message.contains(&format!(
                "`permissions.@acme/x` rule 1 for `fs.read` (`allow`, no filter) decides every call first, so this rule never matches; delete or narrow it in the file, or run `submilli blueprint capability remove fs.read --caller @acme/x --blueprint {}`",
                path.display()
            )),
            "{message}"
        );
    }

    /// The MCP grant goes ahead of the scaffolded `deny`, which it then shadows.
    #[test]
    fn an_unfiltered_mcp_grant_warns_about_the_rules_it_shadows() {
        let (_tmp, path) = temp_blueprint(
            "name: t\nmcp:\n  srv:\n    transport: streamable_http\n    url: https://example.invalid/mcp\npermissions:\n  main:\n    - capability: mcp.srv\n      action: deny\n",
        );

        let message = add(&add_args("mcp.srv", &path)).unwrap();

        assert!(
            message.contains(
                "warning: this rule has no filter, so `permissions.main` rule 2 for `mcp.srv` after it never matches"
            ),
            "{message}"
        );
        assert_eq!(
            reload(&path).permissions["main"][0].action,
            submilli_blueprint::Action::Allow
        );
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
