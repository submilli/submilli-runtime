//! Which GitHub token the CLI's package fetches send, and where
//! `submilli github authenticate` stores one.
//!
//! Sources, first found wins: `GH_TOKEN`, `GITHUB_TOKEN` (the usual names in
//! CI), the token stored by `submilli github authenticate`, then the GitHub
//! CLI's (`gh auth token`). The stored token is its own owner-only file rather
//! than an entry in the local secret store, which blueprints can read.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use submilli_shared::github::{GithubAuth, GithubToken, TokenSource};

/// Environment variables checked for a token, in order.
const ENV_VARS: [&str; 2] = ["GH_TOKEN", "GITHUB_TOKEN"];
/// How long `gh auth token` may take before it's ignored.
const GH_TIMEOUT: Duration = Duration::from_secs(5);
/// Most of `gh auth token`'s output read; a token is far shorter.
const GH_OUTPUT_BYTES: u64 = 4096;

/// The token package fetches send, and where it came from.
pub fn resolve_auth() -> GithubAuth {
    let (token, source) = choose(env_value, read_stored, gh_cli_token);
    GithubAuth::new(token, source)
}

/// The environment variable whose token package fetches use, if any.
pub fn env_token_var() -> Option<&'static str> {
    env_token(env_value).map(|(_, name)| name)
}

/// `$SUBMILLI_HOME/github_token`.
pub fn stored_path() -> PathBuf {
    submilli_build::default_data_root().join("github_token")
}

/// Store `token` readable by its owner only, replacing any earlier one in a
/// single rename: a failed or concurrent write never leaves a half-written
/// token, and a staged copy is removed if the write fails.
pub fn store(token: &GithubToken) -> Result<()> {
    store_at(&stored_path(), token)
}

/// Remove the stored token; `false` if there was none.
pub fn remove_stored() -> Result<bool> {
    let path = stored_path();
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(true),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(err).with_context(|| format!("removing {}", path.display())),
    }
}

/// The first token found, in [`ENV_VARS`], stored, then `gh` order.
fn choose(
    env: impl Fn(&str) -> Option<String>,
    stored: impl FnOnce() -> Option<GithubToken>,
    gh: impl FnOnce() -> Option<GithubToken>,
) -> (Option<GithubToken>, TokenSource) {
    if let Some((token, name)) = env_token(env) {
        return (Some(token), TokenSource::Env(name));
    }
    if let Some(token) = stored() {
        return (Some(token), TokenSource::Stored);
    }
    if let Some(token) = gh() {
        return (Some(token), TokenSource::GhCli);
    }
    (None, TokenSource::Absent)
}

/// The first of [`ENV_VARS`] holding a token, which outranks a stored one. A
/// variable holding something that isn't a token is reported and skipped.
fn env_token(env: impl Fn(&str) -> Option<String>) -> Option<(GithubToken, &'static str)> {
    ENV_VARS.into_iter().find_map(|name| {
        let value = env(name).filter(|value| !value.trim().is_empty())?;
        match GithubToken::parse(&value) {
            Ok(token) => Some((token, name)),
            Err(err) => {
                eprintln!("warning: ignoring `{name}`: {err}");
                None
            }
        }
    })
}

/// The variable's value; one that isn't Unicode is reported, not ignored
/// silently.
fn env_value(name: &str) -> Option<String> {
    match std::env::var(name) {
        Ok(value) => Some(value),
        Err(std::env::VarError::NotPresent) => None,
        Err(std::env::VarError::NotUnicode(_)) => {
            eprintln!("warning: ignoring `{name}`: it is not valid Unicode");
            None
        }
    }
}

fn read_stored() -> Option<GithubToken> {
    let path = stored_path();
    // `symlink_metadata`, so a dangling link is reported rather than taken
    // for no token.
    match std::fs::symlink_metadata(&path) {
        Ok(_) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return None,
        Err(err) => {
            eprintln!(
                "warning: ignoring the stored GitHub token: {}: {err}",
                path.display()
            );
            return None;
        }
    }
    match GithubToken::read_file(&path) {
        Ok(token) => Some(token),
        Err(err) => {
            eprintln!("warning: ignoring the stored GitHub token: {err}");
            None
        }
    }
}

/// The GitHub CLI's token for github.com, if `gh` is installed and logged in.
/// Anything else (no `gh`, an error, a hang, endless output) means no token.
fn gh_cli_token() -> Option<GithubToken> {
    let deadline = Instant::now().checked_add(GH_TIMEOUT)?;
    let mut child = Command::new("gh")
        .args(["auth", "token", "--hostname", "github.com"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let output = read_gh_output(&mut child, deadline);
    // Reaped on every path, so no `gh` is left running or as a zombie.
    let _ = child.kill();
    let status = child.wait().ok()?;
    if !status.success() {
        return None;
    }
    GithubToken::parse(&output?).ok()
}

/// `gh`'s stdout, once it exits before `deadline`. Read on a thread, so a
/// grandchild holding the pipe open can't block past the deadline.
fn read_gh_output(child: &mut Child, deadline: Instant) -> Option<String> {
    let stdout = child.stdout.take()?;
    let (sender, output) = mpsc::channel();
    std::thread::Builder::new()
        .name("gh-auth-token".into())
        .spawn(move || {
            let mut text = String::new();
            let read = stdout.take(GH_OUTPUT_BYTES).read_to_string(&mut text);
            let _ = sender.send(read.map(|_| text));
        })
        .ok()?;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            _ => return None,
        }
    }
    let remaining = deadline.saturating_duration_since(Instant::now());
    output.recv_timeout(remaining).ok()?.ok()
}

fn store_at(path: &std::path::Path, token: &GithubToken) -> Result<()> {
    let dir = path
        .parent()
        .context("the GitHub token path has no directory")?;
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    // `NamedTempFile` creates the file 0600 on Unix and deletes it on drop.
    let mut staged = tempfile::NamedTempFile::new_in(dir)
        .with_context(|| format!("writing in {}", dir.display()))?;
    staged
        .write_all(token.secret().as_bytes())
        .and_then(|()| staged.as_file().sync_all())
        .with_context(|| format!("writing in {}", dir.display()))?;
    staged
        .persist(path)
        .map_err(|err| err.error)
        .with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token(value: &str) -> Option<GithubToken> {
        Some(GithubToken::parse(value).unwrap())
    }

    fn chosen(
        env: &[(&str, &str)],
        stored: Option<&str>,
        gh: Option<&str>,
    ) -> (Option<String>, TokenSource) {
        let (token, source) = choose(
            |name| {
                env.iter()
                    .find(|(key, _)| *key == name)
                    .map(|(_, value)| (*value).to_string())
            },
            || stored.and_then(token),
            || gh.and_then(token),
        );
        (token.map(|token| token.secret().to_string()), source)
    }

    #[test]
    fn the_environment_wins_then_the_stored_token_then_gh() {
        assert_eq!(
            chosen(
                &[("GITHUB_TOKEN", "ghp_b"), ("GH_TOKEN", "ghp_a")],
                Some("ghp_s"),
                Some("gho_g")
            ),
            (Some("ghp_a".into()), TokenSource::Env("GH_TOKEN"))
        );
        assert_eq!(
            chosen(&[("GITHUB_TOKEN", "ghp_b")], Some("ghp_s"), None),
            (Some("ghp_b".into()), TokenSource::Env("GITHUB_TOKEN"))
        );
        assert_eq!(
            chosen(&[], Some("ghp_s"), Some("gho_g")),
            (Some("ghp_s".into()), TokenSource::Stored)
        );
        assert_eq!(
            chosen(&[], None, Some("gho_g")),
            (Some("gho_g".into()), TokenSource::GhCli)
        );
        assert_eq!(chosen(&[], None, None), (None, TokenSource::Absent));
    }

    #[test]
    fn empty_or_malformed_environment_values_are_skipped() {
        assert_eq!(
            chosen(
                &[("GH_TOKEN", " "), ("GITHUB_TOKEN", "not a token")],
                Some("ghp_s"),
                None
            ),
            (Some("ghp_s".into()), TokenSource::Stored)
        );
    }

    #[test]
    fn a_malformed_environment_token_is_not_the_one_in_use() {
        let env = |name: &str| (name == "GH_TOKEN").then(|| "not a token".to_string());
        assert!(env_token(env).is_none());
        let env = |name: &str| {
            Some(
                if name == "GH_TOKEN" {
                    "not a token"
                } else {
                    "ghp_good"
                }
                .to_string(),
            )
        };
        let (token, name) = env_token(env).unwrap();
        assert_eq!((token.secret(), name), ("ghp_good", "GITHUB_TOKEN"));
    }

    #[test]
    fn store_replaces_the_token_owner_only_and_leaves_nothing_else() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("github_token");
        std::fs::write(&path, "ghp_old").unwrap();
        store_at(&path, &GithubToken::parse("ghp_new").unwrap()).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "ghp_new");
        let entries = std::fs::read_dir(dir.path()).unwrap().count();
        assert_eq!(entries, 1, "a staged copy was left behind");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
    }
}
