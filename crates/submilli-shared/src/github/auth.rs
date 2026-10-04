//! Credentials for GitHub fetches: a token, where it came from, and what
//! happened to it during a fetch.
//!
//! The token is a bearer credential, so it never appears in `Debug` output or
//! in an error: messages name where the token came from instead. It is only
//! ever sent to the GitHub hosts in [`super::client`].

use std::fmt;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

/// What a token needs to fetch a package, for help text and error messages.
pub const TOKEN_PERMISSIONS: &str = "a fine-grained personal access token with Repository \
     permissions → Contents: Read-only on the package repositories (GitHub adds Metadata: \
     Read-only), or a classic token with the `repo` scope";

/// Longest token accepted; GitHub's are far shorter.
const MAX_TOKEN_LEN: usize = 1024;
/// Most of a token file read: a token with generous surrounding whitespace.
/// A path to something endless (`/dev/zero`) must not exhaust memory.
const MAX_TOKEN_FILE_BYTES: u64 = 64 * 1024;

/// A GitHub token. Visible ASCII only, so it can't inject into a header.
#[derive(Clone)]
pub struct GithubToken(String);

impl GithubToken {
    /// Validate a token as typed, pasted, or read from a file, dropping
    /// surrounding whitespace (a file written with `echo`, or a Kubernetes
    /// Secret volume, ends in a newline) and a byte-order mark some editors
    /// write.
    pub fn parse(raw: &str) -> Result<Self, TokenError> {
        let token = raw.trim_start_matches('\u{feff}').trim();
        if token.is_empty() {
            return Err(TokenError("the GitHub token is empty".into()));
        }
        if token.len() > MAX_TOKEN_LEN || !token.bytes().all(|b| b.is_ascii_graphic()) {
            return Err(TokenError(
                "that is not a GitHub token: expected one line of visible ASCII, like \
                 `github_pat_…` or `ghp_…`"
                    .into(),
            ));
        }
        Ok(Self(token.to_string()))
    }

    /// Read a token from `path`. Errors name the file, never its contents.
    pub fn read_file(path: &Path) -> Result<Self, TokenError> {
        let unreadable = |err: std::io::Error| {
            TokenError(format!(
                "reading the GitHub token file {}: {err}",
                path.display()
            ))
        };
        // A FIFO or device would block or never end; a token is a file.
        if !std::fs::metadata(path).map_err(unreadable)?.is_file() {
            return Err(TokenError(format!(
                "GitHub token file {} is not a regular file",
                path.display()
            )));
        }
        let mut bytes = Vec::new();
        std::fs::File::open(path)
            .and_then(|file| {
                file.take(MAX_TOKEN_FILE_BYTES.saturating_add(1))
                    .read_to_end(&mut bytes)
            })
            .map_err(unreadable)?;
        if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_TOKEN_FILE_BYTES {
            return Err(TokenError(format!(
                "GitHub token file {} is larger than a token could be",
                path.display()
            )));
        }
        let raw = String::from_utf8(bytes)
            .map_err(|_| TokenError(format!("GitHub token file {} is not text", path.display())))?;
        Self::parse(&raw).map_err(|TokenError(reason)| {
            TokenError(format!("GitHub token file {}: {reason}", path.display()))
        })
    }

    /// The token itself, for storing it or sending it to GitHub.
    pub fn secret(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for GithubToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("GithubToken(***)")
    }
}

/// A token that couldn't be read or isn't well-formed.
#[derive(Debug)]
pub struct TokenError(String);

impl fmt::Display for TokenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for TokenError {}

/// Where a fetch's token came from, or why there is none. Error messages use
/// it to say whose token failed and how to fix it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TokenSource {
    /// The CLI found no token.
    Absent,
    /// The CLI took the token from this environment variable.
    Env(&'static str),
    /// The CLI's stored token (`submilli github authenticate`).
    Stored,
    /// The GitHub CLI's token (`gh auth token`).
    GhCli,
    /// The server's `github_token_file`.
    ServerFile(PathBuf),
    /// The server has no `github_token_file`.
    ServerUnconfigured,
}

impl TokenSource {
    /// Whose token this is, for messages: "the GitHub token in `GH_TOKEN`".
    pub fn describe(&self) -> String {
        match self {
            TokenSource::Absent | TokenSource::ServerUnconfigured => "no GitHub token".into(),
            TokenSource::Env(name) => format!("the GitHub token in `{name}`"),
            TokenSource::Stored => {
                "the GitHub token stored by `submilli github authenticate`".into()
            }
            TokenSource::GhCli => "the GitHub CLI's token (`gh auth token`)".into(),
            TokenSource::ServerFile(path) => {
                format!("the server's GitHub token ({})", path.display())
            }
        }
    }

    /// How to supply a token that can read `owner`'s repositories, or a new
    /// one after GitHub rejected this one.
    pub fn how_to_authenticate(&self, owner: Option<&str>) -> String {
        let link = token_link(owner);
        match self {
            TokenSource::ServerFile(_) | TokenSource::ServerUnconfigured => format!(
                "give the server {TOKEN_PERMISSIONS}, in the file named by `github_token_file` \
                 in its config file (create one at {link})"
            ),
            TokenSource::Env(name) => format!(
                "set `{name}` to {TOKEN_PERMISSIONS} (create one at {link}), or unset it to use \
                 the token from `submilli github authenticate`"
            ),
            TokenSource::Absent | TokenSource::Stored | TokenSource::GhCli => format!(
                "run `submilli github authenticate` with {TOKEN_PERMISSIONS} (create one at \
                 {link})"
            ),
        }
    }
}

/// The token a fetch sends, and what GitHub made of it. One value spans a
/// whole install, including every dependency, so the flags report on all of
/// it.
#[derive(Debug)]
pub struct GithubAuth {
    token: Option<GithubToken>,
    source: TokenSource,
    rejected: AtomicBool,
    /// The owner of the first repository not found while no token was sent.
    unauthenticated_not_found: OnceLock<String>,
}

impl GithubAuth {
    pub fn new(token: Option<GithubToken>, source: TokenSource) -> Self {
        Self {
            token,
            source,
            rejected: AtomicBool::new(false),
            unauthenticated_not_found: OnceLock::new(),
        }
    }

    /// No token: public repositories only.
    pub fn anonymous() -> Self {
        Self::new(None, TokenSource::Absent)
    }

    /// The configured token, whether or not GitHub has rejected it.
    pub fn token(&self) -> Option<&GithubToken> {
        self.token.as_ref()
    }

    pub fn source(&self) -> &TokenSource {
        &self.source
    }

    /// GitHub rejected the token (expired or revoked), and fetches went on
    /// without it.
    pub fn token_rejected(&self) -> bool {
        self.rejected.load(Ordering::Relaxed)
    }

    /// The owner of a repository that was not found while no token was sent,
    /// so may be private.
    pub fn unauthenticated_not_found(&self) -> Option<&str> {
        self.unauthenticated_not_found.get().map(String::as_str)
    }

    /// The token to send, until GitHub rejects it.
    pub(super) fn active_token(&self) -> Option<&GithubToken> {
        self.token.as_ref().filter(|_| !self.token_rejected())
    }

    pub(super) fn mark_rejected(&self) {
        self.rejected.store(true, Ordering::Relaxed);
    }

    /// Note that `org/repo` was not found while no token was sent, so it may
    /// be private.
    pub(super) fn record_unauthenticated_not_found(&self, org: &str) {
        let _ = self.unauthenticated_not_found.set(org.to_string());
    }

    /// `org/repo`, or `commit` in it, answered 404. Without a token that may
    /// only mean private; with one, the token can't see it. A commit that
    /// doesn't exist answers the same way, so the message names it too.
    pub(super) fn not_found_message(
        &self,
        org: &str,
        repo: &str,
        commit: Option<&str>,
        sent_token: bool,
    ) -> String {
        let missing = commit.map_or_else(
            || format!("repository `{org}/{repo}`"),
            |commit| format!("commit `{commit}` in `{org}/{repo}`"),
        );
        if sent_token {
            return format!(
                "GitHub has no {missing} that {} can read: check the name, or grant the token \
                 Contents: Read-only on `{org}/{repo}`; a fine-grained token also reads nothing \
                 private until the organization approves it",
                self.source.describe()
            );
        }
        let fix = self.source.how_to_authenticate(Some(org));
        if self.token_rejected() {
            return format!(
                "GitHub rejected {} (expired or revoked), and there is no public {missing}; {fix}",
                self.source.describe()
            );
        }
        format!("GitHub has no public {missing}; if the repository is private, {fix}")
    }

    /// For an unauthenticated request that hit the rate limit: a token raises
    /// it. `None` once the configured token was rejected, which is reported
    /// on its own.
    pub(super) fn raise_limit_hint(&self) -> Option<&'static str> {
        if self.token_rejected() {
            return None;
        }
        Some(match self.source {
            TokenSource::ServerFile(_) | TokenSource::ServerUnconfigured => {
                "a GitHub token raises the limit (`github_token_file` in the server config)"
            }
            TokenSource::Absent
            | TokenSource::Env(_)
            | TokenSource::Stored
            | TokenSource::GhCli => {
                "a GitHub token raises the limit (`submilli github authenticate`)"
            }
        })
    }

    /// A 403 to a token that isn't SSO or a rate limit.
    pub(super) fn forbidden_message(&self, org: &str, repo: &str) -> String {
        format!(
            "GitHub refused {} for `{org}/{repo}`: grant it Contents: Read-only on `{org}/{repo}`, \
             or, if it has that, the organization may still have to approve the token, or may \
             forbid this kind of token or its expiry",
            self.source.describe()
        )
    }

    pub(super) fn sso_message(&self, org: &str, repo: &str, url: Option<&str>) -> String {
        let authorize = url.map_or_else(
            || "in the token's settings on GitHub (Configure SSO)".to_string(),
            |url| format!("at {url}"),
        );
        format!(
            "`{org}/{repo}` is in an organization that uses SAML single sign-on: authorize {} for \
             it {authorize}",
            self.source.describe()
        )
    }
}

/// A link to GitHub's new-token form with the name, owner, and Contents:
/// Read-only already filled in; the user only picks the repositories.
pub fn token_link(owner: Option<&str>) -> String {
    let mut link = String::from(
        "https://github.com/settings/personal-access-tokens/new?name=submilli&contents=read&expires_in=90",
    );
    if let Some(owner) = owner.filter(|owner| super::is_valid_owner(owner)) {
        link.push_str("&target_name=");
        link.push_str(owner);
    }
    link
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_trims_and_refuses_what_is_not_a_token() {
        assert_eq!(
            GithubToken::parse("  github_pat_abc\n").unwrap().secret(),
            "github_pat_abc"
        );
        assert!(GithubToken::parse(" \n").is_err());
        assert!(GithubToken::parse("ghp_a\r\nX-Injected: 1").is_err());
        assert!(GithubToken::parse("ghp a").is_err());
        assert!(GithubToken::parse(&"a".repeat(MAX_TOKEN_LEN + 1)).is_err());
        assert_eq!(
            GithubToken::parse("\u{feff}ghp_bom\r\n").unwrap().secret(),
            "ghp_bom"
        );
    }

    #[test]
    fn debug_never_shows_the_token() {
        let token = GithubToken::parse("ghp_secretvalue").unwrap();
        let auth = GithubAuth::new(Some(token.clone()), TokenSource::Stored);
        assert!(!format!("{token:?}").contains("secretvalue"));
        assert!(!format!("{auth:?}").contains("secretvalue"));
    }

    #[test]
    fn read_file_names_the_file_not_the_contents() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("token");
        std::fs::write(&path, "ghp_filetoken\n").unwrap();
        assert_eq!(
            GithubToken::read_file(&path).unwrap().secret(),
            "ghp_filetoken"
        );

        std::fs::write(&path, "ghp bad value").unwrap();
        let err = GithubToken::read_file(&path).unwrap_err().to_string();
        assert!(err.contains(&path.display().to_string()), "{err}");
        assert!(!err.contains("bad value"), "{err}");

        // Exactly the cap is read; one byte more is refused before decoding.
        let at_cap = usize::try_from(MAX_TOKEN_FILE_BYTES).unwrap();
        std::fs::write(&path, format!("ghp_x{}", " ".repeat(at_cap - 5))).unwrap();
        assert_eq!(GithubToken::read_file(&path).unwrap().secret(), "ghp_x");
        let mut over = vec![b' '; at_cap];
        over.extend_from_slice("é".as_bytes());
        std::fs::write(&path, over).unwrap();
        let huge = GithubToken::read_file(&path).unwrap_err().to_string();
        assert!(huge.contains("larger than a token"), "{huge}");
        let not_a_file = GithubToken::read_file(dir.path()).unwrap_err().to_string();
        assert!(not_a_file.contains("not a regular file"), "{not_a_file}");

        let missing = GithubToken::read_file(&dir.path().join("missing")).unwrap_err();
        assert!(missing.to_string().contains("missing"), "{missing}");
    }

    #[test]
    fn token_link_prefills_owner_and_contents_read() {
        let link = token_link(Some("acme"));
        assert!(link.starts_with("https://github.com/settings/personal-access-tokens/new?"));
        assert!(link.contains("contents=read"), "{link}");
        assert!(link.contains("target_name=acme"), "{link}");
        assert!(!token_link(None).contains("target_name"));
        // An owner that isn't a GitHub name can't add parameters to the form.
        assert!(!token_link(Some("a&contents=write")).contains("write"));
    }

    #[test]
    fn not_found_without_a_token_suggests_authenticating() {
        let auth = GithubAuth::anonymous();
        let message = auth.not_found_message("acme", "crm", None, false);
        assert!(
            message.contains("if the repository is private"),
            "{message}"
        );
        assert!(
            message.contains("submilli github authenticate"),
            "{message}"
        );

        let server = GithubAuth::new(None, TokenSource::ServerUnconfigured);
        let message = server.not_found_message("acme", "crm", None, false);
        assert!(message.contains("github_token_file"), "{message}");
    }

    #[test]
    fn not_found_with_a_token_names_its_source() {
        let token = GithubToken::parse("ghp_x").unwrap();
        let auth = GithubAuth::new(Some(token), TokenSource::Env("GH_TOKEN"));
        let message = auth.not_found_message("acme", "crm", None, true);
        assert!(message.contains("`GH_TOKEN`"), "{message}");
        assert!(message.contains("Contents: Read-only"), "{message}");
    }
}
