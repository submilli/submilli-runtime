//! The `submilli-standin` subagent's chat hooks and allow rule, merged into a
//! project's `.claude/settings.local.json`: the developer's own settings, which
//! Claude Code keeps out of git, so a teammate without `submilli` never runs them.
//!
//! - the hooks copy the stand-in's conversation to the playground page
//!   (`submilli playground chat-hook`), and fire only for the stand-in's
//!   playground tools and its stop, so the project's other tool calls don't
//!   start `chat-hook`;
//! - the rule allows the stand-in's playground tools, so Claude Code's own checks
//!   (auto mode's among them) let its calls through to the playground, where the
//!   blueprint decides them.
//!
//! Merging again changes nothing that is already in place; settings that aren't
//! a JSON object are never touched.

use std::fs;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use serde_json::{Map, Value, json};

/// Where the hooks go, under the project root.
pub(super) const SETTINGS_FILE: &str = ".claude/settings.local.json";

/// What each hook runs.
const HOOK_COMMAND: &str = "submilli playground chat-hook";

/// Allows every tool of the stand-in's `playground` server.
const ALLOW_RULE: &str = "mcp__playground__*";

/// The hook events `chat-hook` posts for, each with the matcher that narrows it to
/// the stand-in: its playground tools' calls, and its stop, matched on agent type.
const HOOKS: [(&str, &str); 4] = [
    ("PreToolUse", "mcp__playground__.*"),
    ("PostToolUse", "mcp__playground__.*"),
    ("PostToolUseFailure", "mcp__playground__.*"),
    ("SubagentStop", "submilli-standin"),
];

/// A planned merge: the file to write, and its new text if anything changes.
pub(super) struct Merge {
    path: PathBuf,
    target: PathBuf,
    /// The permissions of the file already there, kept on the rewrite.
    permissions: Option<fs::Permissions>,
    text: Option<String>,
}

/// Reads the settings under `root` and plans the merge, so settings that can't be
/// merged stop an install before anything is written.
pub(super) fn plan(root: &Path) -> anyhow::Result<Merge> {
    let path = root.join(SETTINGS_FILE);
    let existing = match fs::read_to_string(&path) {
        Ok(text) => Some(text),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(error).with_context(|| format!("reading {}", path.display())),
    };
    let text = plan_settings(existing.as_deref())
        .map_err(|why| anyhow::anyhow!("{} wasn't changed: {why}", path.display()))?;
    // A link to a file kept elsewhere is followed, so the file stays where it is;
    // a link to nothing is refused rather than replaced.
    let (target, permissions) = match fs::canonicalize(&path) {
        Ok(target) => {
            let permissions = fs::metadata(&target)?.permissions();
            (target, Some(permissions))
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            if fs::symlink_metadata(&path).is_ok_and(|meta| meta.file_type().is_symlink()) {
                bail!(
                    "{} links to a file that isn't there; create it, or remove the link",
                    path.display()
                );
            }
            (path.clone(), None)
        }
        Err(error) => return Err(error).with_context(|| format!("reading {}", path.display())),
    };
    Ok(Merge {
        path,
        target,
        permissions,
        text,
    })
}

impl Merge {
    /// Writes the merged settings whole or not at all. Whether anything changed.
    pub(super) fn apply(self) -> anyhow::Result<bool> {
        let Some(text) = self.text else {
            return Ok(false);
        };
        write(&self.target, &text, self.permissions)
            .with_context(|| format!("writing {}", self.path.display()))?;
        println!(
            "{}: added the submilli-standin agent's chat hooks and permission for its playground tools.",
            self.path.display()
        );
        Ok(true)
    }
}

/// The settings' new text, if it changes: every hook event gets an entry that runs
/// [`HOOK_COMMAND`] with its narrow matcher, and `permissions.allow` gets
/// [`ALLOW_RULE`] unless it has it. Settings that aren't a JSON object are refused.
fn plan_settings(existing: Option<&str>) -> Result<Option<String>, String> {
    let mut settings = match existing {
        None => Map::new(),
        Some(text) if text.trim().is_empty() => Map::new(),
        Some(text) => match serde_json::from_str::<Value>(text) {
            Ok(Value::Object(map)) => map,
            Ok(_) => return Err("it isn't a JSON object".to_owned()),
            Err(error) => return Err(format!("it isn't valid JSON ({error})")),
        },
    };
    let changed_hooks = add_hooks(&mut settings)?;
    let added_rule = add_allow_rule(&mut settings)?;
    if !changed_hooks && !added_rule {
        return Ok(None);
    }
    let text = serde_json::to_string_pretty(&Value::Object(settings))
        .expect("a JSON map with string keys always serializes")
        + "\n";
    Ok(Some(text))
}

/// Gives every hook event an entry that runs [`HOOK_COMMAND`], unless one already does.
/// Entries already there are left alone. Whether anything changed.
fn add_hooks(settings: &mut Map<String, Value>) -> Result<bool, String> {
    let hooks = settings.entry("hooks").or_insert_with(|| json!({}));
    let Some(hooks) = hooks.as_object_mut() else {
        return Err("its `hooks` isn't a JSON object".to_owned());
    };
    let mut changed = false;
    for (event, matcher) in HOOKS {
        let entries = hooks.entry(event).or_insert_with(|| json!([]));
        let Some(entries) = entries.as_array_mut() else {
            return Err(format!("its `hooks.{event}` isn't a list"));
        };
        let runs_it = entries.iter().any(|entry| {
            entry["hooks"]
                .as_array()
                .is_some_and(|hooks| hooks.iter().any(|hook| hook["command"] == HOOK_COMMAND))
        });
        if !runs_it {
            entries.push(json!({
                "matcher": matcher,
                "hooks": [{ "type": "command", "command": HOOK_COMMAND }],
            }));
            changed = true;
        }
    }
    Ok(changed)
}

/// Adds [`ALLOW_RULE`] to `permissions.allow`, unless it's there. Whether it was added.
fn add_allow_rule(settings: &mut Map<String, Value>) -> Result<bool, String> {
    let permissions = settings.entry("permissions").or_insert_with(|| json!({}));
    let Some(permissions) = permissions.as_object_mut() else {
        return Err("its `permissions` isn't a JSON object".to_owned());
    };
    let allow = permissions.entry("allow").or_insert_with(|| json!([]));
    let Some(allow) = allow.as_array_mut() else {
        return Err("its `permissions.allow` isn't a list".to_owned());
    };
    if allow.iter().any(|rule| rule == ALLOW_RULE) {
        return Ok(false);
    }
    allow.push(json!(ALLOW_RULE));
    Ok(true)
}

/// Writes `text` to `path` whole or not at all. A file already there keeps its
/// permissions; a new file is readable as any project file is.
fn write(path: &Path, text: &str, permissions: Option<fs::Permissions>) -> io::Result<()> {
    let dir = path
        .parent()
        .expect("the project root joined with a file path has a parent");
    fs::create_dir_all(dir)?;
    let mut staged = tempfile::Builder::new()
        .prefix(".submilli-stage-")
        .tempfile_in(dir)?;
    staged.write_all(text.as_bytes())?;
    // Set after writing, so a read-only file stays read-only, and set outright,
    // past the umask.
    if let Some(permissions) = staged_permissions(permissions) {
        fs::set_permissions(staged.path(), permissions)?;
    }
    staged.persist(path).map_err(|error| error.error)?;
    Ok(())
}

#[cfg(unix)]
fn staged_permissions(kept: Option<fs::Permissions>) -> Option<fs::Permissions> {
    use std::os::unix::fs::PermissionsExt;
    let mode = kept.map_or(0o644, |kept| kept.mode() & 0o7777);
    Some(fs::Permissions::from_mode(mode))
}

#[cfg(not(unix))]
fn staged_permissions(kept: Option<fs::Permissions>) -> Option<fs::Permissions> {
    kept
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set_up(root: &Path) -> anyhow::Result<bool> {
        plan(root)?.apply()
    }

    #[test]
    fn the_bundled_stand_in_has_frontmatter_and_only_the_playground_tools() {
        let file = super::super::FILES
            .iter()
            .find(|(name, _)| *name == "agents/submilli-standin.md")
            .unwrap()
            .1;
        // The `SubagentStop` matcher is the stand-in's agent type.
        let stop = HOOKS.iter().find(|(event, _)| *event == "SubagentStop");
        assert!(
            file.starts_with(&format!("---\nname: {}\n", stop.unwrap().1)),
            "{file}"
        );
        assert!(
            file.contains("\nmcpServers:\n  - playground:\n      type: stdio\n"),
            "{file}"
        );
        assert!(file.contains("\n---\nYou stand in"), "{file}");
        let tools = file
            .lines()
            .find_map(|line| line.strip_prefix("tools: "))
            .unwrap();
        assert!(
            tools
                .split(", ")
                .all(|tool| tool.starts_with("mcp__playground__submilli__"))
        );
    }

    #[test]
    fn the_hooks_join_settings_already_there_and_are_added_once() {
        let mine = r#"{"permissions":{"allow":["Bash(ls)"]},"hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"mine"}]}]}}"#;
        let text = plan_settings(Some(mine)).unwrap().unwrap();
        let settings: Value = serde_json::from_str(&text).unwrap();
        let pre = settings["hooks"]["PreToolUse"].as_array().unwrap();
        assert_eq!(pre.len(), 2);
        assert_eq!(pre[0]["hooks"][0]["command"], "mine");
        assert_eq!(pre[0]["matcher"], "Bash");
        assert_eq!(
            settings["permissions"]["allow"],
            json!(["Bash(ls)", ALLOW_RULE])
        );
        for (event, matcher) in HOOKS {
            let last = settings["hooks"][event].as_array().unwrap().last().unwrap();
            assert_eq!(last["hooks"][0]["command"], HOOK_COMMAND);
            assert_eq!(last["matcher"], matcher);
        }
        // Run again: nothing more.
        assert_eq!(plan_settings(Some(&text)).unwrap(), None);
        // From nothing, or an empty file.
        assert!(plan_settings(None).unwrap().is_some());
        assert!(plan_settings(Some("\n")).unwrap().is_some());
    }

    #[test]
    fn settings_that_arent_settings_are_refused() {
        for (text, says) in [
            ("{ not json", "isn't valid JSON"),
            ("[]", "isn't a JSON object"),
            (r#"{"hooks":[]}"#, "`hooks` isn't a JSON object"),
            (
                r#"{"hooks":{"PreToolUse":{}}}"#,
                "`hooks.PreToolUse` isn't a list",
            ),
            (
                r#"{"permissions":{"allow":"all"}}"#,
                "`permissions.allow` isn't a list",
            ),
        ] {
            let why = plan_settings(Some(text)).unwrap_err();
            assert!(why.contains(says), "{text}: {why}");
        }
    }

    #[test]
    fn a_refused_settings_file_is_left_as_it_was() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".claude")).unwrap();
        fs::write(dir.path().join(SETTINGS_FILE), "{ not json").unwrap();
        let why = plan(dir.path()).err().unwrap().to_string();
        assert!(why.contains("wasn't changed"), "{why}");
        assert_eq!(
            fs::read_to_string(dir.path().join(SETTINGS_FILE)).unwrap(),
            "{ not json"
        );
    }

    #[test]
    fn a_merge_writes_the_file_and_a_second_one_changes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        assert!(set_up(dir.path()).unwrap());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(dir.path().join(SETTINGS_FILE))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o644);
        }
        assert!(!set_up(dir.path()).unwrap());
    }

    #[cfg(unix)]
    #[test]
    fn settings_kept_elsewhere_stay_there_with_their_mode() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let elsewhere = dir.path().join("dotfiles-settings.json");
        fs::write(&elsewhere, "{}").unwrap();
        fs::set_permissions(&elsewhere, fs::Permissions::from_mode(0o600)).unwrap();
        fs::create_dir_all(dir.path().join(".claude")).unwrap();
        std::os::unix::fs::symlink(&elsewhere, dir.path().join(SETTINGS_FILE)).unwrap();

        set_up(dir.path()).unwrap();

        let link = fs::symlink_metadata(dir.path().join(SETTINGS_FILE)).unwrap();
        assert!(link.file_type().is_symlink());
        assert!(fs::read_to_string(&elsewhere).unwrap().contains(ALLOW_RULE));
        let mode = fs::metadata(&elsewhere).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[cfg(unix)]
    #[test]
    fn a_read_only_settings_file_is_updated_and_stays_read_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let settings = dir.path().join(SETTINGS_FILE);
        fs::create_dir_all(dir.path().join(".claude")).unwrap();
        fs::write(&settings, "{}").unwrap();
        fs::set_permissions(&settings, fs::Permissions::from_mode(0o444)).unwrap();

        set_up(dir.path()).unwrap();

        assert!(fs::read_to_string(&settings).unwrap().contains(ALLOW_RULE));
        let mode = fs::metadata(&settings).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o444);
    }

    #[cfg(unix)]
    #[test]
    fn a_link_to_nothing_is_refused_and_left_as_it_is() {
        let dir = tempfile::tempdir().unwrap();
        let settings = dir.path().join(SETTINGS_FILE);
        fs::create_dir_all(dir.path().join(".claude")).unwrap();
        std::os::unix::fs::symlink(dir.path().join("not-yet.json"), &settings).unwrap();

        let why = plan(dir.path()).err().unwrap().to_string();

        assert!(why.contains("links to a file that isn't there"), "{why}");
        assert!(
            fs::symlink_metadata(&settings)
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }
}
