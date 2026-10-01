//! Host-key verification for SSH fetches, in OpenSSH `known_hosts` format.
//!
//! libgit2's own known_hosts check is bypassed: the `certificate_check`
//! callback hands every presented host key to [`KnownHosts::verify`], so the
//! server can verify against GitHub's published keys without a `~/.ssh`, and
//! the CLI against the user's `~/.ssh/known_hosts`. libgit2 still reads a
//! known_hosts file to choose which host key type to ask for; it gets one
//! written from [`KnownHosts::trusted_lines`] (see `ssh::Libgit2Guard`). An
//! unknown or mismatched key always fails — there is no trust-on-first-use.

use std::path::{Path, PathBuf};
use std::str::FromStr;

use hmac::{Hmac, Mac};
use ssh_key::known_hosts::{Entry, HostPatterns, Marker};
use ssh_key::public::KeyData;
use ssh_key::{HashAlg, PublicKey};

use super::keys::{SshFileError, read_bounded};

/// GitHub's SSH host keys, as published at
/// <https://api.github.com/meta> (`ssh_keys`) and in GitHub's documentation.
const GITHUB_HOST_KEYS: &str = "\
github.com ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOMqqnkVzrm0SdG6UOoqKLsabgH5C9okWi0dh2l9GKJl
github.com ecdsa-sha2-nistp256 AAAAE2VjZHNhLXNoYTItbmlzdHAyNTYAAAAIbmlzdHAyNTYAAABBBEmKSENjQEezOmxkZMy7opKgwFB9nkt5YRrYMjNuG5N87uRgg6CLrbo5wAdT/y6v0mKV0U2w0WZ2YB/++Tpockg=
github.com ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABgQCj7ndNxQowgcQnjshcLrqPEiiphnt+VTTvDP6mHBL9j1aNUkY4Ue1gvwnGLVlOhGeYrnZaMgRK6+PKCUXaDbC7qtbW8gIkhL7aGCsOr/C56SJMy/BCZfxd1nWzAOxSDPgVsmerOBYfNqltV9/hWCqBywINIR+5dIg6JTJ72pcEpEjcYgXkE2YEFXV1JHnsKgbLWNlhScqb2UmyRkQyytRLtL+38TGxkxCflmO+5Z8CSSNY7GidjMIZ7Q4zMjA2n1nGrlTDkzwDCsw+wqFPGQA179cnfGWOWRVruj16z6XyvxvjJwbz0wQZ75XK5tKSb7FNyeIEs4TT4jk+S4dhPeAUC5y+bDYirYgM4GC7uEnztnZyaVWQ7B381AK4Qdrwt51ZqExKbQpTUNn+EjqoTwvqNj4kqx5QUCI0ThS/YkOxJCXmPUWZbhjpCg56i+2aB6CmK2JGhn57K5mj0MNdBXA4/WnwH6XoPWJzK5Nyu2zB3nAZp+S5hpQs+p1vN1/wsjk=
";

const GITHUB_HOST: &str = "github.com";
const FINGERPRINTS_URL: &str = "https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/githubs-ssh-key-fingerprints";

/// Bounds a known_hosts file read; real files are a few kilobytes.
const MAX_KNOWN_HOSTS_BYTES: u64 = 4 * 1024 * 1024;

/// A set of trusted host keys plus where they came from, for messages.
#[derive(Clone, Debug)]
pub struct KnownHosts {
    entries: Vec<Entry>,
    source: Source,
}

#[derive(Clone, Debug)]
enum Source {
    BuiltinGithub,
    File(PathBuf),
}

impl KnownHosts {
    /// GitHub's published host keys, built into the binary. The server's
    /// default when `package_ssh_known_hosts_file` is unset.
    pub fn github_builtin() -> Self {
        Self {
            entries: parse_entries(GITHUB_HOST_KEYS),
            source: Source::BuiltinGithub,
        }
    }

    /// Load a known_hosts file that must exist (the server's override).
    /// Unparsable lines are skipped, as OpenSSH does. A file with no usable
    /// github.com key would refuse every fetch, so it is refused up front.
    pub fn load_file(path: &Path) -> Result<Self, SshFileError> {
        let text = read_bounded(path, MAX_KNOWN_HOSTS_BYTES).map_err(|err| {
            SshFileError::new(format!(
                "reading known_hosts file {}: {err}",
                path.display()
            ))
        })?;
        let hosts = Self {
            entries: parse_entries(&text),
            source: Source::File(path.to_path_buf()),
        };
        if !hosts.has_negotiable_key_for(GITHUB_HOST) {
            let kind = if cfg!(windows) { "RSA key" } else { "key" };
            return Err(SshFileError::new(format!(
                "known_hosts file {} has no {kind} for github.com; add GitHub's lines from \
                 {FINGERPRINTS_URL}",
                path.display()
            )));
        }
        Ok(hosts)
    }

    /// The local user's `~/.ssh/known_hosts`. A missing or unreadable file,
    /// or no home directory, yields an empty set, so every host is unknown
    /// and the fetch fails with a hint rather than trusting anything.
    pub fn user_default() -> Self {
        let Some(home) = std::env::home_dir() else {
            return Self {
                entries: Vec::new(),
                source: Source::File(PathBuf::from("~/.ssh/known_hosts")),
            };
        };
        let path = home.join(".ssh").join("known_hosts");
        let entries = read_bounded(&path, MAX_KNOWN_HOSTS_BYTES)
            .map(|text| parse_entries(&text))
            .unwrap_or_default();
        Self {
            entries,
            source: Source::File(path),
        }
    }

    /// The keys [`Self::verify`] would accept for `host` that this build can
    /// negotiate, as known_hosts lines naming the host exactly, for libgit2's
    /// host key preference.
    pub(crate) fn trusted_lines(&self, host: &str) -> String {
        self.negotiable_keys(host)
            .filter_map(|key| PublicKey::new(key.clone(), "").to_openssh().ok())
            .map(|key| format!("{host} {key}\n"))
            .collect()
    }

    /// Whether any key this build can negotiate would be accepted for `host`.
    pub(crate) fn has_negotiable_key_for(&self, host: &str) -> bool {
        self.negotiable_keys(host).next().is_some()
    }

    fn has_accepted_key_for(&self, host: &str) -> bool {
        self.accepted_keys(host).next().is_some()
    }

    /// Accepted keys of a type this build's libssh2 can negotiate. On
    /// Windows, its WinCNG backend has only RSA host keys, so asking for
    /// another type would fail before any key is checked.
    fn negotiable_keys<'a>(&'a self, host: &'a str) -> impl Iterator<Item = &'a KeyData> {
        self.accepted_keys(host)
            .filter(|key| !cfg!(windows) || matches!(key, KeyData::Rsa(_)))
    }

    /// Keys trusted for `host` and not revoked for it.
    fn accepted_keys<'a>(&'a self, host: &'a str) -> impl Iterator<Item = &'a KeyData> {
        let matching = move || {
            self.entries
                .iter()
                .filter(move |entry| host_matches(entry.host_patterns(), host))
        };
        let revoked: Vec<&KeyData> = matching()
            .filter(|entry| matches!(entry.marker(), Some(Marker::Revoked)))
            .map(|entry| entry.public_key().key_data())
            .collect();
        matching()
            .filter(|entry| entry.marker().is_none())
            .map(|entry| entry.public_key().key_data())
            .filter(move |key| !revoked.contains(key))
    }

    #[cfg(test)]
    pub(crate) fn from_text(text: &str, path: &str) -> Self {
        Self {
            entries: parse_entries(text),
            source: Source::File(PathBuf::from(path)),
        }
    }

    /// Check the raw host key blob a server presented for `host` (`github.com`,
    /// or `[host]:port` off port 22). Returns an actionable message on failure.
    pub(crate) fn verify(&self, host: &str, presented: &[u8]) -> Result<(), String> {
        let presented = PublicKey::from_bytes(presented)
            .map_err(|err| format!("{host} presented an unreadable SSH host key: {err}"))?;
        let presented_key = presented.key_data();
        let fingerprint = presented.fingerprint(HashAlg::Sha256);
        let revoked = self.entries.iter().any(|entry| {
            matches!(entry.marker(), Some(Marker::Revoked))
                && host_matches(entry.host_patterns(), host)
                && entry.public_key().key_data() == presented_key
        });
        if revoked {
            return Err(format!(
                "the SSH host key {host} presented ({fingerprint}) is marked @revoked in {}",
                self.describe_source()
            ));
        }
        if self.accepted_keys(host).any(|key| key == presented_key) {
            return Ok(());
        }
        if !self.has_accepted_key_for(host) {
            return Err(format!(
                "{host} is not a known SSH host in {}; {}",
                self.describe_source(),
                self.fix_hint()
            ));
        }
        if !self.has_negotiable_key_for(host) {
            return Err(format!(
                "{} has no RSA key for {host}, and on Windows only RSA host keys can be \
                 checked; add GitHub's ssh-rsa line with `ssh-keyscan -t rsa github.com`",
                self.describe_source()
            ));
        }
        Err(format!(
            "the SSH host key {host} presented ({fingerprint}) does not match {}; refusing to \
             connect. If GitHub rotated its keys, compare with {FINGERPRINTS_URL}; {}",
            self.describe_source(),
            self.fix_hint()
        ))
    }

    fn describe_source(&self) -> String {
        match &self.source {
            Source::BuiltinGithub => "the built-in GitHub host keys".to_string(),
            Source::File(path) => path.display().to_string(),
        }
    }

    fn fix_hint(&self) -> String {
        match &self.source {
            Source::BuiltinGithub => "set `package_ssh_known_hosts_file` to a known_hosts file \
                 with GitHub's current keys"
                .to_string(),
            Source::File(path) => format!(
                "add GitHub's published host keys with `ssh-keyscan github.com >> {}` after \
                 checking the fingerprints at {FINGERPRINTS_URL}",
                path.display()
            ),
        }
    }
}

fn parse_entries(text: &str) -> Vec<Entry> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| Entry::from_str(line).ok())
        .collect()
}

fn host_matches(patterns: &HostPatterns, host: &str) -> bool {
    match patterns {
        HostPatterns::HashedName { salt, hash } => hashed_name_matches(salt, hash, host),
        HostPatterns::Patterns(patterns) => {
            let mut matched = false;
            for pattern in patterns {
                if let Some(negated) = pattern.strip_prefix('!') {
                    if glob_matches(negated, host) {
                        return false;
                    }
                } else if glob_matches(pattern, host) {
                    matched = true;
                }
            }
            matched
        }
    }
}

fn hashed_name_matches(salt: &[u8], hash: &[u8; 20], host: &str) -> bool {
    let Ok(mut mac) = Hmac::<sha1::Sha1>::new_from_slice(salt) else {
        return false;
    };
    mac.update(host.as_bytes());
    mac.verify_slice(hash).is_ok()
}

/// OpenSSH host patterns: `*` matches any run, `?` one character, and
/// hostnames compare case-insensitively.
fn glob_matches(pattern: &str, host: &str) -> bool {
    let pattern: Vec<char> = pattern.to_ascii_lowercase().chars().collect();
    let host: Vec<char> = host.to_ascii_lowercase().chars().collect();
    let (mut p, mut h) = (0, 0);
    let mut backtrack: Option<(usize, usize)> = None;
    while h < host.len() {
        match pattern.get(p) {
            Some('*') => {
                backtrack = Some((p, h));
                p += 1;
            }
            Some(&c) if c == '?' || host.get(h) == Some(&c) => {
                p += 1;
                h += 1;
            }
            _ => match backtrack {
                Some((star, matched)) => {
                    p = star + 1;
                    h = matched + 1;
                    backtrack = Some((star, matched + 1));
                }
                None => return false,
            },
        }
    }
    pattern
        .get(p..)
        .is_some_and(|rest| rest.iter().all(|&c| c == '*'))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ED25519_B64: &str =
        "AAAAC3NzaC1lZDI1NTE5AAAAIOMqqnkVzrm0SdG6UOoqKLsabgH5C9okWi0dh2l9GKJl";

    fn github_ed25519_blob() -> Vec<u8> {
        PublicKey::from_openssh(&format!("ssh-ed25519 {ED25519_B64}"))
            .unwrap()
            .to_bytes()
            .unwrap()
    }

    fn other_blob() -> Vec<u8> {
        PublicKey::from_openssh(
            "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIHGa4yOXbfVDXHb2BhHQvNmGgBEVsjymlS7hZp3uNbRa",
        )
        .unwrap()
        .to_bytes()
        .unwrap()
    }

    #[test]
    fn builtin_keys_match_githubs_published_fingerprints() {
        let builtin = KnownHosts::github_builtin();
        let fingerprints: Vec<String> = builtin
            .entries
            .iter()
            .map(|entry| entry.public_key().fingerprint(HashAlg::Sha256).to_string())
            .collect();
        assert_eq!(
            fingerprints,
            [
                "SHA256:+DiY3wvvV6TuJJhbpZisF/zLDA0zPMSvHdkr4UvCOqU",
                "SHA256:p2QAMXNIC1TJYWeIOttrVc98/R1BUFWu3/LiyKgUfQM",
                "SHA256:uNiVztksCsDhcc0u9e8BujQXVUpKZIDTMczCvj3tD2s",
            ]
        );
        assert!(builtin.verify("github.com", &github_ed25519_blob()).is_ok());
    }

    #[test]
    fn plain_and_port_qualified_entries_match() {
        let hosts = KnownHosts::from_text(
            &format!(
                "github.com,140.82.112.3 ssh-ed25519 {ED25519_B64}\n[localhost]:2222 ssh-ed25519 {ED25519_B64}\n"
            ),
            "known_hosts",
        );
        assert!(hosts.verify("github.com", &github_ed25519_blob()).is_ok());
        assert!(
            hosts
                .verify("[localhost]:2222", &github_ed25519_blob())
                .is_ok()
        );
        assert!(hosts.verify("localhost", &github_ed25519_blob()).is_err());
    }

    #[test]
    fn hashed_entries_match() {
        use base64::Engine;

        // `ssh-keygen -H` form for github.com with salt "0123456789abcdef0123".
        let salt = b"0123456789abcdef0123";
        let mut mac = Hmac::<sha1::Sha1>::new_from_slice(salt).unwrap();
        mac.update(b"github.com");
        let hash = mac.finalize().into_bytes();
        let b64 = base64::engine::general_purpose::STANDARD;
        let line = format!(
            "|1|{}|{} ssh-ed25519 {ED25519_B64}",
            b64.encode(salt),
            b64.encode(hash)
        );
        let hosts = KnownHosts::from_text(&line, "known_hosts");
        assert!(hosts.verify("github.com", &github_ed25519_blob()).is_ok());
        assert!(hosts.verify("gitlab.com", &github_ed25519_blob()).is_err());
    }

    #[test]
    fn mismatch_unknown_and_revoked_fail() {
        let hosts = KnownHosts::from_text(
            &format!("github.com ssh-ed25519 {ED25519_B64}\n"),
            "/home/me/.ssh/known_hosts",
        );
        let mismatch = hosts.verify("github.com", &other_blob()).unwrap_err();
        assert!(mismatch.contains("does not match"), "{mismatch}");

        let empty = KnownHosts::from_text("", "/home/me/.ssh/known_hosts");
        let unknown = empty
            .verify("github.com", &github_ed25519_blob())
            .unwrap_err();
        assert!(unknown.contains("not a known SSH host"), "{unknown}");
        assert!(unknown.contains("ssh-keyscan github.com"), "{unknown}");

        let revoked = KnownHosts::from_text(
            &format!(
                "@revoked github.com ssh-ed25519 {ED25519_B64}\ngithub.com ssh-ed25519 {ED25519_B64}\n"
            ),
            "known_hosts",
        );
        let message = revoked
            .verify("github.com", &github_ed25519_blob())
            .unwrap_err();
        assert!(message.contains("@revoked"), "{message}");
    }

    /// libgit2's host key preference comes from these lines, so they must hold
    /// exactly what `verify` accepts, written under the plain host name.
    #[cfg(not(windows))]
    #[test]
    fn trusted_lines_name_the_host_and_skip_revoked_keys() {
        use base64::Engine;
        let other = "AAAAC3NzaC1lZDI1NTE5AAAAIHGa4yOXbfVDXHb2BhHQvNmGgBEVsjymlS7hZp3uNbRa";
        let salt = b"0123456789abcdef0123";
        let mut mac = Hmac::<sha1::Sha1>::new_from_slice(salt).unwrap();
        mac.update(b"[localhost]:2222");
        let b64 = base64::engine::general_purpose::STANDARD;
        let hashed = format!(
            "|1|{}|{}",
            b64.encode(salt),
            b64.encode(mac.finalize().into_bytes())
        );
        let hosts = KnownHosts::from_text(
            &format!(
                "{hashed} ssh-ed25519 {ED25519_B64}\n\
                 *.example.com ssh-ed25519 {other}\n\
                 @revoked github.com ssh-ed25519 {other}\n\
                 github.com ssh-ed25519 {other}\n\
                 github.com ssh-ed25519 {ED25519_B64}\n"
            ),
            "known_hosts",
        );
        assert_eq!(
            hosts.trusted_lines("[localhost]:2222"),
            format!("[localhost]:2222 ssh-ed25519 {ED25519_B64}\n")
        );
        assert_eq!(
            hosts.trusted_lines("git.example.com"),
            format!("git.example.com ssh-ed25519 {other}\n")
        );
        // The revoked key is left out even though a plain line also lists it.
        assert_eq!(
            hosts.trusted_lines("github.com"),
            format!("github.com ssh-ed25519 {ED25519_B64}\n")
        );
        assert!(hosts.has_negotiable_key_for("github.com"));
        assert!(!hosts.has_negotiable_key_for("gitlab.com"));

        // Only revoked or CA lines: nothing is trusted, so the host is unknown.
        let revoked_only = KnownHosts::from_text(
            &format!(
                "@revoked github.com ssh-ed25519 {other}\n\
                 @cert-authority github.com ssh-ed25519 {ED25519_B64}\n"
            ),
            "known_hosts",
        );
        assert_eq!(revoked_only.trusted_lines("github.com"), "");
        let message = revoked_only
            .verify("github.com", &github_ed25519_blob())
            .unwrap_err();
        assert!(message.contains("not a known SSH host"), "{message}");
    }

    #[test]
    fn wildcards_and_negation() {
        assert!(glob_matches("*.github.com", "ssh.github.com"));
        assert!(glob_matches("git?ub.com", "GitHub.com"));
        assert!(!glob_matches("*.github.com", "github.com"));
        let patterns = HostPatterns::Patterns(vec!["*.com".into(), "!gitlab.com".into()]);
        assert!(host_matches(&patterns, "github.com"));
        assert!(!host_matches(&patterns, "gitlab.com"));
    }
}
