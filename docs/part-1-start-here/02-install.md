---
title: "Install"
description: "Install the Submilli CLI and server, and the skill for your coding assistant, and check that each works."
slug: install
sidebar:
  order: 2
authorship:
  label: ai-assisted
  confirmed: true
  contentHash: "9e719c0ff9f39bfb523c9947499b145f740cf4d088840f44625a26d9cf3a5f8d"
  confirmedAt: "2026-10-05T13:01:53.009Z"
---

Use the [skill setup box above](#agent-setup) to get started with Codex or
Claude Code. The prompt asks your agent to install and read the Submilli skill
in this project, then verify the CLI and server. If you prefer
to install them yourself, follow the manual steps below.

Two things to install: the `submilli` command, which comes with the
`submilli-server` binary, and the Submilli skill for your coding assistant.
The quickstart needs the first. The tutorials later in the book use the
second.

Submilli is open source under the [Apache License
2.0](https://github.com/submilli/submilli-runtime/blob/main/LICENSE). The
source, the curated Packages, and the releases are in the
[`submilli/submilli-runtime`](https://github.com/submilli/submilli-runtime)
repository.

## Install the CLI and server yourself

The verification steps on this page and the [Quickstart](/docs/quickstart)
use Bash or Zsh. On Windows, follow them inside WSL and install the Linux
binaries there. A native PowerShell installation is separate from WSL.

macOS, Linux, and Windows inside WSL:

```
curl -fsSL https://submilli.ai/install.sh | sh
```

For a native Windows CLI and server installation, use PowerShell:

```
irm https://submilli.ai/install.ps1 | iex
```

These commands install the latest release. To choose a version or inspect
what the installer verifies, see [Pin and verify a release](#pin-and-verify-a-release).
Continue in Bash or Zsh for the checks here. Check the CLI:

```
submilli --version
```

```text
submilli 0.4.2
```

```
submilli builtins Map
```

```text
/**
 * A hash-backed key-value collection. Keys are compared by structural equality through each key's `equals` method; lookup buckets via `hash`. Iteration follows insertion order.
 */
interface Map<K, V> {
  /**
   * The number of entries currently in the map.
   */
  readonly size: number;
…
```

That is the declaration of one of the language's built-ins, printed by the
same compiler that will run your programs. Now the server. It checks a token
on every request, so it needs one to start:

```
export SUBMILLI_SERVER_TOKEN=$(openssl rand -hex 32)
submilli-server &
```

```text
ts=2026-10-03T17:05:23.415Z level=info stream=log target=submilli_server::auth msg="inbound authentication enabled" tokens="SUBMILLI_SERVER_TOKEN (admin)"
ts=2026-10-03T17:05:23.423Z level=info stream=log target=submilli_server::serve msg="submilli-server listening" addr=127.0.0.1:8128 protocol=http
```

The `submilli server` commands read the same variable, so from the same
shell:

```
submilli server status
submilli server stop
```

```text
status:          running
bind:            127.0.0.1:8128
pid:             16882
active sessions: 0
blueprints:      (none)
```

Later, `submilli upgrade` moves both binaries to the latest release in place.

### Pin and verify a release

Pass a release tag to choose the binary version. Keep the tag with the
deployment record, and use the same option when upgrading a pinned deployment:

```sh
curl -fsSL https://submilli.ai/install.sh | sh -s -- --version v0.4.2
```

For native Windows, save and run the installer with its `-Version` parameter:

```powershell
$installer = Join-Path $env:TEMP ('submilli-install-' + [guid]::NewGuid() + '.ps1')
Invoke-WebRequest -UseBasicParsing https://submilli.ai/install.ps1 -OutFile $installer
& $installer -Version v0.4.2
Remove-Item $installer
```

Both installers download the binaries and `SHA256SUMS` over HTTPS and check
each binary against that file before replacing an existing installation.
This detects a mismatch with the checksum list. It does not authenticate the
installer script or establish who published the list. The installer script
at `submilli.ai` can change independently of the selected binary version.
Review the [installer source](https://github.com/submilli/submilli-runtime)
and [release assets](https://github.com/submilli/submilli-runtime/releases)
before running them when you need to approve the code and files yourself.

## The skill

The skill teaches your coding assistant to write Blueprints and Packages,
test them, and connect your agent harness. Choose your assistant:

| Assistant | Command | Installed at |
| --- | --- | --- |
| Claude Code | `submilli skill install --agent claude` | `~/.claude/skills/submilli` |
| Codex | `submilli skill install --agent codex` | `~/.agents/skills/submilli` |
| Cursor | `submilli skill install --agent cursor` | `~/.cursor/skills/submilli` |

Add `--project .` to install into the current project instead of your home
directory, and commit the installed folder to share it with your team. Restart
your assistant afterwards, then invoke the skill: `/submilli` in Claude Code,
`$submilli` in Codex, or ask "Help me adopt Submilli in this project."

Check an installation:

```
submilli skill status --agent claude
```

It reports whether the skill is current and whether its files have been
edited, and exits 0 when everything is current. The skill keeps itself
current from then on.

Next: the [quickstart](/docs/quickstart).
