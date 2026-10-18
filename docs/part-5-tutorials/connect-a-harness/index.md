---
title: "Connect a harness"
description: "Check a local server and user-bound notebooks, then choose a standalone harness tutorial."
slug: tutorials/connect-a-harness
sidebar:
  order: 0
authorship:
  label: ai-generated
  confirmed: true
  contentHash: "d13af199717eb6b4f7ca3da4b6e5a813622adfcac4e118b0161570321da1dd9f"
  confirmedAt: "2026-10-18T12:30:00Z"
# Unpublished AI structural draft. Human rewrite required before publication.
# Validation evidence is in the external authoring handoff. No model call is needed here.
---

In this tutorial we will start a local Submilli server and prove that two
users have separate notebooks. We will use a small program without a model
or provider account.

Your agent framework is the harness. It runs the conversation and sends
programs to Submilli for execution under a Blueprint. The five integrations
below build a research agent that searches the web and saves notes.

| Harness | Language | Connection |
| --- | --- | --- |
| [Mastra](/docs/tutorials/connect-mastra) | TypeScript | MCP |
| [LangChain deepagents](/docs/tutorials/connect-deepagents) | Python | `langchain-submilli` over HTTP |
| [OpenAI Agents SDK](/docs/tutorials/connect-openai-agents) | Python | MCP |
| [Claude Agent SDK](/docs/tutorials/connect-claude-agent-sdk) | TypeScript | MCP |
| [Vercel AI SDK](/docs/tutorials/use-the-http-api) | TypeScript | HTTP |

Each integration includes its own setup. You can start directly with the
framework you use, or complete the server check here first.

For this exercise, install the `submilli` and `submilli-server` binaries,
version 0.4.1 or later, from [Install](/docs/install). Use a macOS or Linux
terminal with Bash, `curl` and `openssl`. Keep port 18128 free.

## Download the example

Open a Bash shell for this tutorial and create a new directory. Keep using
this shell until the cleanup step.

```sh
bash
mkdir submilli-harness-check && cd submilli-harness-check || exit
export HARNESS_ROOT="$PWD"
example_ref=f281964f52da8581ea6766d5cacd27d32eca4639
example_url="https://raw.githubusercontent.com/submilli/submilli-runtime/$example_ref/examples/harnesses"
(
  set -eu
  for file in blueprint.yaml \
      prompt.txt \
      note.ts; do
    mkdir -p "$(dirname "$file")"
    curl -fsSL "$example_url/$file" -o "$file"
  done
)
```

The three files are the [research Blueprint](https://github.com/submilli/submilli-runtime/blob/f281964f52da8581ea6766d5cacd27d32eca4639/examples/harnesses/blueprint.yaml), the
[agent brief](https://github.com/submilli/submilli-runtime/blob/f281964f52da8581ea6766d5cacd27d32eca4639/examples/harnesses/prompt.txt), and the [test program](https://github.com/submilli/submilli-runtime/blob/f281964f52da8581ea6766d5cacd27d32eca4639/examples/harnesses/note.ts).
The Blueprint permits the Jina Package, model calls and access to the mounted
notebook. This exercise uses only the notebook.

## Start the local server

Clear inherited Submilli settings in this tutorial shell so its state stays in
this directory. The CLI reads `SUBMILLI_SERVER_URL` and the example reads
`SUBMILLI_SERVER`, so set both.

```sh
for name in ${!SUBMILLI_@}; do unset "$name"; done
umask 077
export SUBMILLI_HOME="$HARNESS_ROOT/.submilli"
export SUBMILLI_SERVER="http://127.0.0.1:18128"
export SUBMILLI_SERVER_URL="$SUBMILLI_SERVER"
export SUBMILLI_SERVER_TOKEN="$(openssl rand -hex 32)"
openssl rand -base64 32 > store.key
```

Save this complete file as `server.yaml` in the current directory.

```yaml title="server.yaml"
telemetry: false
secret_store:
  key_file: store.key
volumes:
  notes:
    kind: managed-local
    size_limit: unlimited
```

Start the server, then check it with the new token.

```sh
submilli-server --config server.yaml --bind 127.0.0.1 --port 18128 > server.log 2>&1 &
SUBMILLI_SERVER_PID=$!
submilli server status
```

Continue when the status says `running` at `127.0.0.1:18128`. If startup is
still in progress, run the status command again. For an error, read
`server.log` before continuing. This token has admin access for the local
exercise. [Application tokens](/docs/server/run-the-server#give-the-application-its-own-api-token)
cover deployment.

Install the Jina Package and register the downloaded Blueprint. The two dummy
secrets let us check file access without contacting a provider.

```sh
submilli server packages install submilli/submilli-runtime @submilli/jina --sha "$example_ref"
printf 'check-only\n' | submilli server secret put jina_api_key
printf 'check-only\n' | submilli server secret put anthropic_api_key
submilli server blueprint apply blueprint.yaml
```

## Check the notebook

`note.ts` lists the current user's notes, then writes `check.md`. Run it in
three new sessions, first for Ada, then Grace, then Ada again.

```sh
submilli server run-code note.ts --blueprint research --var userId=u_ada
submilli server run-code note.ts --blueprint research --var userId=u_grace
submilli server run-code note.ts --blueprint research --var userId=u_ada
```

The returned values, in order, are:

```text
notes before: none
notes before: none
notes before: check.md
```

Notice that Grace starts with an empty notebook and Ada's second session
finds her file. The Blueprint mounts each user's directory at `/notes`.
[Keep files and state](/docs/blueprints/keep-files-and-state) explains the
mount.

## Stop the server

```sh
kill "$SUBMILLI_SERVER_PID"
wait "$SUBMILLI_SERVER_PID"
exit
```

The example files and `.submilli` directory remain in the tutorial directory.

You have a working server and a notebook that persists across sessions. Choose
your integration from the table above to connect a model.
