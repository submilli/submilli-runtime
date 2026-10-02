//! `submilli blueprint init [<name>]` — scaffold a `blueprint.yaml` in the
//! current directory (or `--blueprint <path>`). Runs offline; no server needed.
//!
//! The default scaffold is minimal: deny-by-default with an empty `main` rule
//! list and pointers to `submilli blueprint capability list`/`add`. `--full`
//! instead lists every stdlib capability the runtime can gate (from
//! [`interpreter::stdlib::capabilities`]) as an unfiltered deny rule with a
//! commented `filter:` example — the exhaustive template. Either way, `submilli server
//! blueprint add ./blueprint.yaml` applies the result to a server.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result};
use interpreter::stdlib::capabilities;

const DEFAULT_FILE: &str = "blueprint.yaml";

#[derive(clap::Args)]
pub struct Args {
    /// Blueprint name (the `name:` field). Defaults to the target directory name.
    name: Option<String>,
    /// Path to write (default: ./blueprint.yaml).
    #[arg(long)]
    blueprint: Option<PathBuf>,
    /// Scaffold every stdlib capability as a deny rule with a commented
    /// filter example, instead of the minimal empty policy.
    #[arg(long)]
    full: bool,
}

pub fn execute(args: Args) -> Result<ExitCode> {
    let path = args
        .blueprint
        .unwrap_or_else(|| PathBuf::from(DEFAULT_FILE));
    let name = match args.name {
        Some(n) => n,
        None => default_name(&path),
    };
    if write_scaffold(&path, &name, args.full)? {
        println!("✓ created {} (name: {name})", path.display());
        Ok(ExitCode::SUCCESS)
    } else {
        eprintln!(
            "error: {} already exists (refusing to overwrite)",
            path.display()
        );
        Ok(ExitCode::from(1))
    }
}

/// Write the scaffold to `path`, returning `false` if the file already exists
/// (never clobbers). Validates the generated YAML parses before writing, so a
/// bad `name` fails loudly instead of producing an unusable file.
fn write_scaffold(path: &Path, name: &str, full: bool) -> Result<bool> {
    let yaml = scaffold(name, full);
    submilli_blueprint::parse(&yaml)
        .with_context(|| format!("refusing to write an invalid blueprint (name: {name})"))?;
    match OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(mut file) => {
            file.write_all(yaml.as_bytes())
                .with_context(|| format!("writing {}", path.display()))?;
            Ok(true)
        }
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
        Err(err) => Err(err).with_context(|| format!("creating {}", path.display())),
    }
}

/// The blueprint name for an omitted `<name>`: the directory that will contain
/// the file, sanitized to the `[A-Za-z0-9_-]` a blueprint name allows.
fn default_name(path: &Path) -> String {
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    let raw = dir
        .canonicalize()
        .ok()
        .as_deref()
        .and_then(Path::file_name)
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let cleaned = sanitize_name(&raw);
    if cleaned.is_empty() {
        "blueprint".to_string()
    } else {
        cleaned
    }
}

fn sanitize_name(raw: &str) -> String {
    raw.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

const HEADER: &str = "\
# Blueprint scaffold from `submilli blueprint init`.
#
# The capability policy is deny-by-default: a capability is blocked unless a
# rule under `main` (your script's caller id) allows it. Grant one with
# `action: allow` (or `ask-human`), and constrain it with a `filter:`
# expression over the call's context fields.
#
# `default:` is the fall-through for any capability no rule matches. It governs
# even when the `permissions:` block is removed: `default: deny` (the scaffold)
# denies everything not explicitly allowed; `default: allow` inverts that to
# allow-by-default (a blocklist), so only the rules you set to `deny` are blocked.
";

/// Commented `variables:` example. Session variables are caller-supplied,
/// operator-declared values bound once per session and matched in a filter as
/// `${vars.NAME}` to scope a capability to per-session data.
const VARIABLES_EXAMPLE: &str = "\
# Session variables: caller-supplied values bound once per session (REST request
# body, or the MCP `initialize` `_meta.variables` object) and matched in a filter
# as `${vars.NAME}` — e.g. `filter: userId == ${vars.tenant_id}` to scope a
# capability to the caller's own data. `required: true` rejects a session that
# omits the variable; `default:` supplies a fallback (the two are mutually
# exclusive). Declare one with `submilli blueprint variable add <name> --required`.
# variables:
#   tenant_id:
#     required: true
#   region:
#     default: \"us\"
";

const PERMISSIONS_MINIMAL: &str = "\
# Grant capabilities as you need them:
#   submilli blueprint capability list         # browse what the runtime can gate
#   submilli blueprint capability add <name>
# or re-run `submilli blueprint init --full` for an exhaustive commented scaffold.
permissions:
  main: []
";

fn scaffold(name: &str, full: bool) -> String {
    let mut out = String::new();
    out.push_str(HEADER);
    out.push_str(&format!("\nkind: blueprint\nname: {name}\n\n"));
    out.push_str(VARIABLES_EXAMPLE);
    out.push_str("\ndefault: deny\n\n");
    if full {
        out.push_str(&permissions_full());
    } else {
        out.push_str(PERMISSIONS_MINIMAL);
    }
    out
}

/// Every non-templated stdlib capability as a deny rule with its summary and a
/// commented `filter:` example, sectioned by module.
fn permissions_full() -> String {
    let mut out = String::from("permissions:\n  main:\n");
    let mut first_group = true;
    for group in capabilities::catalog() {
        // Templated capabilities (`mcp.<server>`) need a concrete declared server,
        // so they're added by `blueprint add-mcp` when a server is added — not
        // scaffolded here. A group of only templates is omitted entirely.
        let mut started = false;
        for cap in group.capabilities.iter().filter(|c| !c.is_template()) {
            if !started {
                if !first_group {
                    out.push('\n');
                }
                out.push_str(&format!("    # --- {} ---\n", group.module));
                started = true;
                first_group = false;
            }
            out.push_str(&format!("    # {}\n", cap.summary));
            // Annotate rather than omit. The capability exists and a package
            // can hold it; dropping the entry would teach an operator scanning
            // the scaffold that there is nothing here to grant.
            if let Some(reason) = cap.main_denial {
                out.push_str(&format!("    # {}: not available here —\n", cap.name));
                out.push_str(&wrapped_comment(reason, "    #   ", 78));
                continue;
            }
            out.push_str(&format!("    - capability: {}\n", cap.name));
            out.push_str("      action: deny\n");
            out.push_str(&format!(
                "      # filter: {}  # fields: {}\n",
                cap.example_filter,
                cap.field_names().collect::<Vec<_>>().join(", ")
            ));
        }
    }
    out
}

/// Word-wraps `text` into `prefix`-led comment lines no wider than `width`,
/// so a long catalog note stays readable in the generated file.
fn wrapped_comment(text: &str, prefix: &str, width: usize) -> String {
    // Width is measured in `char`s, not bytes: the catalog reasons contain
    // em dashes, and byte lengths would wrap those lines short of `width`.
    let columns = |s: &str| s.chars().count();
    let mut out = String::new();
    let mut line = String::from(prefix);
    for word in text.split_whitespace() {
        if columns(&line) > columns(prefix) && columns(&line) + 1 + columns(word) > width {
            out.push_str(&line);
            out.push('\n');
            line = String::from(prefix);
        }
        if columns(&line) > columns(prefix) {
            line.push(' ');
        }
        line.push_str(word);
    }
    out.push_str(&line);
    out.push('\n');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimal_scaffold_parses_with_an_empty_policy() {
        let yaml = scaffold("scaffold-test", false);
        let bp = submilli_blueprint::parse(&yaml).expect("scaffold must parse");
        assert_eq!(bp.name, "scaffold-test");
        assert!(bp.has_permission_policy());
        assert_eq!(
            bp.default_action,
            Some(submilli_blueprint::DefaultAction::Deny)
        );
        assert!(bp.permissions["main"].is_empty());
        assert!(
            yaml.contains("blueprint capability add"),
            "minimal scaffold must point at the capability verbs:\n{yaml}"
        );
    }

    #[test]
    fn full_scaffold_parses_as_a_blueprint() {
        let yaml = scaffold("scaffold-test", true);
        let bp = submilli_blueprint::parse(&yaml).expect("scaffold must parse");
        assert_eq!(bp.name, "scaffold-test");
        assert!(bp.has_permission_policy());
        let listed = capabilities::catalog()
            .iter()
            .flat_map(|g| g.capabilities)
            .filter(|c| !c.is_template() && c.grantable_to_main())
            .count();
        assert_eq!(bp.permissions["main"].len(), listed);
        // Templated capabilities (`mcp.<server>`) are not scaffolded — `add-mcp`
        // adds them when a server is declared.
        assert!(
            !yaml.contains("mcp"),
            "scaffold must not mention MCP capabilities:\n{yaml}"
        );
    }

    #[test]
    fn wrapped_comment_preserves_words_and_respects_the_column_budget() {
        let text =
            "secret values are never available to main-module code — pass the secret NAME instead";
        let wrapped = wrapped_comment(text, "  # ", 40);

        let lines: Vec<&str> = wrapped.lines().collect();
        let mut rejoined = Vec::new();
        for line in &lines {
            assert!(
                line.starts_with("  # "),
                "every line keeps the prefix: {line:?}"
            );
            assert!(
                line.chars().count() <= 40,
                "line exceeds the column budget: {line:?}",
            );
            rejoined.extend(line.trim_start_matches("  # ").split_whitespace());
        }
        assert_eq!(rejoined, text.split_whitespace().collect::<Vec<_>>());
    }

    /// Measuring width in bytes wraps early on a multi-byte character without
    /// ever exceeding the budget, so a ceiling assertion cannot catch it. This
    /// input is sized so the em dash's two extra bytes are exactly what decides
    /// the break: `# — aaaaaa bbbbb` is 16 columns (18 bytes), and `cc` fits in
    /// 19 columns but not in 21 bytes. Byte-width wrapping splits it in two.
    #[test]
    fn wrapped_comment_measures_width_in_columns_not_bytes() {
        let wrapped = wrapped_comment("— aaaaaa bbbbb cc", "# ", 20);

        assert_eq!(
            wrapped.lines().collect::<Vec<_>>(),
            ["# — aaaaaa bbbbb cc"],
            "an em dash must not cost two columns of budget",
        );
    }

    #[test]
    fn wrapped_comment_keeps_a_word_longer_than_the_budget_on_its_own_line() {
        let long = "a".repeat(60);
        let wrapped = wrapped_comment(&format!("short {long} tail"), "# ", 20);

        assert!(
            wrapped.lines().any(|l| l == format!("# {long}")),
            "an unbreakable word gets its own line:\n{wrapped}",
        );
        assert_eq!(
            wrapped
                .split_whitespace()
                .filter(|w| *w != "#")
                .collect::<Vec<_>>(),
            ["short", long.as_str(), "tail"],
        );
    }

    #[test]
    fn full_scaffold_names_a_capability_main_cannot_hold_without_scaffolding_a_rule() {
        let yaml = scaffold("scaffold-test", true);
        let bp = submilli_blueprint::parse(&yaml).expect("scaffold must parse");

        for cap in capabilities::catalog()
            .iter()
            .flat_map(|g| g.capabilities)
            .filter(|c| !c.grantable_to_main())
        {
            assert!(
                yaml.contains(cap.name),
                "{} must still be named so the package grant is discoverable:\n{yaml}",
                cap.name,
            );
            assert!(
                !bp.permissions["main"]
                    .iter()
                    .any(|r| r.capability == cap.name),
                "{} must not be scaffolded as a rule under main:\n{yaml}",
                cap.name,
            );
            // The reason is the operator's only explanation of why no rule is
            // here; wrapping must carry its words through, not just its presence.
            let reason = cap
                .main_denial
                .expect("a refused capability carries a reason");
            let scaffolded: Vec<&str> = yaml.split_whitespace().collect();
            for word in reason.split_whitespace() {
                assert!(
                    scaffolded.contains(&word),
                    "the wrapped reason dropped {word:?}:\n{yaml}",
                );
            }
        }
        // The owning group's header survives the omission of its only rule.
        assert!(
            yaml.contains("# --- submilli:secrets ---"),
            "the section header must not vanish with its rules:\n{yaml}"
        );
    }

    #[test]
    fn uncommenting_a_filter_still_parses() {
        // Stripping the leading `# ` from a `# filter:` line must yield valid
        // YAML — that's the whole point of the commented examples.
        let yaml = scaffold("filter-test", true).replace("      # filter:", "      filter:");
        submilli_blueprint::parse(&yaml).expect("uncommented filters must parse");
    }

    #[test]
    fn uncommenting_variables_block_parses() {
        // Strip the leading `# ` from the `variables:` example and confirm the
        // resulting block is valid YAML for a blueprint.
        let block = VARIABLES_EXAMPLE
            .split_once("# variables:")
            .map(|(_, rest)| format!("variables:{rest}"))
            .expect("example has a variables: block");
        let uncommented: Vec<String> = block
            .lines()
            .map(|l| {
                l.strip_prefix("# ")
                    .or_else(|| l.strip_prefix('#'))
                    .unwrap_or(l)
                    .to_string()
            })
            .collect();
        let yaml = format!("name: vars-test\n{}\n", uncommented.join("\n"));
        let bp = submilli_blueprint::parse(&yaml).expect("uncommented variables must parse");
        assert!(bp.variables.contains_key("tenant_id"));
        assert!(bp.variables.contains_key("region"));
    }

    #[test]
    fn every_example_filter_is_valid() {
        for group in capabilities::catalog() {
            for cap in group.capabilities {
                // A templated `mcp.<server>` capability is concretized against a
                // declared mcp server so the rule (and its filter) validate.
                let yaml = if cap.is_template() {
                    format!(
                        "name: f\nmcp:\n  linear:\n    url: https://x/mcp\npermissions:\n  main:\n    - capability: mcp.linear\n      filter: {}\n      action: allow\n",
                        cap.example_filter
                    )
                } else {
                    // A capability the runtime refuses to `main` still has a
                    // filter to validate — under the package caller that can
                    // actually hold the rule.
                    let caller = if cap.grantable_to_main() {
                        "main"
                    } else {
                        "\"@acme/pkg\""
                    };
                    format!(
                        "name: f\npermissions:\n  {caller}:\n    - capability: {}\n      filter: {}\n      action: allow\n",
                        cap.name, cap.example_filter
                    )
                };
                submilli_blueprint::parse(&yaml)
                    .unwrap_or_else(|e| panic!("bad example_filter for {}: {e}", cap.name));
            }
        }
    }

    #[test]
    fn writes_file_then_refuses_to_clobber() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("blueprint.yaml");

        assert!(
            write_scaffold(&path, "demo", false).unwrap(),
            "first write creates"
        );
        let written = std::fs::read_to_string(&path).unwrap();
        assert!(written.contains("name: demo"));

        // Second call must not overwrite and must report "existed".
        assert!(
            !write_scaffold(&path, "other", false).unwrap(),
            "second write refuses"
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), written);
    }

    #[test]
    fn default_name_from_containing_directory() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("prod-blueprint");
        std::fs::create_dir(&project).unwrap();
        assert_eq!(
            default_name(&project.join("blueprint.yaml")),
            "prod-blueprint"
        );
    }

    #[test]
    fn sanitize_name_table() {
        assert_eq!(sanitize_name("prod-blueprint"), "prod-blueprint");
        assert_eq!(sanitize_name("my blueprint!"), "my-blueprint");
        assert_eq!(sanitize_name("a.b.c"), "a-b-c");
        assert_eq!(sanitize_name("..."), "");
    }
}
