---
docType: how-to
title: "Connect deepagents to Submilli"
description: "Create a deepagents agent that uses Submilli for permitted code execution."
showDescription: false
slug: how-to/connect-deepagents
sidebar:
  order: 4
authorship:
  label: ai-generated
  confirmed: true
  contentHash: "0f245308cb6b8b4ac5cdef98ee8cf3128dfcdeac7929c2ba8acd1f9a9b2582aa"
  confirmedAt: "2026-10-18T13:09:05.218103+00:00"
# Unpublished AI draft. Human prose rewrite required before publication.
---

Create a fresh deepagents agent with Submilli's tools, then run a small
program and see its result in your terminal.

## Use your coding agent

Open a project directory in your coding agent.
[Install the Submilli skill](/docs/install#the-skill) if needed, then restart
the coding agent to load it. Give it this prompt:

```text agent-prompt
Use the Submilli skill and this guide to create a fresh deepagents agent in
the current project. Create the complete submilli-agent.py file shown in this
guide, including its command-line prompt and tool-result logging. Pass it no
application tools, extra middleware, custom backend, or subagents; use the
DeepAgents harness profile and exclusions shown here so the model reaches
Submilli only.
Use the existing local configuration for SUBMILLI_SERVER,
SUBMILLI_SERVER_TOKEN, and SUBMILLI_BLUEPRINT. Do not invent values, expose
secrets, create a Blueprint, or change the server. If any of those three
connection values is unavailable, ask me for that specific value or its local
configuration source. Inspect the project's provider/model setup and ask me
only for an essential missing model identifier, provider package, or
credential. Install the dependencies shown here if they are absent. Verify
the server and Blueprint with the file's --check mode without making a model
request, then leave the file ready for the exact Try it out command.
```

After the coding agent finishes, [try it out](#try-it-out).

<details>
<summary>Connecting an existing agent</summary>

In `create_deep_agent`, set `tools=[]`, `subagents=[]`, and
`middleware=[submilli]`. Remove any custom backend. Register the
`HarnessProfile.excluded_tools` list shown in the runnable file and set
`GeneralPurposeSubagentProfile(enabled=False)` to remove DeepAgents' built-in
tools and general-purpose subagent. Profiles apply to the model in the current
process, so use a separate process if other agents need that model's default
tools. A Blueprint governs programs run through Submilli; another execution
route would bypass that boundary.

</details>

## Prerequisites

Use the [Submilli install guide](/docs/install), [server guide](/docs/server/run-the-server),
and [Blueprint registration guide](/docs/server/register-a-blueprint) if you still
need to prepare the service. This page assumes that preparation is complete:
you have a running Submilli server, a token it accepts, and a registered
Blueprint. You also need Python 3.11 or later and model access for deepagents.

Configure these values in the project environment or its existing local
configuration:

| Variable | Value |
| --- | --- |
| `SUBMILLI_SERVER` | The URL of your running Submilli server |
| `SUBMILLI_SERVER_TOKEN` | A token accepted by that server |
| `SUBMILLI_BLUEPRINT` | The name of your registered Blueprint |
| `DEEPAGENTS_MODEL` | A provider-qualified LangChain model identifier supported by your installed provider integration. Install that provider’s LangChain package and configure its credentials using the [LangChain model setup](https://docs.langchain.com/oss/python/langchain/models). |

The model credentials are separate from the Submilli server token. Reuse your
chosen provider's setup. Do not put credentials in the file.

## Set up manually

In the current project environment, install the integration packages if they
are not already installed:

```sh
python -m pip install "langchain-submilli[deepagents]>=0.1" "deepagents==0.7.28"
```

Set `SUBMILLI_SERVER`, `SUBMILLI_SERVER_TOKEN`, `SUBMILLI_BLUEPRINT`, and
`DEEPAGENTS_MODEL` from your existing local configuration. If a connection
value is missing, use the [server guide](/docs/server/run-the-server) or
[Blueprint registration guide](/docs/server/register-a-blueprint) to identify
the value, then continue with the file below.

### Save the runnable agent

Save this complete file as `submilli-agent.py` in the current project directory.

This file uses DeepAgents' public harness settings. `HarnessProfile.excluded_tools`
removes the ten built-in tools: `ls`, `read_file`, `write_file`, `edit_file`,
`delete`, `glob`, `grep`, `execute`, `task`, and `write_todos`.
`GeneralPurposeSubagentProfile(enabled=False)` separately disables the built-in
general-purpose subagent. The first setting is a model-keyed profile and the
second is the general-purpose-subagent setting; neither grants tools. The
profile is registered for this model in the current process, so run this file
in its own process when other agents use the same model. The middleware requires
asynchronous calls. The `tools=[]`, `subagents=[]`, and `middleware=[submilli]`
arguments below must remain as shown: do not add application tools, middleware
that adds tools, a custom backend, or subagents.

```python title="submilli-agent.py"
import asyncio
import os
import sys


def required(name):
    value = os.environ.get(name)
    if not value:
        raise RuntimeError(f"Set {name} in this terminal before running this file.")
    return value

from deepagents import (
    GeneralPurposeSubagentProfile,
    HarnessProfile,
    create_deep_agent,
    register_harness_profile,
)
from langchain_submilli import SubmilliMiddleware


async def main():
    required("SUBMILLI_SERVER")
    required("SUBMILLI_SERVER_TOKEN")
    required("SUBMILLI_BLUEPRINT")
    model_name = required("DEEPAGENTS_MODEL")
    register_harness_profile(
        model_name,
        HarnessProfile(
            excluded_tools=frozenset({
                "ls", "read_file", "write_file", "edit_file", "delete",
                "glob", "grep", "execute", "task", "write_todos",
            }),
            general_purpose_subagent=GeneralPurposeSubagentProfile(enabled=False),
        ),
    )
    submilli = SubmilliMiddleware(blueprint=required("SUBMILLI_BLUEPRINT"), variables={})
    try:
        await submilli.client.blueprints.prompt(submilli.blueprint)
        print("Connected to Submilli.")
        if sys.argv[1:] == ["--check"]:
            return
        agent = create_deep_agent(
            model=model_name, tools=[], subagents=[], middleware=[submilli],
        )
        prompt = " ".join(sys.argv[1:]) or 'Run this program through Submilli and report its result: export function main(): number { return 2 + 2; }'
        result = await agent.ainvoke({"messages": [{"role": "user", "content": prompt}]})
        for message in result["messages"]:
            if message.type == "ai" and message.tool_calls:
                print("Tool calls:", message.tool_calls)
            elif message.type == "tool":
                print("Tool result:", message.tool_call_id, message.content)
        print("Agent answer:", result["messages"][-1].content)
    finally:
        await submilli.client.aclose()


asyncio.run(main())
```

Check the connection without a model request:

```sh
python submilli-agent.py --check
```

Look for `Connected to Submilli.`. This confirms the server and Blueprint are reachable.

## Try it out

From the project directory with the existing environment and dependencies active,
run the saved file:

```sh
python submilli-agent.py
```

Look for a Submilli execution tool call containing `2 + 2` and its returned
value of `4`. This verifies that the agent ran the program through Submilli.
An answer of `4` without a tool call does not verify the connection.

## Troubleshooting

- Connection refused: check `SUBMILLI_SERVER` and whether the server is running.
- `401`: use a token accepted by that server.
- Blueprint not found: check `SUBMILLI_BLUEPRINT` against the registered name.
- Model authentication error: check the model provider’s credentials in this terminal.
- Tool result but no final answer: read the SDK error or turn-limit result printed by the file.

See [server settings](/docs/reference/server-settings) for server configuration.
