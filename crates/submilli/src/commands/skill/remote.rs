//! Skill releases published from the runtime repository as `skill-v<N>` tags.
//!
//! The newest tag is found through git's ref advertisement rather than the
//! GitHub API, which rate-limits anonymous callers per IP. Each tag's release
//! carries one `submilli-skill.json` asset holding every file.
use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    path::{Component, Path},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};

use super::{Bundle, VERSION_FILE};

const DEFAULT_SOURCE: &str = "https://github.com/submilli/submilli-runtime";
const ASSET: &str = "submilli-skill.json";
const TAG_PREFIX: &str = "refs/tags/skill-v";
const CHECK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
/// Offline sessions should not pay the timeout on every skill invocation.
const RETRY_AFTER_FAILURE: Duration = Duration::from_secs(60 * 60);
const TIMEOUT: Duration = Duration::from_secs(5);
const MAX_REFS_BYTES: u64 = 16 * 1024 * 1024;
const MAX_BUNDLE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_FILES: usize = 200;

#[derive(Clone, Serialize, Deserialize)]
struct ReleaseFile {
    schema: u32,
    version: u32,
    #[serde(default)]
    min_cli: Option<String>,
    files: BTreeMap<String, String>,
}

#[derive(Serialize, Deserialize)]
struct Cache {
    checked_at: u64,
    release: Option<ReleaseFile>,
}

/// The newest usable release strictly newer than `bundled_version`, or `None`
/// when this CLI's bundle is at least as new or checks are disabled.
///
/// The network is consulted at most once per [`CHECK_INTERVAL`]; between checks
/// the cached release answers, so every installation converges on one version.
pub fn newest_release(bundled_version: u32) -> anyhow::Result<Option<Bundle>> {
    if std::env::var_os("SUBMILLI_SKILL_AUTOUPDATE").is_some_and(|value| value == "0") {
        return Ok(None);
    }
    let cache_path = submilli_build::default_data_root().join("skill-release.json");
    let cached = read_cache(&cache_path);
    let release = match cached {
        Some(cache) if age(cache.checked_at) < CHECK_INTERVAL => cache.release,
        stale => {
            let known = stale.and_then(|cache| cache.release);
            match fetch_newest(bundled_version, known.clone()) {
                Ok(release) => {
                    write_cache(&cache_path, now(), &release);
                    release
                }
                Err(error) => {
                    let next_check = CHECK_INTERVAL - RETRY_AFTER_FAILURE;
                    write_cache(
                        &cache_path,
                        now().saturating_sub(next_check.as_secs()),
                        &known,
                    );
                    return Err(error);
                }
            }
        }
    };
    let Some(release) = release else {
        return Ok(None);
    };
    if release.version <= bundled_version {
        return Ok(None);
    }
    if let Some(min_cli) = &release.min_cli
        && parse_version(min_cli) > parse_version(env!("CARGO_PKG_VERSION"))
    {
        bail!(
            "skill v{} needs CLI {min_cli} or newer; upgrade the CLI to receive it",
            release.version
        );
    }
    Ok(Some(Bundle {
        version: release.version,
        origin: format!("skill release v{}", release.version),
        files: release.files,
    }))
}

fn fetch_newest(
    bundled_version: u32,
    known: Option<ReleaseFile>,
) -> anyhow::Result<Option<ReleaseFile>> {
    let source = source()?;
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(TIMEOUT))
        .build()
        .into();
    let refs = read_limited(
        agent
            .get(format!("{source}.git/info/refs?service=git-upload-pack"))
            .call()
            .context("listing skill release tags")?,
        MAX_REFS_BYTES,
    )?;
    let Some(newest) = newest_tag(&String::from_utf8_lossy(&refs)) else {
        return Ok(None);
    };
    if newest <= bundled_version {
        return Ok(None);
    }
    if let Some(known) = known
        && known.version == newest
    {
        return Ok(Some(known));
    }
    let body = read_limited(
        agent
            .get(format!(
                "{source}/releases/download/skill-v{newest}/{ASSET}"
            ))
            .call()
            .with_context(|| format!("downloading skill v{newest}"))?,
        MAX_BUNDLE_BYTES,
    )?;
    let release: ReleaseFile = serde_json::from_slice(&body).context("malformed skill release")?;
    validate(&release, newest)?;
    Ok(Some(release))
}

/// An override serves mirrors and tests. Plain HTTP is accepted only for
/// loopback so skill content cannot be replaced in transit.
fn source() -> anyhow::Result<String> {
    let source = std::env::var("SUBMILLI_SKILL_SOURCE").unwrap_or_else(|_| DEFAULT_SOURCE.into());
    let url = url::Url::parse(&source).context("invalid SUBMILLI_SKILL_SOURCE")?;
    let loopback = matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"));
    if url.scheme() != "https" && !(url.scheme() == "http" && loopback) {
        bail!("SUBMILLI_SKILL_SOURCE must use https");
    }
    Ok(source.trim_end_matches('/').to_owned())
}

fn read_limited(response: ureq::http::Response<ureq::Body>, limit: u64) -> anyhow::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    response
        .into_body()
        .into_reader()
        .take(limit + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        bail!("response exceeds {limit} bytes");
    }
    Ok(bytes)
}

/// Advertised refs end a tag name with a newline, or `^{}` for a peeled tag.
fn newest_tag(advertisement: &str) -> Option<u32> {
    advertisement
        .match_indices(TAG_PREFIX)
        .filter_map(|(index, _)| {
            let digits: String = advertisement[index + TAG_PREFIX.len()..]
                .chars()
                .take_while(char::is_ascii_digit)
                .collect();
            digits.parse().ok()
        })
        .max()
}

fn validate(release: &ReleaseFile, expected_version: u32) -> anyhow::Result<()> {
    if release.schema != 1 {
        bail!("unsupported skill release format; upgrade the CLI");
    }
    if release.version != expected_version {
        bail!("skill release version does not match its tag");
    }
    if release.files.len() > MAX_FILES || !release.files.contains_key("SKILL.md") {
        bail!("skill release has an unexpected file set");
    }
    if release.files.get(VERSION_FILE).map(|text| text.trim())
        != Some(&*expected_version.to_string())
    {
        bail!("skill release VERSION file does not match its tag");
    }
    for name in release.files.keys() {
        let plain = Path::new(name)
            .components()
            .all(|part| matches!(part, Component::Normal(_)));
        // Rejecting dot-prefixed parts also protects the installation receipt.
        if !plain || name.contains('\\') || name.split('/').any(|part| part.starts_with('.')) {
            bail!("skill release contains unsafe path {name:?}");
        }
    }
    Ok(())
}

fn parse_version(text: &str) -> Vec<u64> {
    text.split(['.', '-', '+'])
        .map_while(|part| part.parse().ok())
        .collect()
}

fn age(checked_at: u64) -> Duration {
    Duration::from_secs(now().saturating_sub(checked_at))
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

fn read_cache(path: &Path) -> Option<Cache> {
    serde_json::from_slice(&fs::read(path).ok()?).ok()
}

/// Best effort: an unwritable cache only means the next sync checks again.
fn write_cache(path: &Path, checked_at: u64, release: &Option<ReleaseFile>) {
    let cache = Cache {
        checked_at,
        release: release.clone(),
    };
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(bytes) = serde_json::to_vec(&cache) {
        let _ = fs::write(path, bytes);
    }
}
