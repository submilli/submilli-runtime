//! `submilli upgrade` — replace this executable with a published release.
//!
//! Explicit by design: nothing replaces the executable unless the user asks.
//! The download follows the installer scripts: one release is resolved once,
//! the platform asset is checked against that release's `SHA256SUMS`, and the
//! replacement is staged beside the executable and renamed into place.
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, ExitCode},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::http::{read_limited, release_source};

const SOURCE_ENV: &str = "SUBMILLI_RELEASE_SOURCE";
const CHECK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
const MAX_EXECUTABLE_BYTES: u64 = 512 * 1024 * 1024;
const MAX_CHECKSUMS_BYTES: u64 = 1024 * 1024;

#[derive(clap::Args)]
pub struct Args {
    /// Release tag to install (for example v0.2.0). Defaults to the latest release.
    #[arg(long, value_name = "TAG")]
    version: Option<String>,
    /// Report the latest release without installing it; exit 1 if it is newer.
    #[arg(long)]
    check: bool,
}

pub fn execute(args: Args) -> anyhow::Result<ExitCode> {
    let source = release_source(SOURCE_ENV)?;
    let current = concat!("v", env!("CARGO_PKG_VERSION"));
    let requested = args.version.is_some();
    let tag = match args.version {
        Some(tag) => validated_tag(tag)?,
        None => latest_tag(&source, Duration::from_secs(15))?,
    };
    // Only an explicit --version may move to an older release.
    if !requested && parse_version(&tag) <= parse_version(current) {
        println!("submilli {current} is up to date (latest release: {tag}).");
        return Ok(ExitCode::SUCCESS);
    }
    if args.check {
        println!("submilli {tag} is available (running {current}). Run `submilli upgrade`.");
        return Ok(ExitCode::FAILURE);
    }
    let executable = replaceable_executable()?;
    let replacement = download_verified(&source, &tag)?;
    replace(&executable, &replacement)?;
    println!("Upgraded {} from {current} to {tag}.", executable.display());
    // The new executable carries a newer embedded skill; let it refresh
    // installations. Best effort: the upgrade itself has already succeeded.
    let _ = Command::new(&executable).args(["skill", "sync"]).status();
    Ok(ExitCode::SUCCESS)
}

/// A newer release tag, if a check within the last day (or a quick one now)
/// found one. For callers that only mention an upgrade: never an error, and
/// at most one short request per day.
pub fn newer_release_hint() -> Option<String> {
    #[derive(Serialize, Deserialize)]
    struct Cache {
        checked_at: u64,
        latest: Option<String>,
    }
    let path = submilli_build::default_data_root().join("cli-release.json");
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs());
    let cached: Option<Cache> = fs::read(&path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok());
    let latest = match cached {
        Some(cache) if now.saturating_sub(cache.checked_at) < CHECK_INTERVAL.as_secs() => {
            cache.latest
        }
        _ => {
            let latest = release_source(SOURCE_ENV)
                .and_then(|source| latest_tag(&source, Duration::from_secs(3)))
                .ok();
            let cache = Cache {
                checked_at: now,
                latest: latest.clone(),
            };
            if let Some(parent) = path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            if let Ok(bytes) = serde_json::to_vec(&cache) {
                let _ = fs::write(&path, bytes);
            }
            latest
        }
    };
    latest.filter(|tag| parse_version(tag) > parse_version(env!("CARGO_PKG_VERSION")))
}

/// `releases/latest` redirects to the latest release's tag page. Skill
/// releases are published as not-latest, so this always names a CLI release.
fn latest_tag(source: &str, timeout: Duration) -> anyhow::Result<String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .max_redirects(0)
        .max_redirects_will_error(false)
        .http_status_as_error(false)
        .build()
        .into();
    let response = agent
        .get(format!("{source}/releases/latest"))
        .call()
        .context("resolving the latest release")?;
    let location = response
        .headers()
        .get("location")
        .and_then(|value| value.to_str().ok())
        .with_context(|| format!("no latest release at {source} (HTTP {})", response.status()))?;
    let tag = location
        .rsplit_once("/releases/tag/")
        .map(|(_, tag)| tag.to_owned())
        .context("could not resolve the latest release tag")?;
    validated_tag(tag)
}

fn validated_tag(tag: String) -> anyhow::Result<String> {
    let plain = tag
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    if tag.is_empty() || tag.starts_with('-') || !plain {
        bail!("invalid release tag {tag:?}");
    }
    Ok(tag)
}

fn asset_name() -> anyhow::Result<&'static str> {
    Ok(match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => "submilli-x86_64-unknown-linux-musl",
        ("macos", "x86_64") => "submilli-x86_64-apple-darwin",
        ("macos", "aarch64") => "submilli-aarch64-apple-darwin",
        ("windows", "x86_64") => "submilli-x86_64-pc-windows-msvc.exe",
        (os, arch) => bail!("no released executable for {os} {arch}; build from source"),
    })
}

/// This executable's path, unless something else owns its upgrades.
fn replaceable_executable() -> anyhow::Result<PathBuf> {
    let executable = std::env::current_exe()
        .and_then(fs::canonicalize)
        .context("locating the running executable")?;
    let path = executable.to_string_lossy().replace('\\', "/");
    for (marker, advice) in [
        ("/Cellar/", "upgrade it with Homebrew"),
        ("/.cargo/bin/", "upgrade it with cargo install"),
        ("/target/debug/", "rebuild it with cargo"),
        ("/target/release/", "rebuild it with cargo"),
    ] {
        if path.contains(marker) {
            bail!(
                "{} is not managed by this command; {advice}",
                executable.display()
            );
        }
    }
    Ok(executable)
}

fn download_verified(source: &str, tag: &str) -> anyhow::Result<Vec<u8>> {
    let asset = asset_name()?;
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(300)))
        .build()
        .into();
    let base = format!("{source}/releases/download/{tag}");
    let fetch = |name: &str, limit: u64| {
        let response = agent
            .get(format!("{base}/{name}"))
            .call()
            .with_context(|| format!("downloading {name} for {tag}"))?;
        read_limited(response, limit)
    };
    let checksums = String::from_utf8(fetch("SHA256SUMS", MAX_CHECKSUMS_BYTES)?)
        .context("malformed SHA256SUMS")?;
    let expected: Vec<&str> = checksums
        .lines()
        .filter_map(|line| line.split_once(char::is_whitespace))
        .filter(|(_, name)| name.trim().trim_start_matches('*') == asset)
        .map(|(hash, _)| hash)
        .collect();
    let [expected] = expected[..] else {
        bail!("missing or ambiguous checksum for {asset} in {tag}");
    };
    let bytes = fetch(asset, MAX_EXECUTABLE_BYTES)?;
    if !format!("{:x}", Sha256::digest(&bytes)).eq_ignore_ascii_case(expected) {
        bail!("checksum mismatch for {asset}; the installed executable was not changed");
    }
    Ok(bytes)
}

/// Stage beside the executable so the final step is a rename on one filesystem.
fn replace(executable: &Path, replacement: &[u8]) -> anyhow::Result<()> {
    let directory = executable.parent().context("executable has no parent")?;
    let staged = tempfile::Builder::new()
        .prefix(".submilli-upgrade-")
        .tempfile_in(directory)
        .with_context(|| format!("cannot write to {}", directory.display()))?;
    fs::write(staged.path(), replacement)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(staged.path(), fs::Permissions::from_mode(0o755))?;
    }
    let runs = Command::new(staged.path())
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success());
    if !runs {
        bail!("the downloaded executable does not run here; the installed one was not changed");
    }
    // Windows cannot overwrite a running executable but can rename it aside.
    #[cfg(windows)]
    {
        let aside = executable.with_extension("old.exe");
        let _ = fs::remove_file(&aside);
        fs::rename(executable, &aside)?;
    }
    staged
        .persist(executable)
        .map(drop)
        .context("replacing the executable")
}

fn parse_version(text: &str) -> Vec<u64> {
    text.trim_start_matches('v')
        .split(['.', '-', '+'])
        .map_while(|part| part.parse().ok())
        .collect()
}
