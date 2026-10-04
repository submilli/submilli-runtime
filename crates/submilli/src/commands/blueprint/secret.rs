//! `submilli blueprint secret {add,list,remove}` — manage the secrets declared
//! in a local `blueprint.yaml`'s `secrets:` block, so operators don't hand-edit
//! the YAML. This is local authoring; `submilli server secret` is the separate
//! command that writes values into a running server's secret store.

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Result, bail};
use clap::{ArgGroup, Subcommand};
use submilli_blueprint::{HarnessSecret, SecretSource};

use super::file::{blueprint_path, load, write};

#[derive(Subcommand)]
pub enum SecretCmd {
    /// Declare a secret in the blueprint's `secrets:` block.
    Add(AddArgs),
    /// List the blueprint's declared secrets and their sources (never values).
    List(ListArgs),
    /// Remove a declared secret. Refused while `auth_proxy:`, `mcp:`, or `llm:`
    /// still references it.
    Remove(RemoveArgs),
}

pub fn execute(cmd: SecretCmd) -> Result<ExitCode> {
    let result = match cmd {
        SecretCmd::Add(args) => add(&args),
        SecretCmd::List(args) => list(&args),
        SecretCmd::Remove(args) => remove(&args),
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

/// Exactly one source must be given.
#[derive(clap::Args)]
#[command(group = ArgGroup::new("source").required(true).args(["store", "harness"]))]
pub struct AddArgs {
    /// Secret name — what `${secrets.<NAME>}` references.
    name: String,
    /// Read the value from the server's SecretStore under this key.
    #[arg(long, value_name = "KEY")]
    store: Option<String>,
    /// Bind the value from the trusted harness separately for each session.
    #[arg(long)]
    harness: bool,
    /// Reject session creation or rebind when the harness value is absent.
    #[arg(long, requires = "harness")]
    required: bool,
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
    /// The secret to remove.
    name: String,
    /// Blueprint file to edit (default: ./blueprint.yaml).
    #[arg(long)]
    blueprint: Option<PathBuf>,
}

fn add(args: &AddArgs) -> Result<String> {
    let path = blueprint_path(&args.blueprint);
    let mut blueprint = load(&path)?;

    if blueprint.secrets.contains_key(&args.name) {
        bail!(
            "blueprint '{}' already declares a secret '{}'",
            blueprint.name,
            args.name
        );
    }

    let source = source(args)?;
    let summary = source_summary(&source);
    blueprint.secrets.insert(args.name.clone(), source);
    write(&path, &blueprint)?;

    Ok(format!(
        "✓ declared secret '{}' ({summary}) in {}",
        args.name,
        path.display()
    ))
}

fn list(args: &ListArgs) -> Result<String> {
    let path = blueprint_path(&args.blueprint);
    let blueprint = load(&path)?;
    if blueprint.secrets.is_empty() {
        return Ok(format!("no secrets declared in {}", path.display()));
    }
    let lines: Vec<String> = blueprint
        .secrets
        .iter()
        .map(|(name, source)| format!("{name}  ({})", source_summary(source)))
        .collect();
    Ok(lines.join("\n"))
}

/// Removal leaves the blueprint's own validation to catch a `${secrets.NAME}`
/// still referenced from `auth_proxy:`, `mcp:`, or `llm:`: the re-parse before
/// writing fails and names the reference, and the file is left untouched.
fn remove(args: &RemoveArgs) -> Result<String> {
    let path = blueprint_path(&args.blueprint);
    let mut blueprint = load(&path)?;
    if blueprint.secrets.remove(&args.name).is_none() {
        bail!("no secret '{}' declared in {}", args.name, path.display());
    }
    write(&path, &blueprint)?;
    Ok(format!(
        "✓ removed secret '{}' from {}",
        args.name,
        path.display()
    ))
}

/// The chosen source. The `source` arg group guarantees exactly one is set; the
/// fallback arm is defensive.
fn source(args: &AddArgs) -> Result<SecretSource> {
    match (&args.store, args.harness) {
        (Some(key), false) => Ok(SecretSource::Store(key.clone())),
        (None, true) => Ok(SecretSource::Harness(HarnessSecret {
            required: args.required,
        })),
        _ => bail!("exactly one of --store / --harness is required"),
    }
}

fn source_summary(source: &SecretSource) -> String {
    match source {
        SecretSource::Store(v) => format!("store: {v}"),
        SecretSource::Harness(config) => {
            format!("harness, required: {}", config.required)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use super::*;

    fn add_args(name: &str, path: &Path) -> AddArgs {
        AddArgs {
            name: name.into(),
            store: None,
            harness: false,
            required: false,
            blueprint: Some(path.to_path_buf()),
        }
    }

    fn temp_blueprint() -> (tempfile::TempDir, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("blueprint.yaml");
        fs::write(&path, "name: test\n").unwrap();
        (tmp, path)
    }

    #[test]
    fn adds_store_secret() {
        let (_tmp, path) = temp_blueprint();
        let mut args = add_args("K", &path);
        args.store = Some("prod/key".into());
        add(&args).unwrap();
        let bp = submilli_blueprint::parse(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(bp.secrets["K"], SecretSource::Store("prod/key".into()));
    }

    #[test]
    fn adds_required_harness_secret() {
        let (_tmp, path) = temp_blueprint();
        let mut args = add_args("SLACK_USER_TOKEN", &path);
        args.harness = true;
        args.required = true;
        add(&args).unwrap();
        let bp = submilli_blueprint::parse(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            bp.secrets["SLACK_USER_TOKEN"],
            SecretSource::Harness(HarnessSecret { required: true })
        );
    }

    #[test]
    fn rejects_duplicate_secret() {
        let (_tmp, path) = temp_blueprint();
        let mut a = add_args("K", &path);
        a.store = Some("K_VAR".into());
        add(&a).unwrap();
        let mut dup = add_args("K", &path);
        dup.store = Some("OTHER".into());
        let err = add(&dup).unwrap_err();
        assert!(err.to_string().contains("already declares"), "{err}");
    }

    #[test]
    fn missing_blueprint_is_an_error() {
        let tmp = tempfile::tempdir().unwrap();
        let mut a = add_args("K", &tmp.path().join("nope.yaml"));
        a.store = Some("K".into());
        assert!(add(&a).is_err());
    }

    #[test]
    fn list_shows_names_and_sources_only() {
        let (_tmp, path) = temp_blueprint();
        let mut a = add_args("GH", &path);
        a.store = Some("prod/gh".into());
        add(&a).unwrap();
        let out = list(&ListArgs {
            blueprint: Some(path.clone()),
        })
        .unwrap();
        assert_eq!(out, "GH  (store: prod/gh)");
    }

    #[test]
    fn remove_drops_an_unreferenced_secret() {
        let (_tmp, path) = temp_blueprint();
        let mut a = add_args("GH", &path);
        a.store = Some("GH".into());
        add(&a).unwrap();
        remove(&RemoveArgs {
            name: "GH".into(),
            blueprint: Some(path.clone()),
        })
        .unwrap();
        let bp = submilli_blueprint::parse(&fs::read_to_string(&path).unwrap()).unwrap();
        assert!(bp.secrets.is_empty());
    }

    #[test]
    fn remove_refuses_a_referenced_secret() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("blueprint.yaml");
        let body = "name: t\nsecrets:\n  GH: { store: GH }\nauth_proxy:\n  - host: api.github.com\n    auth: { bearer: GH }\n";
        fs::write(&path, body).unwrap();
        let err = remove(&RemoveArgs {
            name: "GH".into(),
            blueprint: Some(path.clone()),
        })
        .unwrap_err();
        assert!(err.to_string().contains("not written"), "{err}");
        assert_eq!(fs::read_to_string(&path).unwrap(), body);
    }

    #[test]
    fn remove_unknown_secret_is_an_error() {
        let (_tmp, path) = temp_blueprint();
        let err = remove(&RemoveArgs {
            name: "nope".into(),
            blueprint: Some(path.clone()),
        })
        .unwrap_err();
        assert!(err.to_string().contains("no secret"), "{err}");
    }
}
