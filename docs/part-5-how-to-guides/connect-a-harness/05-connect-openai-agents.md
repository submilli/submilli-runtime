---
docType: how-to
title: "Connect OpenAI Agents SDK to Submilli"
description: "Create a OpenAI Agents SDK agent that uses Submilli for permitted code execution."
showDescription: false
slug: how-to/connect-openai-agents
sidebar:
  order: 5
authorship:
  label: ai-generated
  confirmed: true
  contentHash: "d5ec40970e9ddd761f6bc376b2cfd103a526ccfe6d271a58055fe53ea319dd8f"
  confirmedAt: "2026-10-18T13:09:05.218103+00:00"
# Unpublished AI draft. Human prose rewrite required before publication.
---

Create a fresh OpenAI Agents SDK agent with only Submilli execution tools, then
run a small program and see its result in your terminal.

## Use your coding agent

Open a project directory in your coding agent.
[Install the Submilli skill](/docs/install#the-skill) if needed, then restart
the coding agent to load it. Give it this prompt:

```text agent-prompt
Use the Submilli skill and this guide to create a fresh OpenAI Agents SDK demo
in a new submilli-demo directory. Preserve my existing agent configuration.
Create the complete submilli-agent.py file shown in this guide, including its
command-line prompt and tool-result logging. Give it only Submilli execution
tools and apply this guide’s framework-specific exclusions.
Inspect my project’s provider/model setup. Help me choose any missing model
configuration and credentials without guessing values or revealing secrets.
Find a local Submilli server and token, or help me prepare them.
Download https://submilli.ai/docs/examples/permission-demo/basic-blueprint.yaml and
register it using submilli server blueprint apply. Use harness-demo as
SUBMILLI_BLUEPRINT. This first example requires no Package.
Install the dependencies shown here. Verify the server and basic program
without making a model request. Leave the runnable file ready for the exact
Try it out command, and tell me the directory to open and any remaining
human setup. Do not run the optional billing scenario yet.
```

After setup, [try it out](#try-it-out). To set up the same file yourself,
follow the manual path.

<details>
<summary>Connecting an existing agent</summary>

Replace its execution tools with Submilli. Remove other tool providers,
file and shell tools, network tools, and delegation routes that can perform
operations outside Submilli. Blueprint permissions apply to code run by
Submilli; leaving another execution route available bypasses that boundary.

Replace `tools` and `handoffs` with empty lists and `mcp_servers` with
`[submilli]`. Remove hosted tools and agents exposed as tools too.

</details>

## Set up manually

You need the [Submilli CLI](/docs/install), a running
[local server](/docs/server/run-the-server), and a token it accepts.
You also need Python 3.11 or later and model access for OpenAI Agents SDK.

### Prepare a demo directory

Run these commands from the directory where you keep your projects:

```sh
mkdir submilli-demo
cd submilli-demo
python3 -m venv .venv
. .venv/bin/activate
python -m pip install "openai-agents>=0.3"
curl -fsSLo blueprint.yaml https://submilli.ai/docs/examples/permission-demo/basic-blueprint.yaml
```

Keep this terminal in `submilli-demo` for the rest of the guide.
Set these environment variables in this terminal using your own configuration:

| Variable | Value |
| --- | --- |
| `SUBMILLI_SERVER` | Your server URL, such as `http://127.0.0.1:8128` |
| `SUBMILLI_SERVER_TOKEN` | A token accepted by that server |
| `SUBMILLI_BLUEPRINT` | `harness-demo` |
| `OPENAI_MODEL` | An OpenAI model ID available to your account. Export your existing `OPENAI_API_KEY` in this terminal. This file uses the SDK’s default OpenAI provider. |

The model credentials are separate from the Submilli server token. Reuse your
chosen provider’s setup. Do not put credentials in the file you save next.
Register the downloaded Blueprint:

```sh
export SUBMILLI_BLUEPRINT=harness-demo
SUBMILLI_SERVER_URL="$SUBMILLI_SERVER" submilli server blueprint apply blueprint.yaml
```

### Save the runnable agent

Save this complete file as `submilli-demo/submilli-agent.py`. If you are already
inside `submilli-demo`, save it as `submilli-agent.py`.

```python title="submilli-agent.py"
import asyncio
import os
import sys


def required(name):
    value = os.environ.get(name)
    if not value:
        raise RuntimeError(f"Set {name} in this terminal before running this file.")
    return value

from agents import Agent, Runner
from agents.mcp import MCPServerStreamableHttp


async def main():
    model = required("OPENAI_MODEL")
    required("OPENAI_API_KEY")
    server = required("SUBMILLI_SERVER").rstrip("/")
    blueprint = required("SUBMILLI_BLUEPRINT")
    token = required("SUBMILLI_SERVER_TOKEN")
    async with MCPServerStreamableHttp(
        name="submilli",
        params={
            "url": f"{server}/mcp/{blueprint}",
            "headers": {"Authorization": f"Bearer {token}"},
        },
        client_session_timeout_seconds=120,
    ) as submilli:
        tools = await submilli.list_tools()
        if not any(tool.name == "submilli__typescript__execute" for tool in tools):
            raise RuntimeError("Submilli's execute tool was not found")
        print("Connected to Submilli.")
        if sys.argv[1:] == ["--check"]:
            return
        agent = Agent(
            name="Submilli demo", model=model, tools=[], handoffs=[],
            mcp_servers=[submilli],
            instructions="Run requested programs through Submilli and report tool results.",
        )
        prompt = " ".join(sys.argv[1:]) or 'Run this program through Submilli and report its result: export function main(): number { return 2 + 2; }'
        result = await Runner.run(agent, prompt, max_turns=10)
        for item in result.new_items:
            if item.type in ("tool_call_item", "tool_call_output_item"):
                print(item.type, item.raw_item)
        print("Agent answer:", result.final_output)


asyncio.run(main())
```

Check the connection without a model request:

```sh
python submilli-agent.py --check
```

Look for `Connected to Submilli.`. This confirms the server and Blueprint are reachable.

## Try it out

1. Open a terminal in the `submilli-demo` directory created during setup. Activate its Python environment with `. .venv/bin/activate`. Use the same environment variables as during setup.
2. Run the saved agent. Its default request asks Submilli to execute `export function main(): number { return 2 + 2; }`:

   ```sh
   python submilli-agent.py
   ```

3. Read the printed tool call and its matching result, then the agent’s answer. Look for the Submilli execution tool, the program containing `2 + 2`, and a returned value of `4`. An answer of `4` without an execution call does not verify the connection.

The file prints tool calls and results automatically. Match a call’s ID to
its result’s ID. MCP results may contain the value inside a text block.
If no execution call appears, run this explicit request:

```sh
python submilli-agent.py 'Use the Submilli execution tool now. Run export function main(): number { return 2 + 2; } and report the tool result. Do not calculate the answer without a tool call.'
```

<details>
<summary>Optional scenario 2: allow one customer and reject another</summary>

Keep the same agent file. This scenario adds a billing Package containing
local sample charges and a Blueprint permitting `cus_northwind` only.
No billing credentials are needed. Run the Package commands under the same
user and `SUBMILLI_HOME` as your local server.

From `submilli-demo`, prepare the optional demo:

```sh
mkdir permission-demo
cd permission-demo
curl -fsSLo blueprint.yaml https://submilli.ai/docs/examples/permission-demo/blueprint.yaml
curl -fsSLo billing-source.zip https://submilli.ai/docs/examples/permission-demo/billing-source.zip
unzip billing-source.zip
submilli build publish-local
SUBMILLI_SERVER_URL="$SUBMILLI_SERVER" submilli server blueprint apply blueprint.yaml
cd ..
export SUBMILLI_BLUEPRINT=permission-demo
```

Run the allowed request:

```sh
python submilli-agent.py 'Use the billing Package through Submilli to list charges for customer cus_northwind. Report the runtime result.'
```

Look for `ch_a1` (4900) and `ch_a2` (1250) in the printed Submilli tool result.
Then run the denied request:

```sh
python submilli-agent.py 'Use the billing Package through Submilli to list charges for customer cus_initech. Attempt the operation and report the runtime result. Keep the customer ID and Blueprint permissions unchanged.'
```

Look for a tool result with `permission_denied` for `acme.com/charges.list`
and no charges. A model refusal without a tool call does not test the runtime.

To test the rejection directly, including when the model refuses to call the
tool, run this complete request from the same terminal:

```sh
curl -fsS "$SUBMILLI_SERVER/v1/execute" \
  -H "Authorization: Bearer $SUBMILLI_SERVER_TOKEN" \
  -H "Content-Type: application/json" \
  --data '{"blueprint":"permission-demo","code":"import { listCharges, Charge } from \"@acme/billing\"; function main(): Charge[] { return listCharges(\"cus_initech\"); }"}'
```

Check for `error.kind` equal to `"permission_denied"`, Capability
`acme.com/charges.list`, and `result: null`. Older runtimes may return
`runtime_error` with a `PermissionDeniedError` message instead. This direct
request verifies the runtime’s rejection independently of the model.

Return to the basic example with `export SUBMILLI_BLUEPRINT=harness-demo`.

</details>

## Troubleshooting

- Connection refused: check `SUBMILLI_SERVER` and whether the server is running.
- `401`: use a token accepted by that server.
- Blueprint not found: check `SUBMILLI_BLUEPRINT` against the registered name.
- Model authentication error: check the model provider’s credentials in this terminal.
- Tool result but no final answer: read the SDK error or turn-limit result printed by the file.

See [server settings](/docs/reference/server-settings) for server configuration.
