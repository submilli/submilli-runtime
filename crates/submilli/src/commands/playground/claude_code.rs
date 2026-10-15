//! `submilli playground claude-code`: sets Claude Code up in a project to try tasks
//! through the playground, on the developer's Claude subscription. It writes two things:
//!
//! - the stand-in subagent, `.claude/agents/submilli-standin.md`, which has only the
//!   playground's tools (through `submilli playground mcp`), so it acts as the
//!   production agent would rather than reading the project's files itself;
//! - the hooks that copy the stand-in's conversation to the page
//!   (`submilli playground chat-hook`), in `.claude/settings.local.json`: the
//!   developer's own settings, which Claude Code keeps out of git, so a teammate
//!   without `submilli` never runs them;
//! - in the same file, a rule that allows the stand-in's playground tools, so Claude
//!   Code's own checks (auto mode's among them) let its calls through to the
//!   playground, where the blueprint decides them.
//!
//! Running it again changes nothing that is already in place. A stand-in file that
//! differs from this one is left alone unless `--force` replaces it; settings that
//! aren't a JSON object are never touched.

use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

use super::chat::STAND_IN_AGENT;

/// Where the stand-in and the hooks go, under the project root.
pub(crate) const AGENT_FILE: &str = ".claude/agents/submilli-standin.md";
pub(crate) const SETTINGS_FILE: &str = ".claude/settings.local.json";

/// What each hook runs.
pub(crate) const HOOK_COMMAND: &str = "submilli playground chat-hook";

/// Allows every tool of the stand-in's `playground` server.
pub(crate) const ALLOW_RULE: &str = "mcp__playground__*";

/// The hook events `chat-hook` posts for: each of the stand-in's tool calls, and its stop.
const HOOK_EVENTS: [&str; 4] = [
    "PreToolUse",
    "PostToolUse",
    "PostToolUseFailure",
    "SubagentStop",
];

/// The playground's tools, as Claude Code names them for the stand-in's `playground` server.
const TOOLS: [&str; 8] = [
    "submilli__files__list",
    "submilli__files__read",
    "submilli__typescript__builtins__docs",
    "submilli__typescript__builtins__list",
    "submilli__typescript__execute",
    "submilli__typescript__last_run",
    "submilli__typescript__packages__docs",
    "submilli__typescript__packages__search",
];

/// The stand-in subagent's file.
pub(crate) fn agent_file() -> String {
    let tools = TOOLS
        .map(|tool| format!("mcp__playground__{tool}"))
        .join(", ");
    format!(
        "---\n\
         name: {STAND_IN_AGENT}\n\
         description: Runs programs through the Submilli playground as the production agent \
         would. Use when asked to try a task through the playground.\n\
         tools: {tools}\n\
         mcpServers:\n  \
         - playground:\n      \
         type: stdio\n      \
         command: submilli\n      \
         args: [\"playground\", \"mcp\"]\n\
         ---\n\
         You stand in for the production agent. Do the task with the playground's tools only, \
         writing TypeScript-subset programs and running them with the execute tool. Report \
         what happened, including any denial.\n"
    )
}

/// What became of one of the two files.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// It was missing, and now holds what it should.
    Written,
    /// It held what it should already.
    Unchanged,
    /// It held something else, and now holds what it should (`--force`, or the hooks
    /// added to settings already there).
    Updated,
    /// The stand-in file holds something else, left as it is.
    Kept,
}

impl Outcome {
    pub(crate) fn word(self) -> &'static str {
        match self {
            Outcome::Written => "written",
            Outcome::Unchanged => "unchanged",
            Outcome::Updated => "updated",
            Outcome::Kept => "kept",
        }
    }
}

/// The stand-in file's new text, if it changes, and what that is.
pub(crate) fn plan_agent(existing: Option<&str>, force: bool) -> (Option<String>, Outcome) {
    let want = agent_file();
    match existing {
        None => (Some(want), Outcome::Written),
        Some(text) if text == want => (None, Outcome::Unchanged),
        Some(_) if force => (Some(want), Outcome::Updated),
        Some(_) => (None, Outcome::Kept),
    }
}

/// The settings' new text, if it changes, and what that is: every hook event gets an
/// entry that runs [`HOOK_COMMAND`] unless one already does, and `permissions.allow`
/// gets [`ALLOW_RULE`] unless it has it. Settings that aren't a JSON object are refused.
pub(crate) fn plan_settings(existing: Option<&str>) -> Result<(Option<String>, Outcome), String> {
    let mut settings = match existing {
        None => Map::new(),
        Some(text) if text.trim().is_empty() => Map::new(),
        Some(text) => match serde_json::from_str::<Value>(text) {
            Ok(Value::Object(map)) => map,
            Ok(_) => return Err("it isn't a JSON object".to_owned()),
            Err(error) => return Err(format!("it isn't valid JSON ({error})")),
        },
    };
    let hooks = settings.entry("hooks").or_insert_with(|| json!({}));
    let Some(hooks) = hooks.as_object_mut() else {
        return Err("its `hooks` isn't a JSON object".to_owned());
    };
    let mut added = false;
    for event in HOOK_EVENTS {
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
                "matcher": "*",
                "hooks": [{ "type": "command", "command": HOOK_COMMAND }],
            }));
            added = true;
        }
    }
    let permissions = settings.entry("permissions").or_insert_with(|| json!({}));
    let Some(permissions) = permissions.as_object_mut() else {
        return Err("its `permissions` isn't a JSON object".to_owned());
    };
    let allow = permissions.entry("allow").or_insert_with(|| json!([]));
    let Some(allow) = allow.as_array_mut() else {
        return Err("its `permissions.allow` isn't a list".to_owned());
    };
    if !allow.iter().any(|rule| rule == ALLOW_RULE) {
        allow.push(json!(ALLOW_RULE));
        added = true;
    }
    if !added {
        return Ok((None, Outcome::Unchanged));
    }
    let text = serde_json::to_string_pretty(&Value::Object(settings)).expect("JSON value") + "\n";
    let outcome = if existing.is_some() {
        Outcome::Updated
    } else {
        Outcome::Written
    };
    Ok((Some(text), outcome))
}

/// What a setup did, for the report.
#[derive(Debug)]
pub(crate) struct Setup {
    pub(crate) agent: (PathBuf, Outcome),
    pub(crate) settings: (PathBuf, Outcome),
}

/// Sets Claude Code up under `root`. Nothing is written when the settings can't be read
/// as settings: the stand-in alone would show no chat on the page.
pub(crate) fn set_up(root: &Path, force: bool) -> Result<Setup, String> {
    let agent_path = root.join(AGENT_FILE);
    let settings_path = root.join(SETTINGS_FILE);
    let (settings_text, settings_outcome) = plan_settings(read(&settings_path)?.as_deref())
        .map_err(|why| format!("{} wasn't changed: {why}", settings_path.display()))?;
    let (agent_text, agent_outcome) = plan_agent(read(&agent_path)?.as_deref(), force);
    if let Some(text) = agent_text {
        write(&agent_path, &text)?;
    }
    if let Some(text) = settings_text {
        write(&settings_path, &text)?;
    }
    Ok(Setup {
        agent: (agent_path, agent_outcome),
        settings: (settings_path, settings_outcome),
    })
}

fn read(path: &Path) -> Result<Option<String>, String> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("reading {}: {error}", path.display())),
    }
}

/// Writes `text` to `path` whole or not at all, readable as any project file is.
fn write(path: &Path, text: &str) -> Result<(), String> {
    let failed = |error: io::Error| format!("writing {}: {error}", path.display());
    let dir = path.parent().expect("a file under the project root");
    fs::create_dir_all(dir).map_err(failed)?;
    let staged = tempfile::Builder::new()
        .prefix(super::fsx::STAGED_PREFIX)
        .permissions(fs::Permissions::from_mode(0o644))
        .tempfile_in(dir)
        .map_err(failed)?;
    fs::write(staged.path(), text).map_err(failed)?;
    staged.persist(path).map_err(|error| failed(error.error))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stand_in_file_has_frontmatter_and_only_the_playground_tools() {
        let file = agent_file();
        assert!(file.starts_with("---\nname: submilli-standin\n"), "{file}");
        assert!(
            file.contains("\nmcpServers:\n  - playground:\n      type: stdio\n"),
            "{file}"
        );
        assert!(file.contains("\n---\nYou stand in"), "{file}");
        let tools = file
            .lines()
            .find_map(|line| line.strip_prefix("tools: "))
            .unwrap();
        assert_eq!(tools.split(", ").count(), TOOLS.len());
        assert!(
            tools
                .split(", ")
                .all(|tool| tool.starts_with("mcp__playground__submilli__"))
        );
    }

    #[test]
    fn a_stand_in_file_that_differs_is_kept_unless_forced() {
        assert_eq!(plan_agent(None, false).1, Outcome::Written);
        assert_eq!(
            plan_agent(Some(&agent_file()), true),
            (None, Outcome::Unchanged)
        );
        assert_eq!(plan_agent(Some("name: mine"), false), (None, Outcome::Kept));
        assert_eq!(
            plan_agent(Some("name: mine"), true),
            (Some(agent_file()), Outcome::Updated)
        );
    }

    #[test]
    fn the_hooks_join_settings_already_there_and_are_added_once() {
        let mine = r#"{"permissions":{"allow":["Bash(ls)"]},"hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"mine"}]}]}}"#;
        let (text, outcome) = plan_settings(Some(mine)).unwrap();
        assert_eq!(outcome, Outcome::Updated);
        let text = text.unwrap();
        let settings: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(settings["permissions"]["allow"][0], "Bash(ls)");
        let pre = settings["hooks"]["PreToolUse"].as_array().unwrap();
        assert_eq!(pre.len(), 2);
        assert_eq!(pre[0]["hooks"][0]["command"], "mine");
        assert_eq!(
            settings["permissions"]["allow"],
            json!(["Bash(ls)", ALLOW_RULE])
        );
        for event in HOOK_EVENTS {
            assert_eq!(
                settings["hooks"][event].as_array().unwrap().last().unwrap()["hooks"][0]["command"],
                HOOK_COMMAND
            );
        }
        // Run again: nothing more.
        assert_eq!(
            plan_settings(Some(&text)).unwrap(),
            (None, Outcome::Unchanged)
        );
        // From nothing, or an empty file.
        assert_eq!(plan_settings(None).unwrap().1, Outcome::Written);
        assert!(plan_settings(Some("\n")).unwrap().0.is_some());
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
    fn a_refused_settings_file_leaves_both_files_as_they_were() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".claude")).unwrap();
        fs::write(dir.path().join(SETTINGS_FILE), "{ not json").unwrap();
        let why = set_up(dir.path(), false).unwrap_err();
        assert!(why.contains("wasn't changed"), "{why}");
        assert!(!dir.path().join(AGENT_FILE).exists());
        assert_eq!(
            fs::read_to_string(dir.path().join(SETTINGS_FILE)).unwrap(),
            "{ not json"
        );
    }

    #[test]
    fn set_up_writes_both_and_a_second_run_changes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let first = set_up(dir.path(), false).unwrap();
        assert_eq!(
            (first.agent.1, first.settings.1),
            (Outcome::Written, Outcome::Written)
        );
        assert_eq!(
            fs::read_to_string(dir.path().join(AGENT_FILE)).unwrap(),
            agent_file()
        );
        let mode = fs::metadata(dir.path().join(AGENT_FILE))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o644);
        let again = set_up(dir.path(), false).unwrap();
        assert_eq!(
            (again.agent.1, again.settings.1),
            (Outcome::Unchanged, Outcome::Unchanged)
        );
    }
}
