//! `submilli blueprint variable {add,list,remove}` — manage the session variables
//! declared in a local `blueprint.yaml`'s `variables:` block, so operators don't
//! hand-edit the YAML. Declaring is local authoring; the value is bound by the
//! caller when a session opens.

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Result, bail};
use clap::Subcommand;
use submilli_blueprint::{Blueprint, VariableDecl};

use super::file::{blueprint_path, load, write};

#[derive(Subcommand)]
pub enum VariableCmd {
    /// Declare a session variable in the `variables:` block.
    Add(AddArgs),
    /// List the blueprint's declared variables.
    List(ListArgs),
    /// Remove a declared variable. Refused while a permission filter still
    /// references it.
    Remove(RemoveArgs),
}

pub fn execute(cmd: VariableCmd) -> Result<ExitCode> {
    let result = match cmd {
        VariableCmd::Add(args) => add(&args),
        VariableCmd::List(args) => list(&args),
        VariableCmd::Remove(args) => remove(&args),
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
pub struct AddArgs {
    /// Variable name — what `${vars.<NAME>}` references in a filter.
    name: String,
    /// Reject a session that omits the variable or binds it to an empty string.
    #[arg(long, conflicts_with = "default")]
    required: bool,
    /// Value bound when the caller supplies none. A variable is always a string.
    #[arg(long, value_name = "VALUE")]
    default: Option<String>,
    /// Blueprint file to edit (default: ./blueprint.yaml).
    #[arg(long)]
    blueprint: Option<PathBuf>,
}

#[derive(clap::Args)]
pub struct ListArgs {
    /// Blueprint file to read (default: ./blueprint.yaml).
    #[arg(long)]
    blueprint: Option<PathBuf>,
}

#[derive(clap::Args)]
pub struct RemoveArgs {
    /// The variable to remove.
    name: String,
    /// Blueprint file to edit (default: ./blueprint.yaml).
    #[arg(long)]
    blueprint: Option<PathBuf>,
}

fn add(args: &AddArgs) -> Result<String> {
    let path = blueprint_path(&args.blueprint);
    let mut blueprint = load(&path)?;

    if blueprint.variables.contains_key(&args.name) {
        bail!(
            "blueprint '{}' already declares a variable '{}'",
            blueprint.name,
            args.name
        );
    }

    let decl = VariableDecl {
        required: args.required,
        default: args.default.clone(),
    };
    let summary = summary(&decl);
    blueprint.variables.insert(args.name.clone(), decl);
    write(&path, &blueprint)?;

    Ok(format!(
        "✓ declared variable '{}' ({summary}) in {}",
        args.name,
        path.display()
    ))
}

fn list(args: &ListArgs) -> Result<String> {
    let path = blueprint_path(&args.blueprint);
    let blueprint = load(&path)?;
    if blueprint.variables.is_empty() {
        return Ok(format!("no variables declared in {}", path.display()));
    }
    let lines: Vec<String> = blueprint
        .variables
        .iter()
        .map(|(name, decl)| {
            let used_by = referencing_rules(&blueprint, name);
            let usage = if used_by.is_empty() {
                "unused".to_string()
            } else {
                format!("used by {}", used_by.join(", "))
            };
            format!("{name}  ({}; {usage})", summary(decl))
        })
        .collect();
    Ok(lines.join("\n"))
}

fn remove(args: &RemoveArgs) -> Result<String> {
    let path = blueprint_path(&args.blueprint);
    let mut blueprint = load(&path)?;

    if !blueprint.variables.contains_key(&args.name) {
        bail!("no variable '{}' declared in {}", args.name, path.display());
    }
    let used_by = referencing_rules(&blueprint, &args.name);
    if !used_by.is_empty() {
        bail!(
            "variable '{}' is still referenced by {}; remove or rewrite those rules first",
            args.name,
            used_by.join(", ")
        );
    }

    blueprint.variables.remove(&args.name);
    write(&path, &blueprint)?;
    Ok(format!(
        "✓ removed variable '{}' from {}",
        args.name,
        path.display()
    ))
}

fn summary(decl: &VariableDecl) -> String {
    match (&decl.default, decl.required) {
        (Some(value), _) => format!("default: {value:?}"),
        (None, true) => "required".to_string(),
        (None, false) => "optional".to_string(),
    }
}

/// The permission rules whose filter references `${vars.<name>}`, as
/// `<caller>/<capability>` labels, in file order.
fn referencing_rules(blueprint: &Blueprint, name: &str) -> Vec<String> {
    blueprint
        .permissions
        .iter()
        .flat_map(|(caller, rules)| rules.iter().map(move |rule| (caller, rule)))
        .filter(|(_, rule)| {
            rule.filter
                .as_ref()
                .is_some_and(|filter| filter.var_refs().contains(&name))
        })
        .map(|(caller, rule)| format!("{caller}/{}", rule.capability))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use super::*;

    fn add_args(name: &str, path: &Path) -> AddArgs {
        AddArgs {
            name: name.into(),
            required: false,
            default: None,
            blueprint: Some(path.to_path_buf()),
        }
    }

    fn temp_blueprint(body: &str) -> (tempfile::TempDir, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("blueprint.yaml");
        fs::write(&path, body).unwrap();
        (tmp, path)
    }

    fn reload(path: &Path) -> Blueprint {
        submilli_blueprint::parse(&fs::read_to_string(path).unwrap()).unwrap()
    }

    const WITH_RULE: &str = "\
name: t
variables:
  tenant:
    required: true
permissions:
  main:
  - capability: fs.read
    filter: path glob \"${vars.tenant}/*\"
    action: allow
";

    #[test]
    fn adds_required_variable() {
        let (_tmp, path) = temp_blueprint("name: t\n");
        let mut args = add_args("customerId", &path);
        args.required = true;
        let message = add(&args).unwrap();
        assert!(message.contains("required"), "{message}");
        assert_eq!(
            reload(&path).variables["customerId"],
            VariableDecl {
                required: true,
                default: None,
            }
        );
    }

    #[test]
    fn adds_variable_with_default() {
        let (_tmp, path) = temp_blueprint("name: t\n");
        let mut args = add_args("region", &path);
        args.default = Some("us".into());
        add(&args).unwrap();
        assert_eq!(
            reload(&path).variables["region"],
            VariableDecl {
                required: false,
                default: Some("us".into()),
            }
        );
    }

    #[test]
    fn adds_optional_variable() {
        let (_tmp, path) = temp_blueprint("name: t\n");
        let message = add(&add_args("note", &path)).unwrap();
        assert!(message.contains("optional"), "{message}");
        assert!(reload(&path).variables.contains_key("note"));
    }

    #[test]
    fn rejects_duplicate_variable() {
        let (_tmp, path) = temp_blueprint("name: t\n");
        add(&add_args("customerId", &path)).unwrap();
        let err = add(&add_args("customerId", &path)).unwrap_err();
        assert!(err.to_string().contains("already declares"), "{err}");
    }

    #[test]
    fn missing_blueprint_is_an_error() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(add(&add_args("x", &tmp.path().join("nope.yaml"))).is_err());
    }

    #[test]
    fn list_shows_declaration_and_usage() {
        let (_tmp, path) = temp_blueprint(WITH_RULE);
        let mut unused = add_args("region", &path);
        unused.default = Some("us".into());
        add(&unused).unwrap();
        let out = list(&ListArgs {
            blueprint: Some(path.clone()),
        })
        .unwrap();
        assert!(
            out.contains("tenant  (required; used by main/fs.read)"),
            "{out}"
        );
        assert!(out.contains("region  (default: \"us\"; unused)"), "{out}");
    }

    #[test]
    fn list_of_empty_block() {
        let (_tmp, path) = temp_blueprint("name: t\n");
        let out = list(&ListArgs {
            blueprint: Some(path.clone()),
        })
        .unwrap();
        assert!(out.starts_with("no variables declared"), "{out}");
    }

    #[test]
    fn remove_drops_an_unreferenced_variable() {
        let (_tmp, path) = temp_blueprint("name: t\n");
        add(&add_args("note", &path)).unwrap();
        remove(&RemoveArgs {
            name: "note".into(),
            blueprint: Some(path.clone()),
        })
        .unwrap();
        assert!(reload(&path).variables.is_empty());
    }

    #[test]
    fn remove_refuses_a_referenced_variable() {
        let (_tmp, path) = temp_blueprint(WITH_RULE);
        let err = remove(&RemoveArgs {
            name: "tenant".into(),
            blueprint: Some(path.clone()),
        })
        .unwrap_err();
        assert!(err.to_string().contains("main/fs.read"), "{err}");
        assert!(reload(&path).variables.contains_key("tenant"));
    }

    #[test]
    fn remove_unknown_variable_is_an_error() {
        let (_tmp, path) = temp_blueprint("name: t\n");
        let err = remove(&RemoveArgs {
            name: "nope".into(),
            blueprint: Some(path.clone()),
        })
        .unwrap_err();
        assert!(err.to_string().contains("no variable"), "{err}");
    }
}
