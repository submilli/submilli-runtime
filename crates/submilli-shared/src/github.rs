//! Fetch a Submilli package source from GitHub at a pinned commit.
//!
//! `submilli install <url>` and the server's package-install path both route
//! through here: parse a repo spec, resolve a ref to a concrete commit SHA, and
//! download + extract the repo tarball into a temp directory. Network access is
//! constrained to GitHub hosts (`github.com`, `api.github.com`, `codeload`),
//! which keeps the fetch off arbitrary hosts. Everything here is blocking
//! (`ureq`); async callers wrap it in `tokio::task::spawn_blocking`.

use std::fmt;
use std::fs;
use std::io::Read;
use std::path::PathBuf;
use std::time::Duration;

use tempfile::TempDir;

const USER_AGENT: &str = "submilli";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

/// A parsed GitHub repo reference: an `org/repo` pair plus an optional git ref
/// (branch, tag, or commit SHA). The ref is resolved to a concrete SHA by
/// [`GithubSpec::resolve`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GithubSpec {
    pub org: String,
    pub repo: String,
    pub git_ref: Option<String>,
}

/// A repo pinned to a concrete 40-hex commit SHA.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedRepo {
    pub org: String,
    pub repo: String,
    pub sha: String,
}

/// A fetched repo: the extracted source tree (held by a `TempDir` that is wiped
/// on drop) plus the commit it was pinned to.
pub struct FetchedRepo {
    pub dir: TempDir,
    pub resolved: ResolvedRepo,
}

#[derive(Debug)]
pub enum GithubError {
    /// The spec couldn't be parsed, or named a non-GitHub host. Carries an
    /// LLM-actionable message naming the accepted forms.
    InvalidSpec(String),
    /// Resolving a ref to a SHA failed (network, HTTP status, or a body that
    /// wasn't a SHA).
    Resolve(String),
    /// Downloading or extracting the tarball failed.
    Download(String),
}

impl fmt::Display for GithubError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GithubError::InvalidSpec(message)
            | GithubError::Resolve(message)
            | GithubError::Download(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for GithubError {}

type Result<T> = std::result::Result<T, GithubError>;

/// Parse a repo spec into an `org/repo` + optional ref. Accepts, in order of
/// forgiveness: `https://github.com/org/repo`, `github.com/org/repo`,
/// `org/repo`; a trailing `.git`; a `@<ref>` or `#<ref>` suffix; and the
/// browser URL forms `.../tree/<ref>` and `.../commit/<sha>`. Any other host is
/// rejected — GitHub is the only supported source.
pub fn parse_spec(input: &str) -> Result<GithubSpec> {
    let trimmed = input.trim();
    let mut s = trimmed;
    for scheme in ["https://", "http://", "git://"] {
        if let Some(rest) = s.strip_prefix(scheme) {
            s = rest;
            break;
        }
    }

    if let Some(rest) = s.strip_prefix("github.com/") {
        s = rest;
    } else if s.split('/').next().is_some_and(|first| first.contains('.')) {
        return Err(GithubError::InvalidSpec(format!(
            "`{input}` is not a github.com source; use `org/repo`, `github.com/org/repo`, or \
             `https://github.com/org/repo` (optionally `@<ref>`)"
        )));
    }

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
    })
}

impl GithubSpec {
    /// Resolve the spec's ref to a concrete commit SHA. A full 40-hex ref is
    /// used as-is (no request); any other ref — or none, meaning the default
    /// branch — is resolved through the GitHub API. Network call unless the ref
    /// is already a SHA.
    pub fn resolve(&self) -> Result<ResolvedRepo> {
        let sha = match &self.git_ref {
            Some(git_ref) if is_sha(git_ref) => git_ref.clone(),
            other => self.resolve_ref(other.as_deref().unwrap_or("HEAD"))?,
        };
        Ok(ResolvedRepo {
            org: self.org.clone(),
            repo: self.repo.clone(),
            sha,
        })
    }

    fn resolve_ref(&self, git_ref: &str) -> Result<String> {
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
                "GitHub returned HTTP {status} resolving {}/{} ref `{git_ref}`; check the repo is \
                 public and the ref exists",
                self.org, self.repo
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
    /// Download the repo tarball at the pinned SHA from codeload and extract it
    /// into a fresh temp directory, stripping GitHub's `<repo>-<sha>/` prefix.
    /// Network call.
    pub fn download(&self) -> Result<TempDir> {
        Ok(self.download_with_hash()?.0)
    }

    /// Like [`download`](Self::download), but also returns the source hash
    /// (`sha256:<hex>` of the downloaded tarball) — the integrity anchor a
    /// dependency lockfile records.
    pub fn download_with_hash(&self) -> Result<(TempDir, String)> {
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
                "codeload returned HTTP {status} for {}/{} at {}",
                self.org, self.repo, self.sha
            )));
        }
        let mut bytes = Vec::new();
        resp.into_body()
            .into_reader()
            .read_to_end(&mut bytes)
            .map_err(|err| GithubError::Download(format!("reading tarball: {err}")))?;
        let source_hash = sha256_hex(&bytes);
        let dir = extract_tarball(bytes.as_slice())?;
        Ok((dir, source_hash))
    }
}

/// Parse, resolve, and download in one step.
pub fn fetch(input: &str) -> Result<FetchedRepo> {
    let resolved = parse_spec(input)?.resolve()?;
    let dir = resolved.download()?;
    Ok(FetchedRepo { dir, resolved })
}

fn extract_tarball(reader: impl Read) -> Result<TempDir> {
    let tmp = tempfile::tempdir()
        .map_err(|err| GithubError::Download(format!("creating temp dir: {err}")))?;
    let gz = flate2::read::GzDecoder::new(reader);
    let mut archive = tar::Archive::new(gz);
    let entries = archive
        .entries()
        .map_err(|err| GithubError::Download(format!("reading tarball: {err}")))?;
    for entry in entries {
        let mut entry =
            entry.map_err(|err| GithubError::Download(format!("reading tar entry: {err}")))?;
        let path = entry
            .path()
            .map_err(|err| GithubError::Download(format!("bad tar entry path: {err}")))?
            .into_owned();
        // GitHub wraps everything under a single `<repo>-<sha>/` directory; drop
        // it so the extracted tree starts at the repo root.
        let stripped: PathBuf = path.components().skip(1).collect();
        if stripped.as_os_str().is_empty() {
            continue;
        }
        let out = tmp.path().join(&stripped);
        if !out.starts_with(tmp.path()) {
            return Err(GithubError::Download(format!(
                "tar entry `{}` escapes the extraction directory",
                stripped.display()
            )));
        }
        if let Some(parent) = out.parent() {
            fs::create_dir_all(parent).map_err(|err| {
                GithubError::Download(format!("creating {}: {err}", parent.display()))
            })?;
        }
        entry
            .unpack(&out)
            .map_err(|err| GithubError::Download(format!("extracting {}: {err}", out.display())))?;
    }
    Ok(tmp)
}

/// The [`RepoFetcher`](submilli_build::RepoFetcher) implementation backing the
/// recursive GitHub-dependency resolver: fetch a repo at a concrete SHA,
/// download + hash its tarball, and hand the extracted tree to the build crate.
pub struct GithubRepoFetcher;

impl submilli_build::RepoFetcher for GithubRepoFetcher {
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
        };
        let (dir, source_hash) = resolved
            .download_with_hash()
            .map_err(|err| submilli_build::FetchError::new(err.to_string()))?;
        Ok(submilli_build::FetchedRepo {
            org: resolved.org,
            repo: resolved.repo,
            root: dir.path().to_path_buf(),
            source_hash,
            keep_alive: Some(Box::new(dir)),
        })
    }
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
        let resolved = parse_spec(&format!("org/repo@{sha}"))
            .unwrap()
            .resolve()
            .unwrap();
        assert_eq!(resolved.sha, sha);
        assert_eq!(resolved.org, "org");
        assert_eq!(resolved.repo, "repo");
    }
}
