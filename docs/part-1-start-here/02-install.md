---
title: "Install"
description: "Install the Submilli CLI and server, and the skill for your coding assistant, and check that each works."
slug: install
sidebar:
  order: 2
---

Two things to install: the `submilli` command, which comes with the
`submilli-server` binary, and the Submilli skill for your coding assistant.
The quickstart needs the first. The tutorials later in the book use the
second.

Submilli is open source under the [Apache License
2.0](https://github.com/submilli/submilli-runtime/blob/main/LICENSE). The
source, the curated packages, and the releases are in the
[`submilli/submilli-runtime`](https://github.com/submilli/submilli-runtime)
repository.

## The CLI and the server

macOS and Linux:

```
curl -fsSL https://submilli.ai/install.sh | sh
```

Windows (PowerShell):

```
irm https://submilli.ai/install.ps1 | iex
```

One line, and you have both binaries. Check the CLI:

```
submilli --version
```

```text
submilli 0.2.0
```

```
submilli builtins Map
```

```text
/**
 * A hash-backed key-value collection. Keys are compared by structural equality through each key's `equals` method; lookup buckets via `hash`. Insertion-order iteration is not guaranteed in v1.
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

## The skill

The skill teaches your coding assistant to write blueprints and packages,
test them, and connect your agent harness. Choose your assistant:

| Assistant | Command | Installed at |
| --- | --- | --- |
| Claude Code | `submilli skill install --agent claude` | `~/.claude/skills/submilli` |
| Codex | `submilli skill install --agent codex` | `~/.agents/skills/submilli` |
| Cursor | `submilli skill install --agent cursor` | `~/.cursor/skills/submilli` |

Add `--project .` to install into the current project instead of your home
directory; commit the installed folder to share it with your team. Restart
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
