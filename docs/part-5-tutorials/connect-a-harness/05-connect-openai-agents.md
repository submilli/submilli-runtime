---
title: "Connect OpenAI Agents SDK"
description: "Connect OpenAI Agents SDK to a local Submilli server without a model call."
slug: tutorials/connect-openai-agents
sidebar:
  order: 5
authorship:
  label: ai-generated
  confirmed: true
  contentHash: "24f51d6f245b995309018e0d1151ef59b2089d63f5690e3950f7f10415808bca"
  confirmedAt: "2026-10-18T13:09:05.218103+00:00"
# Unpublished AI draft. Human prose rewrite required before publication.
---

In this tutorial we will connect OpenAI Agents SDK to a Submilli server on your
computer. OpenAI Agents SDK is a Python library for building agent programs. SDK stands for software development kit.
An agent program uses an AI model and calls tools on its behalf.
A tool is an action the agent can request.

Submilli receives code from an agent and runs it. Here, we only check the
connection. You will start the server, give it a small rules file, and run
the connection code. Success prints `Connected to Submilli.`

The check asks Submilli which tools it offers. Success means the library
can see the tool for running code.

You do not need a model account or API key. This check uses no model credits.

## What you need

- Python 3.11 or later.
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
mkdir submilli-openai-agents-connection && cd submilli-openai-agents-connection || exit
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

## Install OpenAI Agents SDK

Download the list of Python libraries this check needs.

```sh
curl -fsSL https://raw.githubusercontent.com/submilli/submilli-runtime/f281964f52da8581ea6766d5cacd27d32eca4639/examples/harnesses/openai-agents/requirements.txt -o requirements.txt
```

Check the Python version. Continue only if it is 3.11 or later.

```sh
python3 --version
```

Create a Python environment for this folder so its libraries stay separate.

```sh
python3 -m venv .venv
```

Activate it. Python commands in this shell will now use this folder's libraries.

```sh
. .venv/bin/activate
```

Install the libraries listed in `requirements.txt`.

```sh
python -m pip install -r requirements.txt
```

Wait for installation to finish without an error before continuing.

## Save the connection code

Save this complete file as `check.py` in the tutorial folder.
Use it as written. It reads the server address and token set in your shell.

```python title="check.py"
import asyncio
import os

from agents.mcp import MCPServerStreamableHttp


async def main():
    async with MCPServerStreamableHttp(
        name="submilli",
        params={
            "url": f"{os.environ['SUBMILLI_SERVER']}/mcp/connection-check",
            "headers": {
                "Authorization": f"Bearer {os.environ['SUBMILLI_SERVER_TOKEN']}",
            },
        },
    ) as connection:
        tools = await connection.list_tools()
        if not any(tool.name == "submilli__typescript__execute" for tool in tools):
            raise RuntimeError("Submilli's execute tool was not found")
        print("Connected to Submilli.")


asyncio.run(main())
```

MCP (Model Context Protocol) is a standard way for an agent library to
connect to tools. `MCPServerStreamableHttp` manages that connection here.
`list_tools()` asks Submilli for its available tools. The check looks for
`submilli__typescript__execute`, the tool that runs code.

The `async with` block closes the connection when the check ends.

## Check the connection

Run the file.

```sh
python check.py
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

The files stay in the tutorial folder. You have checked the OpenAI Agents SDK
connection to Submilli.
