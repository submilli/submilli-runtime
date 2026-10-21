//! `submilli install <url> [package]` — download a package source from GitHub,
//! compile it, and place it in the local package store, pinned to the resolved
//! commit SHA. Writes nothing to the manifest or any blueprint: the store is the
//! only thing it touches. The fetch lives in `submilli-shared`; the
//! compile-and-store core is shared with `server packages install` via
//! `submilli_build::install_from_dir`.

use std::process::ExitCode;

use super::build::render_manifest_diagnostics;
use submilli_build::{
    DriverError, GithubSource, InstallError, InstallPreparation, Lockfile, PackageName,
    PackageSource, PackageStore, ResolveError, deny_warnings_from_env, load_manifest,
    resolve_github_closure, warning_denial_message,
};
use submilli_shared::github;

use crate::commands::github::retry::{self, LazyAuth};

#[derive(clap::Args)]
pub struct Args {
    /// Set a src/config.ts parameter of the selected package.
    #[arg(long, value_name = "NAME=VALUE")]
    config: Vec<String>,
    /// Remove a saved override and use the source default.
    #[arg(long, value_name = "NAME")]
    reset_config: Vec<String>,
    /// GitHub repo to install from: `org/repo`, `github.com/org/repo`, or a full
    /// URL — optionally pinned with `@<ref>` (branch, tag, or commit SHA). A
    /// private repository needs a GitHub token: see `submilli github
    /// authenticate`.
    url: String,

    /// Install only this package (`@org/name`). Omit to install every package
    /// the repo declares.
    package: Option<String>,

    /// Replace an installed package when its source or configuration changes.
    #[arg(long)]
    upgrade: bool,

    /// Fail on code warnings; also enabled by SUBMILLI_DENY_WARNINGS=1.
    #[arg(long)]
    deny_warnings: bool,
}

impl Args {
    pub fn metric_flags(&self) -> Vec<(&'static str, bool)> {
        vec![
            ("package", self.package.is_some()),
            ("upgrade", self.upgrade),
        ]
    }
}

pub fn execute(args: Args) -> anyhow::Result<ExitCode> {
    let inputs = submilli_build::ConfigInputs::parse(
        &args.config,
        &args.reset_config,
        args.package.is_some(),
    )?;
    retry::with_authentication_retry(|auth| install(&args, auth, &inputs))
}

fn install(
    args: &Args,
    auth: &LazyAuth,
    inputs: &submilli_build::ConfigInputs,
) -> anyhow::Result<ExitCode> {
    let fetched = match github::fetch(&args.url, auth.get()) {
        Ok(fetched) => fetched,
        Err(err) => {
            eprintln!("error: {err}");
            return Ok(ExitCode::from(1));
        }
    };
    let resolved = &fetched.resolved;
    eprintln!(
        "fetched github.com/{}/{} at {}",
        resolved.org,
        resolved.repo,
        &resolved.sha[..resolved.sha.len().min(12)]
    );

    let store = PackageStore::default();
    let only = args.package.clone().map(PackageName::new);
    let source = PackageSource::Github(GithubSource {
        org: resolved.org.clone(),
        repo: resolved.repo.clone(),
        sha: resolved.sha.clone(),
        source_hash: None,
    });

    // Resolve the repo's GitHub-dependency closure (network), install it
    // deps-first, then install the repo's own package(s) — mirroring the
    // single-repo fetch → install_from_dir split.
    let manifest_path = fetched.dir.path().join("submilli.toml");
    let manifest = match load_manifest(&manifest_path) {
        Ok(manifest) => manifest,
        Err(diags) => {
            let manifest_text = std::fs::read_to_string(&manifest_path).unwrap_or_default();
            render_manifest_diagnostics(&manifest_path, &manifest_text, &diags);
            return Ok(ExitCode::from(1));
        }
    };
    let existing_lock = Lockfile::read(fetched.dir.path()).ok().flatten();
    let closure =
        match resolve_github_closure(&store, &manifest, &auth.fetcher(), existing_lock.as_ref()) {
            Ok(closure) => closure,
            Err(err) => {
                render_resolve_error(&err);
                return Ok(ExitCode::from(1));
            }
        };
    let result = (|| {
        let mut preparation = InstallPreparation::new(&store)?;
        preparation.prepare_plan(&closure.plan, args.upgrade)?;
        let report = preparation.prepare_repo_configured(
            fetched.dir.path(),
            only.as_ref(),
            &source,
            args.upgrade,
            inputs,
        )?;
        let warnings = preparation.warnings.clone();
        preparation.publish(args.deny_warnings || deny_warnings_from_env())?;
        Ok::<_, InstallError>((report, warnings))
    })();
    match result {
        Ok((report, warnings)) => {
            for warning in warnings {
                eprint!("{warning}");
            }
            for name in &report.up_to_date {
                eprintln!("up to date {}", name.as_str());
            }
            for package in &report.installed {
                eprintln!(
                    "installed {} v{} -> {}",
                    package.name.as_str(),
                    package.version.as_str(),
                    package.dir.display()
                );
            }
            Ok(ExitCode::SUCCESS)
        }
        Err(err) => {
            render_install_error(&err);
            Ok(ExitCode::from(1))
        }
    }
}

pub(crate) fn render_install_error(err: &InstallError) {
    match err {
        InstallError::ConfigurationInput(error) => eprintln!("error: {error}"),
        InstallError::ConfigurationRequired { package, .. } => {
            eprintln!(
                "error: package `{}` requires configuration. Install it first, then retry this command:",
                package.as_str()
            );
            if let Some(command) = err.configuration_command(false) {
                eprintln!("  {command}");
            }
        }
        InstallError::WarningsDenied { warnings } => {
            for warning in warnings {
                eprint!("{warning}");
            }
            eprintln!("error: {}", warning_denial_message(warnings.len()));
        }
        InstallError::Preparation(error) => eprintln!("error: preparing install: {error}"),
        InstallError::NoManifest { repo_dir } => {
            eprintln!(
                "error: {} has no submilli.toml at its root; only Submilli package repos can be installed",
                repo_dir.display()
            );
        }
        InstallError::Manifest {
            manifest_path,
            manifest_text,
            diagnostics,
        } => render_manifest_diagnostics(manifest_path, manifest_text, diagnostics),
        InstallError::Driver(DriverError::Compile { rendered, .. }) => eprint!("{rendered}"),
        InstallError::Driver(err) => eprintln!("error: {err}"),
        InstallError::ScopeMismatch {
            org,
            repo,
            packages,
        } => {
            for name in packages {
                eprintln!(
                    "error: package `{}` from github.com/{org}/{repo} must be scoped `@{org}/...` \
                     to match its source org",
                    name.as_str()
                );
            }
        }
        InstallError::Conflict {
            incoming,
            conflicts,
        } => {
            for conflict in conflicts {
                eprintln!(
                    "error: `{}` is already installed from {}; pass --upgrade to replace it with {incoming}",
                    conflict.name.as_str(),
                    conflict.current
                );
            }
        }
        InstallError::Store(err) => eprintln!("error: {err}"),
    }
}

fn render_resolve_error(err: &ResolveError) {
    match err {
        ResolveError::Manifest {
            manifest_path,
            manifest_text,
            diagnostics,
            ..
        } => render_manifest_diagnostics(manifest_path, manifest_text, diagnostics),
        other => eprintln!("error: {other}"),
    }
}
