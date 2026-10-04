//! Fetch a Submilli package source from GitHub at a pinned commit.
//!
//! `submilli install <url>` and the server's package-install path both route
//! through here: parse a repo spec, resolve a ref to a concrete commit SHA, and
//! download + extract the repo tarball into a temp directory. Requests go only
//! to `api.github.com` (refs) and `codeload.github.com` (tarballs), which keeps
//! the fetch off arbitrary hosts. A [`GithubAuth`] token, when there is one, is
//! sent to those hosts alone and reaches private repositories; see
//! [`client`]. Everything here is blocking (`ureq`); async callers wrap it in
//! `tokio::task::spawn_blocking`.

mod auth;
mod client;
mod extract;
#[cfg(test)]
mod http_tests;
mod identity;

use std::fmt;
use std::io::Read;

use tempfile::TempDir;

pub use auth::{GithubAuth, GithubToken, TOKEN_PERMISSIONS, TokenError, TokenSource, token_link};
use client::{Endpoints, Target};
use extract::{MAX_SOURCE_BYTES, extract_tarball, source_too_large};
pub use identity::{TokenIdentity, token_identity};

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
    /// The spec couldn't be parsed, named a non-GitHub host, or names a ref
    /// the repository doesn't have. Carries an LLM-actionable message.
    InvalidSpec(String),
    /// Resolving a ref to a SHA failed (network, HTTP status, or a body that
    /// wasn't a SHA).
    Resolve(String),
    /// Downloading or extracting the tarball failed.
    Download(String),
    /// GitHub didn't let the token (or no token) read the repository: not
    /// found, private, or blocked by SAML single sign-on. The message says how
    /// to authenticate.
    Access(String),
    /// GitHub's rate limit is used up.
    RateLimited(String),
}

impl fmt::Display for GithubError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GithubError::InvalidSpec(message)
            | GithubError::Resolve(message)
            | GithubError::Download(message)
            | GithubError::Access(message)
            | GithubError::RateLimited(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for GithubError {}

impl GithubError {
    /// Whether this was a credential or rate-limit problem, for a caller that
    /// answers those differently from a failed transfer.
    pub fn kind(&self) -> submilli_build::FetchErrorKind {
        use submilli_build::FetchErrorKind;
        match self {
            GithubError::Access(_) => FetchErrorKind::Access,
            GithubError::RateLimited(_) => FetchErrorKind::RateLimited,
            GithubError::InvalidSpec(_) | GithubError::Resolve(_) | GithubError::Download(_) => {
                FetchErrorKind::Failed
            }
        }
    }
}

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

    if !is_valid_owner(org) {
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
    pub fn resolve(&self, auth: &GithubAuth) -> Result<ResolvedRepo> {
        self.resolve_at(&Endpoints::github(), auth)
    }

    fn resolve_at(&self, endpoints: &Endpoints, auth: &GithubAuth) -> Result<ResolvedRepo> {
        // The ref becomes part of a URL the token is sent to, so it may only
        // name a ref: `..`, `?`, or `%` would point the request elsewhere.
        if let Some(git_ref) = &self.git_ref
            && !is_sha(git_ref)
            && !is_valid_ref(git_ref)
        {
            return Err(GithubError::InvalidSpec(format!(
                "`{git_ref}` is not a git branch, tag, or commit SHA"
            )));
        }
        let sha = match &self.git_ref {
            Some(git_ref) if is_sha(git_ref) => git_ref.clone(),
            other => self.resolve_ref(endpoints, auth, other.as_deref().unwrap_or("HEAD"))?,
        };
        Ok(ResolvedRepo {
            org: self.org.clone(),
            repo: self.repo.clone(),
            sha,
        })
    }

    fn resolve_ref(
        &self,
        endpoints: &Endpoints,
        auth: &GithubAuth,
        git_ref: &str,
    ) -> Result<String> {
        let url = format!(
            "{}/repos/{}/{}/commits/{}",
            endpoints.api, self.org, self.repo, git_ref
        );
        let action = format!("resolving {}/{} ref `{git_ref}`", self.org, self.repo);
        let target = Target {
            org: &self.org,
            repo: &self.repo,
            commit: None,
            action: &action,
            fail: GithubError::Resolve,
        };
        let response = client::get(&url, Some("application/vnd.github.sha"), auth, &target)?;
        let sha = response
            .into_body()
            .read_to_string()
            .map_err(|err| GithubError::Resolve(format!("{action}: reading the SHA: {err}")))?;
        let sha = sha.trim().to_string();
        // The body isn't echoed: it came back for a request carrying a token.
        if !is_sha(&sha) {
            return Err(GithubError::Resolve(format!(
                "{action}: GitHub answered with something other than a commit SHA"
            )));
        }
        Ok(sha)
    }
}

impl ResolvedRepo {
    /// Download the repo tarball at the pinned SHA from codeload and extract it
    /// into a fresh temp directory, stripping GitHub's `<repo>-<sha>/` prefix.
    /// Network call.
    pub fn download(&self, auth: &GithubAuth) -> Result<TempDir> {
        Ok(self.download_with_hash(auth)?.0)
    }

    /// Like [`download`](Self::download), but also returns the source hash
    /// (`sha256:<hex>` of the downloaded tarball) — the integrity anchor a
    /// dependency lockfile records. A token doesn't change the tarball, so the
    /// hash is the same with or without one.
    pub fn download_with_hash(&self, auth: &GithubAuth) -> Result<(TempDir, String)> {
        self.download_at(&Endpoints::github(), auth)
    }

    fn download_at(&self, endpoints: &Endpoints, auth: &GithubAuth) -> Result<(TempDir, String)> {
        let gzipped = self.download_tarball(endpoints, auth)?;
        let dir = extract_tarball(flate2::read::GzDecoder::new(gzipped.as_slice()))?;
        Ok((dir, sha256_hex(&gzipped)))
    }

    fn download_tarball(&self, endpoints: &Endpoints, auth: &GithubAuth) -> Result<Vec<u8>> {
        // codeload, not the API's `/tarball` route: that one serves a
        // differently laid out archive, whose hash wouldn't match lockfiles.
        let url = format!(
            "{}/{}/{}/tar.gz/{}",
            endpoints.codeload, self.org, self.repo, self.sha
        );
        let action = format!("downloading {}/{} at {}", self.org, self.repo, self.sha);
        let target = Target {
            org: &self.org,
            repo: &self.repo,
            commit: Some(&self.sha),
            action: &action,
            fail: GithubError::Download,
        };
        let response = client::get(&url, None, auth, &target)?;
        let mut bytes = Vec::new();
        response
            .into_body()
            .into_reader()
            .take(MAX_SOURCE_BYTES.saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(|err| {
                GithubError::Download(format!("{action}: reading the tarball: {err}"))
            })?;
        if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_SOURCE_BYTES {
            return Err(source_too_large());
        }
        Ok(bytes)
    }
}

/// Parse, resolve, and download in one step.
pub fn fetch(input: &str, auth: &GithubAuth) -> Result<FetchedRepo> {
    let resolved = parse_spec(input)?.resolve(auth)?;
    let dir = resolved.download(auth)?;
    Ok(FetchedRepo { dir, resolved })
}

/// The [`RepoFetcher`](submilli_build::RepoFetcher) implementation backing the
/// recursive GitHub-dependency resolver: fetch a repo at a concrete SHA,
/// download + hash its tarball, and hand the extracted tree to the build crate.
pub struct GithubRepoFetcher<'a> {
    auth: &'a GithubAuth,
    endpoints: Endpoints,
}

impl<'a> GithubRepoFetcher<'a> {
    pub fn new(auth: &'a GithubAuth) -> Self {
        Self {
            auth,
            endpoints: Endpoints::github(),
        }
    }
}

impl submilli_build::RepoFetcher for GithubRepoFetcher<'_> {
    fn fetch(
        &self,
        url: &str,
        sha: &str,
    ) -> std::result::Result<submilli_build::FetchedRepo, submilli_build::FetchError> {
        let spec = parse_spec(url).map_err(fetch_error)?;
        let resolved = ResolvedRepo {
            org: spec.org,
            repo: spec.repo,
            sha: sha.to_string(),
        };
        let (dir, source_hash) = resolved
            .download_at(&self.endpoints, self.auth)
            .map_err(fetch_error)?;
        Ok(submilli_build::FetchedRepo {
            org: resolved.org,
            repo: resolved.repo,
            root: dir.path().to_path_buf(),
            source_hash,
            keep_alive: Some(Box::new(dir)),
        })
    }
}

/// A dependency fetch failure for the resolver, keeping whether it was a
/// credential or rate-limit problem so the server can answer with the same
/// code as for the repository itself.
fn fetch_error(err: GithubError) -> submilli_build::FetchError {
    submilli_build::FetchError::with_kind(err.kind(), err.to_string())
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

fn non_empty(value: &str) -> Option<String> {
    (!value.is_empty()).then(|| value.to_string())
}

fn is_sha(value: &str) -> bool {
    value.len() == 40 && value.chars().all(|c| c.is_ascii_hexdigit())
}

/// Whether `org` is a GitHub user or organization name.
pub fn is_valid_owner(org: &str) -> bool {
    !org.is_empty()
        && org.len() <= 39
        && !org.starts_with('-')
        && !org.ends_with('-')
        && !org.contains("--")
        && org.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// A ref name `git check-ref-format` accepts, limited to visible ASCII and
/// free of the characters a URL path would read as structure (`?`, `#`, `%`).
fn is_valid_ref(git_ref: &str) -> bool {
    git_ref.len() <= 255
        && !git_ref.is_empty()
        && git_ref
            .bytes()
            .all(|b| b.is_ascii_graphic() && !b"~^:?*[\\%#".contains(&b))
        && !git_ref.contains("..")
        && !git_ref.contains("@{")
        && git_ref
            .split('/')
            .all(|part| !part.is_empty() && !part.starts_with('.') && !part.ends_with(".lock"))
        && !git_ref.ends_with('.')
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
            .resolve(&GithubAuth::anonymous())
            .unwrap();
        assert_eq!(resolved.sha, sha);
        assert_eq!(resolved.org, "org");
        assert_eq!(resolved.repo, "repo");
    }
}
