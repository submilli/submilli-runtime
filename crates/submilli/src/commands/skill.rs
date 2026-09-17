//! Release-bundled, offline skill distribution. Never replace user-edited files.
use std::{
    collections::BTreeMap,
    fs,
    io::IsTerminal,
    path::{Path, PathBuf},
    process::ExitCode,
};

use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const RECEIPT: &str = ".submilli-skill.json";

macro_rules! bundle {
    ($($path:literal),+ $(,)?) => {
        &[$(($path, include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../skills/submilli/", $path)))),+]
    };
}

const FILES: &[(&str, &str)] = bundle![
    "SKILL.md",
    "references/setup.md",
    "references/concepts.md",
    "references/discovery.md",
    "references/capability-design.md",
    "references/packages.md",
    "references/blueprints.md",
    "references/use-cases.md",
    "references/harnesses.md",
    "references/deepagents.md",
    "references/vercel.md",
    "references/langchain.md",
    "references/mastra.md",
    "references/custom-loop.md",
];

#[derive(clap::Subcommand)]
pub enum SkillCmd {
    /// Install the skill bundled with this CLI (no network required).
    Install(Target),
    /// Refresh an installed, unmodified skill from this CLI's bundle.
    Update(Target),
    /// Check installation integrity and freshness; exit 1 if missing or different.
    Status(Target),
}

#[derive(Clone, Copy, clap::ValueEnum)]
enum Agent {
    Claude,
    Codex,
    Cursor,
}

impl Agent {
    fn directory(self) -> &'static str {
        match self {
            Self::Claude => ".claude",
            Self::Codex => ".agents",
            Self::Cursor => ".cursor",
        }
    }
}

#[derive(clap::Args)]
pub struct Target {
    /// Assistant whose discovery directory to use.
    #[arg(long, value_enum)]
    agent: Agent,
    /// Install in this project instead of the user home directory.
    #[arg(long, value_name = "DIRECTORY")]
    project: Option<PathBuf>,
}

#[derive(Serialize, Deserialize)]
struct Receipt {
    schema: u32,
    cli_version: String,
    files: BTreeMap<String, String>,
}

pub fn execute(cmd: SkillCmd) -> anyhow::Result<ExitCode> {
    let target = match &cmd {
        SkillCmd::Install(t) | SkillCmd::Update(t) | SkillCmd::Status(t) => t,
    };
    let root = match &target.project {
        Some(path) => path.clone(),
        None => user_home().context("cannot locate home; use --project DIRECTORY")?,
    };
    let root = root
        .canonicalize()
        .with_context(|| format!("opening {}", root.display()))?;
    let path = root.join(target.agent.directory()).join("skills/submilli");
    check_ancestors(&root, &path)?;
    match cmd {
        SkillCmd::Status(_) => {
            let state = status(&path)?;
            println!(
                "{}: {state} (CLI {})",
                path.display(),
                env!("CARGO_PKG_VERSION")
            );
            Ok(if state == "current" {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            })
        }
        SkillCmd::Install(_) => {
            install(&path, false)?;
            Ok(ExitCode::SUCCESS)
        }
        SkillCmd::Update(_) => {
            install(&path, true)?;
            Ok(ExitCode::SUCCESS)
        }
    }
}

/// No network or mutation on ordinary invocations. Project copies and user
/// copies are checked so a CLI upgrade makes the update step discoverable.
pub fn warn_if_outdated() {
    if !std::io::stderr().is_terminal() {
        return;
    }
    let mut roots = Vec::new();
    if let Some(home) = user_home() {
        roots.push(home);
    }
    if let Ok(cwd) = std::env::current_dir() {
        for dir in cwd.ancestors() {
            roots.push(dir.to_path_buf());
            if dir.join(".git").exists() {
                break;
            }
        }
    }
    roots.sort();
    roots.dedup();
    let bundled = bundled_receipt();
    for root in roots {
        for agent in [Agent::Claude, Agent::Codex, Agent::Cursor] {
            let path = root.join(agent.directory()).join("skills/submilli");
            if check_ancestors(&root, &path).is_ok()
                && let Ok(receipt) = read_receipt(&path)
                && receipt.files != bundled.files
            {
                eprintln!(
                    "Submilli skill at {} differs from this CLI. Run `submilli skill status` then `submilli skill update` with its --agent and --project target (omit --project for a user install). Local edits are preserved.",
                    path.display()
                );
            }
        }
    }
}

fn user_home() -> Option<PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
}

fn check_ancestors(root: &Path, path: &Path) -> anyhow::Result<()> {
    let mut current = root.to_path_buf();
    for component in path.strip_prefix(root)?.components() {
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(meta) if !meta.is_dir() || meta.file_type().is_symlink() => {
                bail!("refusing non-directory or symlink {}", current.display())
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn bundled_receipt() -> Receipt {
    Receipt {
        schema: 1,
        cli_version: env!("CARGO_PKG_VERSION").to_owned(),
        files: FILES
            .iter()
            .map(|(p, content)| ((*p).to_owned(), digest(content.as_bytes())))
            .collect(),
    }
}

fn read_receipt(path: &Path) -> anyhow::Result<Receipt> {
    let receipt_path = path.join(RECEIPT);
    let meta = fs::symlink_metadata(&receipt_path)
        .context("not a managed installation; move it aside before installing")?;
    if !meta.is_file() || meta.file_type().is_symlink() {
        bail!("invalid skill receipt");
    }
    let receipt: Receipt = serde_json::from_slice(&fs::read(receipt_path)?)?;
    if receipt.schema != 1 {
        bail!("unsupported skill receipt; upgrade the CLI");
    }
    Ok(receipt)
}

fn disk_files(root: &Path, dir: &Path, out: &mut BTreeMap<String, String>) -> anyhow::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            bail!("refusing symlink {}", path.display());
        }
        if kind.is_dir() {
            disk_files(root, &path, out)?;
        } else if kind.is_file() {
            let name = path
                .strip_prefix(root)?
                .components()
                .map(|part| {
                    part.as_os_str()
                        .to_str()
                        .context("non-UTF-8 filename in installed skill")
                })
                .collect::<anyhow::Result<Vec<_>>>()?
                .join("/");
            if name != RECEIPT {
                out.insert(name, digest(&fs::read(path)?));
            }
        } else {
            bail!("unsupported file {}", path.display());
        }
    }
    Ok(())
}

fn status(path: &Path) -> anyhow::Result<&'static str> {
    if !path.try_exists()? {
        return Ok("not installed");
    }
    let receipt = read_receipt(path)?;
    let mut actual = BTreeMap::new();
    disk_files(path, path, &mut actual)?;
    if actual != receipt.files {
        return Ok("locally modified; preserved");
    }
    Ok(if receipt.files == bundled_receipt().files {
        "current"
    } else {
        "different from bundled skill; run submilli skill update with the same target"
    })
}

fn install(path: &Path, update: bool) -> anyhow::Result<()> {
    let parent = path.parent().context("skill target needs a parent")?;
    fs::create_dir_all(parent)?;
    // Serialize installers before inspecting the receipt. No automatic stale
    // lock deletion: another process may still be using it.
    let lock_path = parent.join(".submilli-install.lock");
    let _lock_file = fs::OpenOptions::new().write(true).create_new(true).open(&lock_path)
        .with_context(|| format!("cannot acquire {}; if a previous installer crashed, remove this lock only after confirming no installer is running", lock_path.display()))?;
    let _lock = InstallLock(lock_path);
    drop(_lock_file);
    let exists = path.try_exists()?;
    if update && !exists {
        bail!(
            "skill is not installed at {}; run skill install first",
            path.display()
        );
    }
    if exists {
        let state = status(path)?;
        if state == "current" {
            println!("{}: current", path.display());
            return Ok(());
        }
        if state == "locally modified; preserved" {
            bail!(
                "{}: {state}. Move this directory aside to keep your edits, then install again",
                path.display()
            );
        }
        if !update {
            bail!(
                "already installed; use skill update with the same --agent and --project options"
            );
        }
    }
    // Both renames stay on the same filesystem. The previous directory is kept
    // until the replacement succeeds; a failed second rename restores it.
    let staging = tempfile::Builder::new()
        .prefix(".submilli-stage-")
        .tempdir_in(parent)?;
    let next = staging.path().join("next");
    fs::create_dir(&next)?;
    for (name, content) in FILES {
        let file = next.join(name);
        fs::create_dir_all(file.parent().context("bundled file needs a parent")?)?;
        fs::write(file, content)?;
    }
    fs::write(
        next.join(RECEIPT),
        serde_json::to_vec_pretty(&bundled_receipt())?,
    )?;
    let previous = staging.path().join("previous");
    if exists {
        fs::rename(path, &previous)?;
    }
    if let Err(error) = fs::rename(&next, path) {
        if exists && let Err(restore) = fs::rename(&previous, path) {
            let recovery = staging.keep();
            bail!(
                "install failed: {error}; restore failed: {restore}; previous skill preserved in {}",
                recovery.join("previous").display()
            );
        }
        return Err(error.into());
    }
    println!(
        "Installed Submilli skill from CLI {} at {}. Restart your assistant to reload it.",
        env!("CARGO_PKG_VERSION"),
        path.display()
    );
    Ok(())
}

struct InstallLock(PathBuf);
impl Drop for InstallLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
