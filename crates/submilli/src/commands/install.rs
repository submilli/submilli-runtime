//! `submilli install <url> [package]` — download a package source from GitHub,
//! compile it, and place it in the local package store, pinned to the resolved
//! commit SHA. Writes nothing to the manifest or any blueprint: the store is the
//! only thing it touches. The fetch lives in `submilli-shared`; the
//! compile-and-store core is shared with `server packages install` via
//! `submilli_build::install_from_dir`.

use std::path::Path;
use std::process::ExitCode;

use interpreter::{Severity, Sources, Span, diagnostics};
use submilli_build::{
    BuildDiagnostic, BuildSeverity, DriverError, GithubSource, InstallError, Lockfile, PackageName,
    PackageSource, PackageStore, ResolveError, install_from_dir, install_plan, load_manifest,
    resolve_github_closure,
};
use submilli_shared::github;
use submilli_shared::github::GithubRepoFetcher;

#[derive(clap::Args)]
pub struct Args {
    /// GitHub repo to install from: `org/repo`, `github.com/org/repo`, or a full
    /// URL — optionally pinned with `@<ref>` (branch, tag, or commit SHA).
    url: String,

    /// Install only this package (`@org/name`). Omit to install every package
    /// the repo declares.
    package: Option<String>,

    /// Re-install over a package already in the store at a different commit.
    #[arg(long)]
    upgrade: bool,
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
    let fetched = match github::fetch(&args.url) {
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
    let only = args.package.map(PackageName::new);
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
    let closure = match resolve_github_closure(
        &store,
        &manifest,
        &GithubRepoFetcher,
        existing_lock.as_ref(),
    ) {
        Ok(closure) => closure,
        Err(err) => {
            render_resolve_error(&err);
            return Ok(ExitCode::from(1));
        }
    };
    if let Err(err) = install_plan(&store, &closure.plan, args.upgrade) {
        render_install_error(&err);
        return Ok(ExitCode::from(1));
    }

    match install_from_dir(
        &store,
        fetched.dir.path(),
        only.as_ref(),
        &source,
        args.upgrade,
    ) {
        Ok(report) => {
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

fn render_manifest_diagnostics(
    manifest_path: &Path,
    manifest_text: &str,
    diags: &[BuildDiagnostic],
) {
    let (sources, file) = Sources::single(
        manifest_path.display().to_string(),
        manifest_text.to_string(),
    );
    for diag in diags {
        let span = match diag.span {
            Some(span) => Span::new(file, span.start as u32, span.end as u32),
            None => Span::at(file),
        };
        let rendered = diagnostics::render(
            &interpreter::Diagnostic {
                severity: match diag.severity {
                    BuildSeverity::Error => Severity::Error,
                    BuildSeverity::Warning => Severity::Warning,
                },
                span,
                message: diag.message.clone(),
                help: diag.help.clone(),
                notes: Vec::new(),
            },
            &sources,
        );
        eprint!("{rendered}");
    }
}
