---
title: "Coding assistant skill"
slug: skill
sidebar:
  order: 4
---

The Submilli skill teaches your coding assistant to explain the runtime,
inspect your project, design packages and blueprints, and connect your agent
harness. It asks about the agent's intended work and authority before turning
existing APIs into agent operations. It supports Deep Agents, Vercel AI SDK,
LangChain/LangGraph, Mastra, and custom loops.

## Install

Install the [CLI](/docs/quickstart), then choose your assistant:

| Assistant | Command | User installation |
| --- | --- | --- |
| Claude Code | `submilli skill install --agent claude` | `~/.claude/skills/submilli` |
| Codex | `submilli skill install --agent codex` | `~/.agents/skills/submilli` |
| Cursor | `submilli skill install --agent cursor` | `~/.cursor/skills/submilli` |

The home directory is your OS user home (`USERPROFILE` on Windows). Add
`--project .` to any command to use that directory instead; a different
existing directory also works. Run project installation from the repository
root if the whole team should discover the skill. Commit the installed folder,
including `.submilli-skill.json`, to share it.

The skill is the same on each assistant. Cursor also discovers `.agents` and
`.claude` skill folders, so a Codex installation can serve local Cursor too;
avoid duplicate copies in one scope. Dedicated Cursor installations support
Cursor's own personal-skill sync. Remote/cloud environments need their own
installation, a committed project skill, or the assistant's sync mechanism.

Restart your assistant after installing or updating. Ask “Help me adopt
Submilli in this project,” select the skill, or invoke `/submilli` in Claude
Code or `$submilli` in Codex. Start with a real task, such as “let our support
agent look up only the signed-in customer's charges.”

## Updates

Every CLI release embeds a complete skill. Installation and update do not
download or execute a second installer. To get a published skill update:

1. Upgrade the CLI using the official installer from the quickstart.
2. Run `submilli skill update --agent codex`, substituting your assistant and
   adding the same `--project` directory used during installation.
3. Restart the assistant so it reads the new instructions.

`submilli skill status --agent codex` compares an installation with the running
binary's bundle and checks its files for edits. It returns 0 for current and 1
for missing, different, or edited content. A different bundle can be newer
or older; select the intended CLI version before updating. Status does not
query the latest release on the internet.

Ordinary CLI commands print a reminder in interactive terminals when a managed
user or enclosing project installation differs from their bundle. The skill
also directs the assistant to check status during implementation sessions.
Neither mechanism changes files automatically. Machine-readable command
output and noninteractive runs are unaffected by reminders.

Updates verify the installed file hashes before replacing anything. Edited,
added, or missing files stop replacement. To keep custom instructions, move
the existing skill directory aside, install a clean copy, then review and
reapply your changes. There is no force-overwrite flag. Symlinked installation
paths are refused; choose a regular directory. If an installer crashes, an
error may identify a lock or a recovery directory; confirm it is no longer
running before removing a stale lock, and preserve any recovered files.

For teams, pin the CLI version in your development setup, run status in CI,
and update the committed skill in a dependency-update PR. This makes changes
to the instructions reviewable along with runtime upgrades. Publishing skill
changes requires a new CLI release; independently downloaded or automatically
changing skills are not part of this mechanism.
