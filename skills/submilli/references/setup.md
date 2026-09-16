# Installation and projects

Check `submilli --version`, `submilli-server --help`, the OS, and project files.
Do not reinstall a working CLI to answer a concepts question. If absent, the
official installation commands are:

```sh
# macOS / Linux
curl -fsSL https://submilli.ai/install.sh | sh
```

```powershell
# Windows PowerShell
irm https://submilli.ai/install.ps1 | iex
```

Use the user's preferred installation method when specified. If a download
fails, report the actual error and consult https://submilli.ai/docs/quickstart;
do not invent a Homebrew/npm/pip distribution. Verify both binaries afterwards.

## Install and maintain this skill

The CLI carries a complete offline copy matching its release:

```sh
submilli skill install --agent claude
submilli skill install --agent codex
submilli skill install --agent cursor
```

Choose the user's assistant; these are alternatives. User installations use
`~/.claude/skills/submilli`, `~/.agents/skills/submilli`, or
`~/.cursor/skills/submilli`. Add `--project .` to install in the current project
instead. Restart the assistant to discover/reload the skill. Invoke Submilli
by name or use its skill selector (`/submilli` in Claude Code, `$submilli` in
Codex). Cursor supports skill invocation through its slash menu.

Check and update the same target:

```sh
submilli skill status --agent codex --project .
submilli skill update --agent codex --project .
```

Status exits 1 for missing, different, or edited content, and 0 for current.
It compares against the running binary, not a remote release. Upgrade the CLI
using the official installer before refreshing to a newly published skill.
`update` replaces only an intact managed installation. If locally edited,
move it aside deliberately, install, and review/reapply the customization.
Do not silently delete it. Project copies can be committed and updated by a
team's normal dependency-update PR. A pinned older CLI installs its older
bundle; use the team's intended CLI version.

Cursor also discovers `.agents` and `.claude` skills: avoid redundant copies
in one scope. If Codex is installed too, its `.agents` copy serves local Cursor.
Use a dedicated `.cursor` copy only when needed, such as Cursor cloud sync;
check the assistant's current discovery settings for duplicates. Local home
installs do not automatically propagate to remote/cloud machines.

## New project

Establish the first agent workflow and trusted user identity source using
[discovery](discovery.md). Create a directory only when a new project is
requested. Scaffold the first package from its root:

```sh
submilli build init @acme/billing packages/billing
```

Implement [the package](packages.md), then [its blueprint](blueprints.md),
then [the harness](harnesses.md). Keep ordinary application dependencies in
the application's existing environment.

## Existing project

Read manifests, agent entrypoints, authentication, service clients, tool
registrations, and tests first. Explain which existing code remains the host
application and which operations become Submilli wrappers. Preserve layout,
lockfiles, and framework choices. If `submilli.toml` exists, work at its root
and add a package with `submilli build new @acme/billing packages/billing`;
do not overwrite it with `build init`. Port only the wrapper's required logic;
ordinary Node/Python SDKs cannot be imported into Submilli.

Finish with commands actually run, allowed/denied results, files changed, and
any missing service credentials or model access. Distinguish deterministic
runtime verification from a live model test.
