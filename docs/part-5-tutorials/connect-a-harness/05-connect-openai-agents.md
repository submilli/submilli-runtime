---
title: "Connect OpenAI Agents"
description: "Run a standalone OpenAI Agents SDK research agent, check its connection, and verify its saved notes."
slug: tutorials/connect-openai-agents
sidebar:
  order: 5
authorship:
  label: ai-generated
  confirmed: true
  contentHash: "987f4ee5295a8238e588f8d42032711180de2847b8f147a4c11762b28db5d2b2"
  confirmedAt: "2026-10-18T12:30:00Z"
# Unpublished AI structural draft. Human rewrite required before publication.
# Validation: fresh live-model replay is pending. Historical output is not a replay.
---

In this tutorial we will run a research agent with OpenAI Agents SDK, execute
its programs on a local Submilli server, and read its saved notes in a new
session. The page includes the server setup.

You need Python 3.11 or later, a macOS or Linux terminal with Bash, `curl`
and `openssl`, and the `submilli` and `submilli-server` binaries at version
0.4.1 or later. [Install](/docs/install) covers the binaries. Keep port 18128
free. Have Jina, Anthropic and OpenAI API keys ready for the conversation step.
The setup checks use dummy keys and make no model calls.

## Download the example

Open a Bash shell for this tutorial and create a new directory. Keep using
this shell until the cleanup step.

```sh
bash
mkdir submilli-openai-agents && cd submilli-openai-agents || exit
export HARNESS_ROOT="$PWD"
example_ref=f281964f52da8581ea6766d5cacd27d32eca4639
example_url="https://raw.githubusercontent.com/submilli/submilli-runtime/$example_ref/examples/harnesses"
(
  set -eu
  for file in blueprint.yaml \
      prompt.txt \
      note.ts \
      openai-agents/agent.py \
      openai-agents/check.py \
      openai-agents/requirements.txt; do
    mkdir -p "$(dirname "$file")"
    curl -fsSL "$example_url/$file" -o "$file"
  done
)
```

The root holds `blueprint.yaml`, `prompt.txt` and `note.ts`. Keep the
`openai-agents` directory beside them because the agent reads `../prompt.txt`
and the check reads `../note.ts`. These downloads are pinned to one source
revision. The dependency files still contain version ranges.

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

The Jina Package searches and reads web pages. The downloaded
[Blueprint](https://github.com/submilli/submilli-runtime/blob/f281964f52da8581ea6766d5cacd27d32eca4639/examples/harnesses/blueprint.yaml)
permits those operations, model calls and notebook access. Install the Package
and register the Blueprint with two dummy secrets for the file-access check.

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

## Check the framework connection

Confirm that `python3 --version` reports Python 3.11 or later before creating
the environment.

```sh
cd openai-agents
python3 -m venv .venv
. .venv/bin/activate
python -m pip install -r requirements.txt
OPENAI_API_KEY=check-only python check.py
```

The check ends with:

```text
openai-agents: ok
```

The scripted model checks that the connection can execute a program and
that users retain separate notebooks across sessions.

## Follow the session

Open the downloaded [agent.py](https://github.com/submilli/submilli-runtime/blob/f281964f52da8581ea6766d5cacd27d32eca4639/examples/harnesses/openai-agents/agent.py).
The `MCPServerStreamableHttp` context manager opens a connection and closes
it after the answer. Its `submilli-variables` header binds the session to
`u_ada`, standing in for the signed-in user supplied by your application.

Keep `client_session_timeout_seconds=120` when adapting this example. Search
and page-reading programs can take longer than the SDK's default timeout.
The conversation uses the OpenAI SDK's default model. The Anthropic model in
the Blueprint is a separate model that generated programs may call to
summarize pages.

## Run a research question

Replace the two dummy server secrets. Each command prompts for the value
without putting the key in shell history.

```sh
submilli server secret put jina_api_key
submilli server secret put anthropic_api_key
```

The server uses the Jina and Anthropic keys for generated programs. The
conversation itself needs your OpenAI key in the Python process.

Enter that key at the hidden prompt and start the agent.

```sh
read -rsp "OPENAI_API_KEY: " OPENAI_API_KEY; printf '\n'
export OPENAI_API_KEY
python agent.py
```

The example asks on behalf of `u_ada`:

```text
What is new in the latest stable release of Rust? Save a note with your sources.
```

The previous tutorial's recorded run reused a notebook populated by other
harnesses. Its answer included:

```text
I saved and updated the research note at `/notes/rust-latest-release.md`.
```

This tutorial starts a separate notebook, so the model needs to create its
research note on the first run.

Your run will produce different wording. Watch for the research steps and a
saved note with sources. A model answer alone does not verify that a file was written, so read it next.

## Read the saved note in a new session

Return to the tutorial root and save this complete program as
`inspect-notes.ts`. It reads files without changing them.

```sh
cd "$HARNESS_ROOT"
```

```typescript title="inspect-notes.ts"
import fs from "submilli:fs";

function main(): string {
  const notes: string[] = [];
  for (const entry of fs.list(".", true)) {
    if (entry.kind === "file") {
      notes.push(entry.path + "\n" + (fs.readText(entry.path) ?? "(file too large to read)"));
    }
  }
  return notes.join("\n\n");
}
```

```sh
submilli server run-code inspect-notes.ts --blueprint research --var userId=u_ada
```

Look for a research note with source links alongside `check.md`. Its name and
contents depend on the model's run. Seeing it here confirms that a new
session can read what the agent saved.

## Stop the server

```sh
kill "$SUBMILLI_SERVER_PID"
wait "$SUBMILLI_SERVER_PID"
exit
```

The example files and `.submilli` directory remain in the tutorial directory.

You have connected OpenAI Agents SDK to Submilli and checked the saved notebook.
For adapting the storage policy, continue with [Keep files and state](/docs/blueprints/keep-files-and-state).
