//! Fetch a Submilli package source from GitHub at a pinned commit.
//!
//! `submilli install <url>` and the server's package-install path both route
//! through here: parse a repo spec, resolve a ref to a concrete commit SHA, and
//! download + extract the repo source into a temp directory. Two transports:
//!
//! - HTTPS (`org/repo`, `github.com/org/repo`, `https://…`): anonymous GitHub
//!   API ref resolution plus a `codeload` tarball. Public repositories only.
//! - SSH (`git@github.com:org/repo.git`, `ssh://git@github.com/org/repo.git`,
//!   and the Submilli shorthand `git://github.com/org/repo`): the git protocol
//!   over SSH through libgit2, authenticated with the identity [`FetchAuth`]
//!   supplies. See [`ssh`].
//!
//! Network access is constrained to GitHub hosts, which keeps the fetch off
//! arbitrary hosts. Everything here is blocking; async callers wrap it in
//! `tokio::task::spawn_blocking`.

mod keys;
mod known_hosts;
mod ssh;

use std::fmt;
use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use tempfile::TempDir;

pub use keys::{LocalIdentities, PassphrasePrompt, ServerSshKey, SshFileError};
pub use known_hosts::KnownHosts;
pub use ssh::SSH_NOT_CONFIGURED_HINT;
pub use submilli_build::GithubTransport;

const USER_AGENT: &str = "submilli";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
/// Bounds a downloaded or built source tarball, which is held in memory, and
/// what one extracts to.
const MAX_SOURCE_BYTES: u64 = 512 * 1024 * 1024;
/// Bounds the entries a source tarball may hold.
const MAX_SOURCE_ENTRIES: u64 = 200_000;

/// A parsed GitHub repo reference: an `org/repo` pair, an optional git ref
/// (branch, tag, or commit SHA), and the transport the spec asked for. The ref
/// is resolved to a concrete SHA by [`GithubSpec::resolve`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GithubSpec {
    pub org: String,
    pub repo: String,
    pub git_ref: Option<String>,
    pub transport: GithubTransport,
}

/// A repo pinned to a concrete 40-hex commit SHA.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedRepo {
    pub org: String,
    pub repo: String,
    pub sha: String,
    pub transport: GithubTransport,
}

/// A fetched repo: the extracted source tree (held by a `TempDir` that is wiped
/// on drop) plus the commit it was pinned to.
pub struct FetchedRepo {
    pub dir: TempDir,
    pub resolved: ResolvedRepo,
}

/// The SSH identity an SSH fetch authenticates with. HTTPS fetches ignore it.
#[derive(Clone, Copy)]
pub enum FetchAuth<'a> {
    /// A server without `package_ssh_key_file`: SSH specs fail with
    /// [`GithubError::SshNotConfigured`].
    Unconfigured,
    /// The server's configured key file, checked against the server's GitHub
    /// host keys.
    Server {
        key: &'a ServerSshKey,
        known_hosts: &'a KnownHosts,
    },
    /// The local user's ssh-agent and default key files, checked against
    /// `~/.ssh/known_hosts`.
    Local {
        identities: &'a LocalIdentities<'a>,
        known_hosts: &'a KnownHosts,
    },
}

#[derive(Debug)]
pub enum GithubError {
    /// The spec couldn't be parsed, or named a non-GitHub host. Carries an
    /// LLM-actionable message naming the accepted forms.
    InvalidSpec(String),
    /// Resolving a ref to a SHA failed (network, HTTP status, or a body that
    /// wasn't a SHA).
    Resolve(String),
    /// Downloading or extracting the source failed.
    Download(String),
    /// No SSH identity was available, or GitHub rejected every one tried.
    Auth(String),
    /// An SSH spec reached a server with no SSH identity configured.
    SshNotConfigured(String),
    /// GitHub's SSH host key was unknown or did not match.
    HostKey(String),
}

impl fmt::Display for GithubError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GithubError::InvalidSpec(message)
            | GithubError::Resolve(message)
            | GithubError::Download(message)
            | GithubError::Auth(message)
            | GithubError::SshNotConfigured(message)
            | GithubError::HostKey(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for GithubError {}

type Result<T> = std::result::Result<T, GithubError>;

const ACCEPTED_FORMS: &str = "use `org/repo`, `github.com/org/repo`, or \
     `https://github.com/org/repo` for a public repo, or `git@github.com:org/repo.git`, \
     `ssh://git@github.com/org/repo.git`, or `git://github.com/org/repo` to fetch over SSH \
     (optionally `@<ref>`)";

/// Parse a repo spec into an `org/repo` + optional ref + transport. Accepts,
/// in order of forgiveness: `https://github.com/org/repo`,
/// `github.com/org/repo`, `org/repo`; the SSH forms `git@github.com:org/repo`,
/// `ssh://git@github.com[:22]/org/repo`, and `git://github.com/org/repo`
/// (Submilli shorthand for SSH — the native `git://` protocol is never used);
/// a trailing `.git`; a `@<ref>` or `#<ref>` suffix; and the browser URL forms
/// `.../tree/<ref>` and `.../commit/<sha>`. Any other host, SSH user, or port
/// is rejected — GitHub is the only supported source.
pub fn parse_spec(input: &str) -> Result<GithubSpec> {
    let trimmed = input.trim();
    let (s, transport) = split_transport(input, trimmed)?;

    // A `#<ref>` suffix wins; otherwise a `@<ref>` on the repo segment, or a
    // `/tree|/commit/<ref>` path tail, supplies the ref.
    let (path, ref_from_hash) = match s.split_once('#') {
        Some((path, git_ref)) => (path, non_empty(git_ref)),
        None => (s, None),
    };

    let segments: Vec<&str> = path.split('/').filter(|seg| !seg.is_empty()).collect();
    let [org, repo_seg, rest @ ..] = segments.as_slice() else {
        return Err(GithubError::InvalidSpec(format!(
            "`{input}` must name a repo as `org/repo` (optionally `@<ref>`)"
        )));
    };

    let (mut repo, ref_from_at) = match repo_seg.split_once('@') {
        Some((repo, git_ref)) => (repo, non_empty(git_ref)),
        None => (*repo_seg, None),
    };
    repo = repo.strip_suffix(".git").unwrap_or(repo);

    let ref_from_path = match rest {
        ["tree" | "commit", git_ref @ ..] if !git_ref.is_empty() => Some(git_ref.join("/")),
        [] => None,
        _ => {
            return Err(GithubError::InvalidSpec(format!(
                "`{input}` has an unexpected path; use `org/repo`, `org/repo/tree/<ref>`, or \
                 `org/repo/commit/<sha>`"
            )));
        }
    };

    let git_ref = ref_from_hash.or(ref_from_at).or(ref_from_path);

    if !is_valid_org(org) {
        return Err(GithubError::InvalidSpec(format!(
            "`{org}` is not a valid GitHub org/user (letters, digits, single hyphens)"
        )));
    }
    if !is_valid_repo(repo) {
        return Err(GithubError::InvalidSpec(format!(
            "`{repo}` is not a valid GitHub repo name (letters, digits, `.`, `_`, `-`)"
        )));
    }

    Ok(GithubSpec {
        org: (*org).to_string(),
        repo: repo.to_string(),
        git_ref,
        transport,
    })
}

/// Strip the scheme and host from `trimmed`, returning the `org/repo…` rest and
/// the transport the form selects.
fn split_transport<'s>(input: &str, trimmed: &'s str) -> Result<(&'s str, GithubTransport)> {
    if let Some(rest) = trimmed.strip_prefix("git@github.com:") {
        return Ok((rest, GithubTransport::Ssh));
    }
    if let Some(rest) = trimmed.strip_prefix("ssh://") {
        let rest = rest
            .strip_prefix("git@github.com/")
            .or_else(|| rest.strip_prefix("git@github.com:22/"))
            .ok_or_else(|| {
                GithubError::InvalidSpec(format!(
                    "`{input}` must connect as `git@github.com` on the default port; {ACCEPTED_FORMS}"
                ))
            })?;
        return Ok((rest, GithubTransport::Ssh));
    }
    if let Some(rest) = trimmed.strip_prefix("git://") {
        let rest = rest
            .strip_prefix("github.com/")
            .ok_or_else(|| not_github(input))?;
        return Ok((rest, GithubTransport::Ssh));
    }

    let mut s = trimmed;
    for scheme in ["https://", "http://"] {
        if let Some(rest) = s.strip_prefix(scheme) {
            s = rest;
            break;
        }
    }
    if let Some(rest) = s.strip_prefix("github.com/") {
        return Ok((rest, GithubTransport::Https));
    }
    // `user@host:path` (scp-like SSH) or `host/…` naming anything but GitHub.
    let first = s.split('/').next().unwrap_or_default();
    if first.contains('.') || first.contains(':') {
        return Err(not_github(input));
    }
    Ok((s, GithubTransport::Https))
}

fn not_github(input: &str) -> GithubError {
    GithubError::InvalidSpec(format!(
        "`{input}` is not a github.com source; {ACCEPTED_FORMS}"
    ))
}

impl GithubSpec {
    /// Resolve the spec's ref to a concrete commit SHA. A full 40-hex ref is
    /// used as-is (no request); any other ref — or none, meaning the default
    /// branch — is resolved through the spec's transport.
    pub fn resolve(&self, auth: &FetchAuth<'_>) -> Result<ResolvedRepo> {
        let sha = match &self.git_ref {
            Some(git_ref) if is_sha(git_ref) => git_ref.clone(),
            other => match self.transport {
                GithubTransport::Https => {
                    self.resolve_ref_https(other.as_deref().unwrap_or("HEAD"))?
                }
                GithubTransport::Ssh => {
                    ssh::resolve_ref(&self.org, &self.repo, other.as_deref(), auth)?
                }
            },
        };
        Ok(ResolvedRepo {
            org: self.org.clone(),
            repo: self.repo.clone(),
            sha,
            transport: self.transport,
        })
    }

    fn resolve_ref_https(&self, git_ref: &str) -> Result<String> {
        let url = format!(
            "https://api.github.com/repos/{}/{}/commits/{}",
            self.org, self.repo, git_ref
        );
        let resp = github_agent()
            .get(&url)
            .header("User-Agent", USER_AGENT)
            .header("Accept", "application/vnd.github.sha")
            .call()
            .map_err(|err| GithubError::Resolve(resolve_message(self, git_ref, &err)))?;
        let status = resp.status().as_u16();
        if status != 200 {
            return Err(GithubError::Resolve(format!(
                "GitHub returned HTTP {status} resolving {}/{} ref `{git_ref}`; check the ref \
                 exists, and for a private repo install over SSH with \
                 `git@github.com:{}/{}.git`",
                self.org, self.repo, self.org, self.repo
            )));
        }
        let sha = resp
            .into_body()
            .read_to_string()
            .map_err(|err| GithubError::Resolve(format!("reading SHA from GitHub: {err}")))?;
        let sha = sha.trim().to_string();
        if !is_sha(&sha) {
            return Err(GithubError::Resolve(format!(
                "GitHub returned `{sha}` for {}/{} ref `{git_ref}`, which is not a commit SHA",
                self.org, self.repo
            )));
        }
        Ok(sha)
    }
}

impl ResolvedRepo {
    /// The repo's canonical URL for this transport, for messages.
    pub fn display_url(&self) -> String {
        match self.transport {
            GithubTransport::Https => format!("github.com/{}/{}", self.org, self.repo),
            GithubTransport::Ssh => format!("git@github.com:{}/{}.git", self.org, self.repo),
        }
    }

    /// Download the repo source at the pinned SHA and extract it into a fresh
    /// temp directory. Network call.
    pub fn download(&self, auth: &FetchAuth<'_>) -> Result<TempDir> {
        Ok(self.download_with_hash(auth)?.0)
    }

    /// Like [`download`](Self::download), but also returns the source hash
    /// (`sha256:<hex>` of the source tarball) — the integrity anchor a
    /// dependency lockfile records. HTTPS hashes GitHub's codeload tarball; SSH
    /// hashes the deterministic tarball [`ssh`] builds from the commit tree, so
    /// the two transports record different hashes for the same commit.
    pub fn download_with_hash(&self, auth: &FetchAuth<'_>) -> Result<(TempDir, String)> {
        match self.transport {
            GithubTransport::Https => {
                let gzipped = self.download_tarball_https()?;
                let dir = extract_tarball(flate2::read::GzDecoder::new(gzipped.as_slice()))?;
                Ok((dir, sha256_hex(&gzipped)))
            }
            GithubTransport::Ssh => {
                let tar = ssh::download_tarball(&self.org, &self.repo, &self.sha, auth)?;
                let dir = extract_tarball(tar.as_slice())?;
                Ok((dir, sha256_hex(&tar)))
            }
        }
    }

    fn download_tarball_https(&self) -> Result<Vec<u8>> {
        let url = format!(
            "https://codeload.github.com/{}/{}/tar.gz/{}",
            self.org, self.repo, self.sha
        );
        let resp = github_agent()
            .get(&url)
            .header("User-Agent", USER_AGENT)
            .call()
            .map_err(|err| {
                GithubError::Download(format!(
                    "downloading {}/{} at {}: {err}",
                    self.org, self.repo, self.sha
                ))
            })?;
        let status = resp.status().as_u16();
        if status != 200 {
            return Err(GithubError::Download(format!(
                "codeload returned HTTP {status} for {}/{} at {}; for a private repo install \
                 over SSH with `git@github.com:{}/{}.git`",
                self.org, self.repo, self.sha, self.org, self.repo
            )));
        }
        let mut bytes = Vec::new();
        resp.into_body()
            .into_reader()
            .take(MAX_SOURCE_BYTES.saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(|err| GithubError::Download(format!("reading tarball: {err}")))?;
        if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_SOURCE_BYTES {
            return Err(GithubError::Download(format!(
                "{}/{} at {} exceeds {} MiB",
                self.org,
                self.repo,
                self.sha,
                MAX_SOURCE_BYTES / (1024 * 1024)
            )));
        }
        Ok(bytes)
    }
}

/// Parse, resolve, and download in one step.
pub fn fetch(input: &str, auth: &FetchAuth<'_>) -> Result<FetchedRepo> {
    let resolved = parse_spec(input)?.resolve(auth)?;
    let dir = resolved.download(auth)?;
    Ok(FetchedRepo { dir, resolved })
}

/// Extract an uncompressed tar stream into a fresh temp directory, dropping
/// the single `<repo>-<sha>/` directory every entry is wrapped in.
///
/// The tree is untrusted and later read by the build, so nothing may land or
/// resolve outside the directory: entry paths must be plain names, symlinks
/// must point inside the tree, no entry may replace an earlier one (on a
/// case-insensitive file system `Link` and `link` collide, and writing `link/x`
/// would follow a symlink `Link`), and every parent must canonicalize inside.
fn extract_tarball(reader: impl Read) -> Result<TempDir> {
    let download = |message: String| GithubError::Download(message);
    let tmp = tempfile::tempdir().map_err(|err| download(format!("creating temp dir: {err}")))?;
    let root = fs::canonicalize(tmp.path())
        .map_err(|err| download(format!("resolving temp dir: {err}")))?;
    let mut archive = tar::Archive::new(reader);
    let entries = archive
        .entries()
        .map_err(|err| download(format!("reading tarball: {err}")))?;
    // Bounds what a small compressed download may expand to.
    let mut unpacked: u64 = 0;
    let mut count: u64 = 0;
    for entry in entries {
        let mut entry = entry.map_err(|err| download(format!("reading tar entry: {err}")))?;
        count = count.saturating_add(1);
        unpacked = unpacked.saturating_add(entry.size());
        if unpacked > MAX_SOURCE_BYTES || count > MAX_SOURCE_ENTRIES {
            return Err(source_too_large());
        }
        let Some(out) = prepare_destination(&entry, tmp.path(), &root)? else {
            continue;
        };
        entry
            .unpack(&out)
            .map_err(|err| download(format!("extracting {}: {err}", out.display())))?;
    }
    Ok(tmp)
}

/// Where `entry` may be written under `dir` (whose canonical form is `root`),
/// enforcing [`extract_tarball`]'s rules; `None` for the `<repo>-<sha>/`
/// wrapper and pax global headers, which carry no file. Creates the entry's
/// parent directory, since where it resolves can only be checked once it
/// exists.
fn prepare_destination<R: Read>(
    entry: &tar::Entry<'_, R>,
    dir: &Path,
    root: &Path,
) -> Result<Option<PathBuf>> {
    let download = |message: String| GithubError::Download(message);
    let path = entry
        .path()
        .map_err(|err| download(format!("bad tar entry path: {err}")))?;
    let stripped: PathBuf = path.components().skip(1).collect();
    let kind = entry.header().entry_type();
    if stripped.as_os_str().is_empty() || kind.is_pax_global_extensions() {
        return Ok(None);
    }
    // `Path::starts_with` is lexical, so `..` must be rejected outright.
    if !stripped
        .components()
        .all(|component| matches!(component, Component::Normal(_)))
    {
        return Err(download(format!(
            "tar entry `{}` lies outside the extraction directory",
            stripped.display()
        )));
    }
    if !matches!(
        kind,
        tar::EntryType::Regular | tar::EntryType::Directory | tar::EntryType::Symlink
    ) {
        return Err(download(format!(
            "tar entry `{}` has unsupported type {kind:?}",
            stripped.display()
        )));
    }

    let out = dir.join(&stripped);
    let parent = out.parent().unwrap_or(dir);
    fs::create_dir_all(parent)
        .map_err(|err| download(format!("creating {}: {err}", parent.display())))?;
    let resolved = fs::canonicalize(parent)
        .map_err(|err| download(format!("resolving {}: {err}", parent.display())))?;
    let Ok(parent_in_root) = resolved.strip_prefix(root) else {
        return Err(download(format!(
            "tar entry `{}` is written through a symlink outside the extraction directory",
            stripped.display()
        )));
    };

    if kind == tar::EntryType::Symlink {
        let target = entry
            .link_name()
            .map_err(|err| download(format!("bad symlink target: {err}")))?
            .unwrap_or_default();
        if target.as_os_str().is_empty()
            || !symlink_stays_inside(parent_in_root.components().count(), &target)
        {
            return Err(download(format!(
                "symlink `{}` points to `{}`, outside the repository; package sources may only \
                 link within the repository, with `..` only at the start of the target",
                stripped.display(),
                target.display()
            )));
        }
    }
    if let Ok(existing) = fs::symlink_metadata(&out)
        && !(existing.is_dir() && kind == tar::EntryType::Directory)
    {
        return Err(download(format!(
            "`{}` collides with an earlier path in the repository; on a case-insensitive file \
             system (macOS, Windows) paths that differ only in case are the same file, so rename \
             one",
            stripped.display()
        )));
    }
    Ok(Some(out))
}

/// Whether a symlink in a directory `depth` levels below the tree root (as
/// resolved on disk) pointing at `target` stays inside the tree. `..` is only
/// allowed as a leading run that climbs no higher than the root: after a name,
/// it would step out of whatever that name resolves to, which may itself be a
/// symlink. Every symlink is held to this rule, so names inside the tree
/// resolve inside it too.
fn symlink_stays_inside(depth: usize, target: &Path) -> bool {
    let mut depth = depth;
    let mut descended = false;
    for component in target.components() {
        match component {
            Component::Normal(_) => descended = true,
            Component::CurDir => {}
            Component::ParentDir if descended => return false,
            Component::ParentDir => match depth.checked_sub(1) {
                Some(up) => depth = up,
                None => return false,
            },
            Component::RootDir | Component::Prefix(_) => return false,
        }
    }
    true
}

/// The [`RepoFetcher`](submilli_build::RepoFetcher) implementation backing the
/// recursive GitHub-dependency resolver: fetch a repo at a concrete SHA over
/// the transport its URL names, hash the source, and hand the extracted tree to
/// the build crate.
pub struct GithubRepoFetcher<'a> {
    pub auth: FetchAuth<'a>,
}

impl submilli_build::RepoFetcher for GithubRepoFetcher<'_> {
    fn fetch(
        &self,
        url: &str,
        sha: &str,
    ) -> std::result::Result<submilli_build::FetchedRepo, submilli_build::FetchError> {
        let spec =
            parse_spec(url).map_err(|err| submilli_build::FetchError::new(err.to_string()))?;
        let resolved = ResolvedRepo {
            org: spec.org,
            repo: spec.repo,
            sha: sha.to_string(),
            transport: spec.transport,
        };
        let (dir, source_hash) =
            resolved
                .download_with_hash(&self.auth)
                .map_err(|err| match err {
                    GithubError::SshNotConfigured(message) => {
                        submilli_build::FetchError::ssh_not_configured(message)
                    }
                    other => submilli_build::FetchError::new(other.to_string()),
                })?;
        Ok(submilli_build::FetchedRepo {
            org: resolved.org,
            repo: resolved.repo,
            root: dir.path().to_path_buf(),
            source_hash,
            transport: resolved.transport,
            keep_alive: Some(Box::new(dir)),
        })
    }
}

/// A source past [`MAX_SOURCE_BYTES`] or [`MAX_SOURCE_ENTRIES`], on either
/// transport.
fn source_too_large() -> GithubError {
    GithubError::Download(format!(
        "repository source exceeds {} MiB or {MAX_SOURCE_ENTRIES} entries",
        MAX_SOURCE_BYTES / (1024 * 1024)
    ))
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    use std::fmt::Write;

    let digest = Sha256::digest(bytes);
    let mut out = String::from("sha256:");
    for byte in digest {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

fn github_agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(REQUEST_TIMEOUT))
        .http_status_as_error(false)
        .build()
        .into()
}

fn resolve_message(spec: &GithubSpec, git_ref: &str, err: &ureq::Error) -> String {
    format!(
        "resolving {}/{} ref `{git_ref}`: {err}",
        spec.org, spec.repo
    )
}

fn non_empty(value: &str) -> Option<String> {
    (!value.is_empty()).then(|| value.to_string())
}

fn is_sha(value: &str) -> bool {
    value.len() == 40 && value.chars().all(|c| c.is_ascii_hexdigit())
}

fn is_valid_org(org: &str) -> bool {
    !org.is_empty()
        && org.len() <= 39
        && !org.starts_with('-')
        && !org.ends_with('-')
        && !org.contains("--")
        && org.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

fn is_valid_repo(repo: &str) -> bool {
    !repo.is_empty()
        && repo != "."
        && repo != ".."
        && repo
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(org: &str, repo: &str, git_ref: Option<&str>) -> GithubSpec {
        GithubSpec {
            org: org.to_string(),
            repo: repo.to_string(),
            git_ref: git_ref.map(str::to_string),
            transport: GithubTransport::Https,
        }
    }

    fn ssh_spec(org: &str, repo: &str, git_ref: Option<&str>) -> GithubSpec {
        GithubSpec {
            transport: GithubTransport::Ssh,
            ..spec(org, repo, git_ref)
        }
    }

    #[test]
    fn parses_shorthand_and_url_forms() {
        assert_eq!(parse_spec("org/repo").unwrap(), spec("org", "repo", None));
        assert_eq!(
            parse_spec("github.com/org/repo").unwrap(),
            spec("org", "repo", None)
        );
        assert_eq!(
            parse_spec("https://github.com/org/repo").unwrap(),
            spec("org", "repo", None)
        );
        assert_eq!(
            parse_spec("https://github.com/org/repo.git").unwrap(),
            spec("org", "repo", None)
        );
    }

    #[test]
    fn parses_refs_from_every_form() {
        assert_eq!(
            parse_spec("org/repo@main").unwrap(),
            spec("org", "repo", Some("main"))
        );
        assert_eq!(
            parse_spec("github.com/org/repo#v1.2.3").unwrap(),
            spec("org", "repo", Some("v1.2.3"))
        );
        assert_eq!(
            parse_spec("https://github.com/org/repo/tree/feature/x").unwrap(),
            spec("org", "repo", Some("feature/x"))
        );
        let sha = "0123456789abcdef0123456789abcdef01234567";
        assert_eq!(
            parse_spec(&format!("org/repo/commit/{sha}")).unwrap(),
            spec("org", "repo", Some(sha))
        );
    }

    #[test]
    fn parses_ssh_forms_with_refs() {
        assert_eq!(
            parse_spec("git@github.com:org/repo.git").unwrap(),
            ssh_spec("org", "repo", None)
        );
        assert_eq!(
            parse_spec("git@github.com:org/repo.git@v1.2.3").unwrap(),
            ssh_spec("org", "repo", Some("v1.2.3"))
        );
        assert_eq!(
            parse_spec("ssh://git@github.com/org/repo.git#main").unwrap(),
            ssh_spec("org", "repo", Some("main"))
        );
        assert_eq!(
            parse_spec("ssh://git@github.com:22/org/repo").unwrap(),
            ssh_spec("org", "repo", None)
        );
        assert_eq!(
            parse_spec("git://github.com/org/repo@feature").unwrap(),
            ssh_spec("org", "repo", Some("feature"))
        );
    }

    #[test]
    fn rejects_non_github_ssh_targets() {
        for input in [
            "git@gitlab.com:org/repo.git",
            "deploy@github.com:org/repo.git",
            "ssh://deploy@github.com/org/repo.git",
            "ssh://git@github.com:2222/org/repo.git",
            "ssh://git@gitlab.com/org/repo.git",
            "git://gitlab.com/org/repo",
        ] {
            assert!(
                matches!(parse_spec(input), Err(GithubError::InvalidSpec(_))),
                "{input} should be rejected"
            );
        }
    }

    #[test]
    fn rejects_non_github_hosts() {
        assert!(matches!(
            parse_spec("https://gitlab.com/org/repo"),
            Err(GithubError::InvalidSpec(_))
        ));
        assert!(matches!(
            parse_spec("example.com/org/repo"),
            Err(GithubError::InvalidSpec(_))
        ));
    }

    #[test]
    fn rejects_missing_repo() {
        assert!(matches!(
            parse_spec("org"),
            Err(GithubError::InvalidSpec(_))
        ));
    }

    #[test]
    fn full_sha_resolves_without_network() {
        let sha = "0123456789abcdef0123456789abcdef01234567";
        for input in [
            format!("org/repo@{sha}"),
            format!("git@github.com:org/repo@{sha}"),
        ] {
            let resolved = parse_spec(&input)
                .unwrap()
                .resolve(&FetchAuth::Unconfigured)
                .unwrap();
            assert_eq!(resolved.sha, sha);
            assert_eq!(resolved.org, "org");
            assert_eq!(resolved.repo, "repo");
        }
    }

    #[test]
    fn extraction_rejects_parent_components() {
        let mut builder = tar::Builder::new(Vec::new());
        let mut header = tar::Header::new_gnu();
        header.set_size(1);
        header.set_mode(0o644);
        // `append_data` refuses `..`, so write the raw name into the header.
        let name = b"repo-sha/a/../../escape";
        header.as_gnu_mut().unwrap().name[..name.len()].copy_from_slice(name);
        header.set_cksum();
        builder.append(&header, &b"x"[..]).unwrap();
        let bytes = builder.into_inner().unwrap();
        assert!(matches!(
            extract_tarball(bytes.as_slice()),
            Err(GithubError::Download(message)) if message.contains("lies outside")
        ));
    }

    /// A tar of `(path, entry)` under `repo-sha/`, built header by header so
    /// tests can write entries the builder's own checks would refuse.
    fn tarball(entries: &[(&str, TarEntry)]) -> Vec<u8> {
        let mut builder = tar::Builder::new(Vec::new());
        for (path, entry) in entries {
            let mut header = tar::Header::new_gnu();
            let name = format!("repo-sha/{path}");
            header.as_gnu_mut().unwrap().name[..name.len()].copy_from_slice(name.as_bytes());
            header.set_mode(0o644);
            match entry {
                TarEntry::File(content) => {
                    header.set_size(content.len() as u64);
                    header.set_cksum();
                    builder.append(&header, content.as_bytes()).unwrap();
                }
                TarEntry::Symlink(target) | TarEntry::HardLink(target) => {
                    header.set_entry_type(if matches!(entry, TarEntry::Symlink(_)) {
                        tar::EntryType::Symlink
                    } else {
                        tar::EntryType::Link
                    });
                    header.set_size(0);
                    header.as_gnu_mut().unwrap().linkname[..target.len()]
                        .copy_from_slice(target.as_bytes());
                    header.set_cksum();
                    builder.append(&header, &b""[..]).unwrap();
                }
            }
        }
        builder.into_inner().unwrap()
    }

    enum TarEntry {
        File(&'static str),
        Symlink(&'static str),
        HardLink(&'static str),
    }

    fn extract_err(entries: &[(&str, TarEntry)]) -> String {
        match extract_tarball(tarball(entries).as_slice()) {
            Err(GithubError::Download(message)) => message,
            Err(other) => panic!("unexpected error kind: {other}"),
            Ok(_) => panic!("extraction should have been refused"),
        }
    }

    #[test]
    fn extraction_keeps_symlinks_inside_the_tree() {
        let tree = extract_tarball(
            tarball(&[
                ("docs/readme.md", TarEntry::File("# pkg\n")),
                ("src/readme.md", TarEntry::Symlink("../docs/readme.md")),
                ("here", TarEntry::Symlink(".")),
            ])
            .as_slice(),
        )
        .unwrap();
        assert_eq!(
            fs::read_to_string(tree.path().join("src/readme.md")).unwrap(),
            "# pkg\n"
        );

        let absolute = extract_err(&[("docs/readme.md", TarEntry::Symlink("/etc/passwd"))]);
        assert!(absolute.contains("outside the repository"), "{absolute}");
        let climbing = extract_err(&[("docs/readme.md", TarEntry::Symlink("../../etc/passwd"))]);
        assert!(climbing.contains("outside the repository"), "{climbing}");
        // `b` resolves to the root, so `b/..` would be its parent: `..` after a
        // name is refused because the name may itself be a symlink.
        let through_link = extract_err(&[
            ("b", TarEntry::Symlink(".")),
            ("a", TarEntry::Symlink("b/..")),
        ]);
        assert!(
            through_link.contains("outside the repository"),
            "{through_link}"
        );
    }

    #[test]
    fn extraction_refuses_hard_links_and_duplicate_entries() {
        let hard = extract_err(&[
            ("a", TarEntry::File("x")),
            ("b", TarEntry::HardLink("repo-sha/a")),
        ]);
        assert!(hard.contains("unsupported type"), "{hard}");
        // A second entry at the same path would write through whatever the
        // first one was; on a case-insensitive file system `A` and `a` collide.
        let duplicate = extract_err(&[("a", TarEntry::Symlink(".")), ("a", TarEntry::File("x"))]);
        assert!(duplicate.contains("collides"), "{duplicate}");
    }

    /// A symlink created inside a directory reached through another symlink is
    /// judged from where it really lands, not from its spelled path.
    #[test]
    fn symlink_depth_follows_the_real_parent() {
        // `d/up` resolves to the root, so `d/up/x -> ..` would leave the tree.
        let escape = extract_err(&[
            ("d/keep", TarEntry::File("x")),
            ("d/up", TarEntry::Symlink("..")),
            ("d/up/x", TarEntry::Symlink("..")),
        ]);
        assert!(escape.contains("outside the repository"), "{escape}");
    }

    /// A dependency fetch on a server without an SSH key is a configuration
    /// error, not a failed fetch, all the way to the resolver.
    #[test]
    fn dependency_fetch_without_ssh_identity_reports_unconfigured() {
        use submilli_build::RepoFetcher;
        let fetcher = GithubRepoFetcher {
            auth: FetchAuth::Unconfigured,
        };
        let Err(err) = fetcher.fetch(
            "git@github.com:acme/private.git",
            "0123456789abcdef0123456789abcdef01234567",
        ) else {
            panic!("fetch should fail without an identity");
        };
        assert!(err.missing_ssh_identity, "{err}");
        assert!(err.message.contains("package_ssh_key_file"), "{err}");
    }

    #[test]
    fn symlink_depth_rules() {
        assert!(symlink_stays_inside(0, Path::new("a/b")));
        assert!(symlink_stays_inside(2, Path::new("../../a")));
        assert!(!symlink_stays_inside(1, Path::new("../../a")));
        assert!(!symlink_stays_inside(3, Path::new("a/../b")));
        assert!(!symlink_stays_inside(3, Path::new("/a")));
    }
}
