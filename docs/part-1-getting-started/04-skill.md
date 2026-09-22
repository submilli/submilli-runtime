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

## The verifier subagent

Installing the skill also installs a `submilli-verifier` subagent beside it:
`.claude/agents/submilli-verifier.md` for Claude Code, `.cursor/agents/` for
Cursor, and `.codex/agents/submilli-verifier.toml` for Codex. After your
assistant writes or changes a package or blueprint, the skill has it delegate
an independent review to that subagent, which reads the policy as someone
looking for a way around it and reports findings with evidence. Each finding
carries a confidence anchor (100: visible in the files; 75: a traced route;
50: unconfirmed), and a finding at 75 or above must quote the line that makes
it true. Assistants without subagents run the same checklist themselves. Updates refresh the
verifier unless you have edited it.

## Updates

The skill keeps itself current. Its first instruction tells your assistant to
run `submilli skill sync` before reading anything else. That command takes no
flags: it finds every installation made by the CLI in your home directory and
from the working directory up to the repository root, and brings each
unedited one to the newest skill release. You do nothing, and you can run it
yourself at any time.

Skill releases are `skill-v<N>` tags of the
[runtime repository](https://github.com/submilli/submilli-runtime) and ship
independently of CLI releases. `sync` asks GitHub for the newest tag at most
once a day with a five-second timeout, downloads that release's single
`submilli-skill.json` asset over HTTPS, and rejects files that would land
outside the skill directory. When offline, or when the CLI's embedded copy is
newer, it installs the embedded copy instead. If a release needs a newer CLI,
`sync` says so and keeps what you have; `submilli upgrade` installs the latest
CLI. `sync` also mentions when a newer CLI exists but never upgrades it for
you. Set `SUBMILLI_SKILL_AUTOUPDATE=0` to
use only the embedded copy.

`submilli skill status --agent codex` checks one installation against the
running binary and checks its files for edits. It returns 0 for current
content, including a skill release newer than the binary's copy, and 1 for
missing, outdated, or edited content. `submilli skill update` with the same
target applies the binary's copy without any network access. Ordinary CLI
commands print a reminder in interactive terminals when an installation is
older than their bundle; machine-readable output is unaffected.

Updates verify the installed file hashes before replacing anything. Edited,
added, or missing files stop replacement. To keep custom instructions, move
the existing skill directory aside, install a clean copy, then review and
reapply your changes. There is no force-overwrite flag. Symlinked installation
paths are refused; choose a regular directory. If an installer crashes, an
error may identify a lock or a recovery directory; confirm it is no longer
running before removing a stale lock, and preserve any recovered files.

For teams, a project installation that is committed shows each skill update as
an ordinary diff: whoever's assistant syncs first gets the change to review
and commit. To control timing instead, set `SUBMILLI_SKILL_AUTOUPDATE=0` in
the team's environment, pin the CLI version, and run `skill status` in CI.
