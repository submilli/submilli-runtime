---
title: "Connect Mastra"
description: "Connect Mastra to a local Submilli server without a model call."
slug: tutorials/connect-mastra
sidebar:
  order: 3
authorship:
  label: ai-generated
  confirmed: true
  contentHash: "b4446c5f6ba19896c64b7f8530656cfc4f8d5d143204a05604c8b0228b003a1b"
  confirmedAt: "2026-10-18T13:09:05.218103+00:00"
# Unpublished AI draft. Human prose rewrite required before publication.
---

In this tutorial we will connect Mastra to a Submilli server on your
computer. Mastra is a TypeScript library for building agent programs.
An agent program uses an AI model and calls tools on its behalf.
A tool is an action the agent can request.

Submilli receives code from an agent and runs it. Here, we only check the
connection. You will start the server, give it a small rules file, and run
the connection code. Success prints `Connected to Submilli.`

The check asks Submilli which tools it offers. Success means the library
can see the tool for running code.

You do not need a model account or API key. This check uses no model credits.

## What you need

- Node.js 26.8.1 or later.
- A macOS or Linux terminal with Bash, `curl`, and `openssl`.
- The `submilli` and `submilli-server` programs, version 0.4.1 or later.
  [Install](/docs/install) explains how to get them.

The steps use port 18128, a local address number for the server.

## Create a folder

Open Bash, the command interpreter used by these steps.

```sh
bash
```

Create a new folder. If this name already exists, use a new name. The command
stops if it cannot create the folder.

```sh
mkdir submilli-mastra-connection && cd submilli-mastra-connection || exit
```

Keep using this terminal until the last step. Save this folder's path so
later commands can find it.

```sh
export TUTORIAL_ROOT="$PWD"
```

## Set up the server

The server is the program that accepts the connection. These commands keep
its data inside the tutorial folder. They create a random token, which acts
as the password for this local server.

```sh
for name in ${!SUBMILLI_@}; do unset "$name"; done
umask 077
export SUBMILLI_HOME="$TUTORIAL_ROOT/.submilli"
export SUBMILLI_SERVER="http://127.0.0.1:18128"
export SUBMILLI_SERVER_URL="$SUBMILLI_SERVER"
export SUBMILLI_SERVER_TOKEN="$(openssl rand -hex 32)"
```

The first line clears old Submilli settings in this shell. `umask 077` makes
new files private to your computer account. The connection code reads
`SUBMILLI_SERVER`. The `submilli` command reads `SUBMILLI_SERVER_URL`.
Both addresses point to this computer, at `127.0.0.1`.

Save this file as `server.yaml`. It turns off usage reporting for the check.

```yaml title="server.yaml"
telemetry: false
```

Start the server. `--bind` keeps it on this computer. `--port` selects 18128.
The `&` leaves it running while you use the same terminal. Messages go to
`server.log`.

```sh
submilli-server --config server.yaml --bind 127.0.0.1 --port 18128 > server.log 2>&1 &
SUBMILLI_SERVER_PID=$!
```

`SUBMILLI_SERVER_PID` stores the process number so you can stop this server
later. Check that it started.

```sh
submilli server status
```

Look for `running` and `127.0.0.1:18128`. If it says `stopped`, wait one
second and run the status command again. If it still says `stopped`, read
the server messages.

```sh
cat server.log
```

If the log says `Address already in use`, another program has port 18128.
Change `18128` to `18129` in both the `SUBMILLI_SERVER` setting and the
server command's `--port` value. Repeat the address settings and start command.
The status should now show `running` at `127.0.0.1:18129`.

## Add the rules file

Submilli requires a Blueprint for the connection. A Blueprint is a file that
sets the rules for code sent to Submilli. Save this as `blueprint.yaml`.

```yaml title="blueprint.yaml"
kind: blueprint
name: connection-check
default: deny
```

`name` gives these rules the name `connection-check`. `default: deny` blocks
operations such as reading files and calling web services. This connection
check needs neither operation.

Load the rules into the server.

```sh
submilli server blueprint apply blueprint.yaml
```

The command prints `Added blueprint 'connection-check'`.

## Install Mastra

Download the list of JavaScript libraries this check needs.

```sh
curl -fsSL https://raw.githubusercontent.com/submilli/submilli-runtime/f281964f52da8581ea6766d5cacd27d32eca4639/examples/harnesses/mastra/package.json -o package.json
```

Check the Node.js version. Continue only if it is 26.8.1 or later.

```sh
node --version
```

Install the libraries listed in `package.json` into this folder.

```sh
npm install
```

Wait for installation to finish without an error before continuing.

## Save the connection code

Save this complete file as `check.ts` in the tutorial folder.
Use it as written. It reads the server address and token set in your shell.

```typescript title="check.ts"
import { MCPClient } from "@mastra/mcp";

const connection = new MCPClient({
  id: "connection-check",
  servers: {
    submilli: {
      url: new URL(`${process.env.SUBMILLI_SERVER}/mcp/connection-check`),
      requestInit: {
        headers: { Authorization: `Bearer ${process.env.SUBMILLI_SERVER_TOKEN}` },
      },
    },
  },
});

try {
  const tools = await connection.listTools();
  const hasExecute = Object.keys(tools).some(
    (name) => name.endsWith("submilli__typescript__execute"),
  );
  if (!hasExecute) throw new Error("Submilli's execute tool was not found");
  console.log("Connected to Submilli.");
} finally {
  await connection.disconnect();
}
```

MCP (Model Context Protocol) is a standard way for an agent library to
connect to tools. Mastra's `MCPClient` manages that connection here.
`listTools()` asks Submilli for its available tools. The check looks for
`submilli__typescript__execute`, the tool that runs code.

`finally` closes the client connection even if the check fails.

## Check the connection

Run the file.

```sh
npx tsx check.ts
```

Look for this output:

```text
Connected to Submilli.
```

An error means the check did not pass. Run `submilli server status` to check
that the server is still running. Keep the same terminal open so the address
and token are still set.

## Stop the server

Stop the server that this tutorial started.

```sh
kill "$SUBMILLI_SERVER_PID"
wait "$SUBMILLI_SERVER_PID"
```

Close the tutorial shell to remove its temporary settings.

```sh
exit
```

The files stay in the tutorial folder. You have checked the Mastra
connection to Submilli.
