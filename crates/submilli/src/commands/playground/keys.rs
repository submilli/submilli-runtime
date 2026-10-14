//! Where chat's model keys and settings come from, first match wins: this process's
//! environment, the project's `.env`, then the local secret store
//! (`submilli secret put <NAME>`). Only the playground's own names are read from the
//! file and the store, so an app's other secrets there are never picked up. The file
//! and the store are read each time, so a key added to either while the playground
//! runs is used without a restart; one exported in the shell needs one.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// The project's `.env`, once the playground knows its project.
static PROJECT_ENV: OnceLock<PathBuf> = OnceLock::new();

/// Reads `<root>/.env` from now on.
pub(crate) fn use_project(root: &Path) {
    let _ = PROJECT_ENV.set(root.join(ENV_FILE));
}

pub(crate) const ENV_FILE: &str = ".env";

/// Where a value was found.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Source {
    Environment,
    EnvFile,
    SecretStore,
}

impl Source {
    /// As the start report says it: "key from …".
    pub(crate) fn words(self) -> &'static str {
        match self {
            Self::Environment => "the environment",
            Self::EnvFile => "the project's .env",
            Self::SecretStore => "the secret store",
        }
    }
}

/// `name`'s value and where it came from; `None` when no source has it.
pub(crate) fn lookup(name: &str) -> Option<(String, Source)> {
    lookup_in(
        name,
        |name| std::env::var(name).ok(),
        PROJECT_ENV.get().map(PathBuf::as_path),
        store_value,
    )
}

/// `name`'s value, wherever [`lookup`] finds it.
pub(crate) fn value(name: &str) -> Option<String> {
    lookup(name).map(|(value, _)| value)
}

fn lookup_in(
    name: &str,
    env: impl Fn(&str) -> Option<String>,
    env_file: Option<&Path>,
    store: impl Fn(&str) -> Option<String>,
) -> Option<(String, Source)> {
    let set = |value: Option<String>| value.filter(|value| !value.trim().is_empty());
    if let Some(value) = set(env(name)) {
        return Some((value, Source::Environment));
    }
    if !super::agent::reads(name) {
        return None;
    }
    if let Some(value) = set(env_file.and_then(|path| file_value(path, name))) {
        return Some((value, Source::EnvFile));
    }
    set(store(name)).map(|value| (value, Source::SecretStore))
}

/// `name` as the `.env` at `path` sets it, the last line naming it winning:
/// `NAME=value`, optionally after `export `, with the value bare (a ` #` starts a
/// comment) or in single or double quotes.
fn file_value(path: &Path, name: &str) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    parse_value(&text, name)
}

fn parse_value(text: &str, name: &str) -> Option<String> {
    let mut found = None;
    for line in text.lines() {
        let line = line.trim();
        let line = line.strip_prefix("export ").unwrap_or(line);
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key.trim() != name {
            continue;
        }
        let value = value.trim();
        let quoted = ['"', '\''].into_iter().find_map(|quote| {
            value
                .strip_prefix(quote)
                .and_then(|rest| rest.split_once(quote))
                .map(|(inside, _)| inside)
        });
        let bare = || {
            value
                .split_once(" #")
                .map_or(value, |(before, _)| before)
                .trim_end()
        };
        found = Some(quoted.unwrap_or_else(bare).to_owned());
    }
    found
}

/// `name` from the local secret store, or `None` when it has none or can't be read.
fn store_value(name: &str) -> Option<String> {
    let store = crate::commands::local::open_secret_store().ok()?;
    // The local store reads a file and never waits on anything else.
    pollster::block_on(store.get(name))
        .ok()
        .flatten()
        .map(|value| value.trim().to_owned())
}

/// The longest key taken from the page.
const MAX_KEY_BYTES: usize = 4096;

/// What saving or removing a provider's key from the page did.
#[derive(Debug, PartialEq, Eq, serde::Serialize)]
pub(crate) struct Saved {
    /// The variable it was saved under, as `submilli secret put` names it.
    pub(crate) variable: &'static str,
    /// Where chat now finds that provider's key, if anywhere. Anything but the secret
    /// store means a key there comes first, so the one saved here isn't used yet.
    pub(crate) from: Option<Source>,
}

/// Saves `key` as `provider`'s key in the local secret store, replacing one saved
/// there before, or with `None` removes the saved one. The same store, under the same
/// name, as `submilli secret put <VARIABLE>`. The key is never in what this returns.
pub(crate) fn save_provider_key(provider: &str, key: Option<&str>) -> Result<Saved, String> {
    let variable = super::agent::key_variable(provider)
        .ok_or_else(|| format!("chat has no provider `{provider}`"))?;
    let store =
        crate::commands::local::open_secret_store().map_err(|error| format!("{error:#}"))?;
    match key.map(str::trim) {
        Some(key) => {
            check_key(key)?;
            pollster::block_on(store.put(variable, key))
                .map_err(|error| format!("saving the key failed: {error}"))?;
        }
        None => {
            pollster::block_on(store.delete(variable))
                .map_err(|error| format!("removing the key failed: {error}"))?;
        }
    }
    let from = super::agent::key_from(provider);
    Ok(Saved { variable, from })
}

fn check_key(key: &str) -> Result<(), String> {
    if key.is_empty() {
        return Err("give the key".to_owned());
    }
    if key.len() > MAX_KEY_BYTES {
        return Err(format!("a key is at most {MAX_KEY_BYTES} bytes"));
    }
    if key.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err("a key has no spaces or line breaks".to_owned());
    }
    Ok(())
}

/// Whether the project's `.env` is in a git repository but not ignored by it, so a key
/// in it could be committed by mistake.
pub(crate) fn env_file_tracked(root: &Path) -> bool {
    let path = root.join(ENV_FILE);
    if !path.is_file() {
        return false;
    }
    let ignored = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["check-ignore", "-q", ENV_FILE])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    // 1: not ignored, inside a repository. 128: not a repository, or no git.
    matches!(ignored.map(|status| status.code()), Ok(Some(1)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn none(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn the_shell_wins_then_the_project_file_then_the_secret_store() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(ENV_FILE);
        std::fs::write(&file, "OPENAI_API_KEY=from-file\n").unwrap();
        let shell = |name: &str| (name == "OPENAI_API_KEY").then(|| "from-shell".to_owned());
        let store = |name: &str| (name == "OPENAI_API_KEY").then(|| "from-store".to_owned());
        let found = |env: &dyn Fn(&str) -> Option<String>, file: Option<&Path>| {
            lookup_in("OPENAI_API_KEY", env, file, store)
        };
        assert_eq!(
            found(&shell, Some(&file)),
            Some(("from-shell".into(), Source::Environment))
        );
        assert_eq!(
            found(&none, Some(&file)),
            Some(("from-file".into(), Source::EnvFile))
        );
        assert_eq!(
            found(&none, None),
            Some(("from-store".into(), Source::SecretStore))
        );
        // An empty value counts as unset, so the next source is asked.
        std::fs::write(&file, "OPENAI_API_KEY=\n").unwrap();
        assert_eq!(
            found(&none, Some(&file)),
            Some(("from-store".into(), Source::SecretStore))
        );
    }

    #[test]
    fn the_file_and_the_store_give_only_the_playgrounds_own_names() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(ENV_FILE);
        std::fs::write(&file, "DATABASE_URL=postgres://secret\n").unwrap();
        let store = |_: &str| Some("stored".to_owned());
        assert_eq!(lookup_in("DATABASE_URL", none, Some(&file), store), None);
        // The shell is the process's own environment, read as it always was.
        let shell = |_: &str| Some("shell".to_owned());
        assert_eq!(
            lookup_in("DATABASE_URL", shell, Some(&file), store),
            Some(("shell".into(), Source::Environment))
        );
    }

    #[test]
    fn a_key_from_the_page_is_one_token() {
        assert!(check_key("sk-proj-abc_123").is_ok());
        assert_eq!(check_key(""), Err("give the key".to_owned()));
        assert_eq!(
            check_key("sk abc"),
            Err("a key has no spaces or line breaks".to_owned())
        );
        assert_eq!(
            check_key("sk\nabc"),
            Err("a key has no spaces or line breaks".to_owned())
        );
        assert!(check_key(&"k".repeat(MAX_KEY_BYTES + 1)).is_err());
    }

    #[test]
    fn a_dotenv_line_takes_export_quotes_and_comments() {
        let text = "# keys\n\
                    export ANTHROPIC_API_KEY=\"sk-ant # not a comment\"\n\
                    OPENAI_API_KEY = sk-old\n\
                    OPENAI_API_KEY=sk-new   # rotated\n\
                    GEMINI_API_KEY='g key'\n\
                    not a line\n";
        assert_eq!(
            parse_value(text, "ANTHROPIC_API_KEY").as_deref(),
            Some("sk-ant # not a comment")
        );
        assert_eq!(
            parse_value(text, "OPENAI_API_KEY").as_deref(),
            Some("sk-new")
        );
        assert_eq!(
            parse_value(text, "GEMINI_API_KEY").as_deref(),
            Some("g key")
        );
        assert_eq!(parse_value(text, "GOOGLE_API_KEY"), None);
    }
}
