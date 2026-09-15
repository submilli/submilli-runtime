//! `submilli blueprint secret add <name> --env|--file|--store|--harness` — declare
//! a secret in a local `blueprint.yaml`'s `secrets:` block, so operators don't
//! hand-edit the YAML. This is local authoring; `submilli server secret` is the
//! separate command that writes values into a running server's secret store.

use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::{ArgGroup, Subcommand};
use submilli_blueprint::{HarnessSecret, SecretSource};

const DEFAULT_FILE: &str = "blueprint.yaml";

#[derive(Subcommand)]
pub enum SecretCmd {
    /// Declare a secret in the blueprint's `secrets:` block.
    Add(AddArgs),
}

pub fn execute(cmd: SecretCmd) -> Result<ExitCode> {
    let result = match cmd {
        SecretCmd::Add(args) => add(&args),
    };
    match result {
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

/// Exactly one source must be given.
#[derive(clap::Args)]
#[command(group = ArgGroup::new("source").required(true).args(["env", "file", "store", "harness"]))]
pub struct AddArgs {
    /// Secret name — what `${secrets.<NAME>}` references.
    name: String,
    /// Read the value from this server-process environment variable.
    #[arg(long, value_name = "ENV_VAR")]
    env: Option<String>,
    /// Read the value from this file path (e.g. a k8s secret mount), trimmed.
    #[arg(long, value_name = "PATH")]
    file: Option<String>,
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

fn add(args: &AddArgs) -> Result<String> {
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

    let updated = submilli_blueprint::to_yaml(&blueprint);
    // Re-validate before writing so we never leave an unparseable blueprint on disk.
    submilli_blueprint::parse(&updated)
        .context("the resulting blueprint is invalid — not written")?;
    fs::write(&path, &updated).with_context(|| format!("writing {}", path.display()))?;

    Ok(format!(
        "declared secret '{}' ({summary}) in {}",
        args.name,
        path.display()
    ))
}

/// The chosen source. The `source` arg group guarantees exactly one is set; the
/// fallback arm is defensive.
fn source(args: &AddArgs) -> Result<SecretSource> {
    match (&args.env, &args.file, &args.store, args.harness) {
        (Some(v), _, _, _) => Ok(SecretSource::Env(v.clone())),
        (_, Some(v), _, _) => Ok(SecretSource::File(v.clone())),
        (_, _, Some(v), _) => Ok(SecretSource::Store(v.clone())),
        (_, _, _, true) => Ok(SecretSource::Harness(HarnessSecret {
            required: args.required,
        })),
        _ => bail!("exactly one of --env / --file / --store / --harness is required"),
    }
}

fn source_summary(source: &SecretSource) -> String {
    match source {
        SecretSource::Env(v) => format!("env: {v}"),
        SecretSource::File(v) => format!("file: {v}"),
        SecretSource::Store(v) => format!("store: {v}"),
        SecretSource::Harness(config) => {
            format!("harness, required: {}", config.required)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    fn add_args(name: &str, path: &Path) -> AddArgs {
        AddArgs {
            name: name.into(),
            env: None,
            file: None,
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
    fn adds_env_secret() {
        let (_tmp, path) = temp_blueprint();
        let mut a = add_args("LINEAR_API_KEY", &path);
        a.env = Some("LINEAR_API_KEY".into());
        add(&a).unwrap();
        let bp = submilli_blueprint::parse(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            bp.secrets["LINEAR_API_KEY"],
            SecretSource::Env("LINEAR_API_KEY".into())
        );
    }

    #[test]
    fn adds_file_and_store_secrets() {
        let (_tmp, path) = temp_blueprint();
        let mut f = add_args("A", &path);
        f.file = Some("/run/secrets/a".into());
        add(&f).unwrap();
        let mut s = add_args("B", &path);
        s.store = Some("prod/b".into());
        add(&s).unwrap();
        let bp = submilli_blueprint::parse(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(bp.secrets["A"], SecretSource::File("/run/secrets/a".into()));
        assert_eq!(bp.secrets["B"], SecretSource::Store("prod/b".into()));
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
        a.env = Some("K_VAR".into());
        add(&a).unwrap();
        let mut dup = add_args("K", &path);
        dup.env = Some("OTHER".into());
        let err = add(&dup).unwrap_err();
        assert!(err.to_string().contains("already declares"), "{err}");
    }

    #[test]
    fn missing_blueprint_is_an_error() {
        let tmp = tempfile::tempdir().unwrap();
        let mut a = add_args("K", &tmp.path().join("nope.yaml"));
        a.env = Some("K".into());
        assert!(add(&a).is_err());
    }
}
