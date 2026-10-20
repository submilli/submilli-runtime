---
title: "Follow a code execution through a harness"
docType: tutorial
description: "A local exercise with a scripted model and the OpenAI Agents SDK."
slug: tutorials/follow-a-code-execution
sidebar:
  order: 7
verification:
  model: "Scripted model, no provider requests. Verified with submilli-server 0.4.2 and the pinned Python packages on 2026-10-20; non-loopback connections blocked."
  editorial: "Draft. Human rewrite and review required before publication."
authorship:
  label: ai-generated
  confirmed: true
  contentHash: "b161e3a20aff4f2fbe95c354de7fa0e25f929b0662701462a0098ba662645d03"
  confirmedAt: "2026-10-18T14:35:00Z"
---

:::caution[Draft]
This AI-generated tutorial needs a human rewrite and review before publication.
:::

In this tutorial we will calculate `19 + 23` with the OpenAI Agents SDK and inspect each handoff. The SDK sends tool definitions to the model, dispatches its tool requests, and returns results to it.

We will substitute a scripted model that always requests the same program. The Submilli runtime will compile and run the program. No model provider is contacted, and no provider key is needed. This makes the handoffs repeatable, but does not demonstrate how a live model chooses a tool or writes code.

You need Python 3.12 or later, basic Python and TypeScript, and the `submilli` and `submilli-server` binaries from [Install](/docs/install). Use a macOS or Linux shell for the commands.

## 1. Start an isolated server

Make a working directory. Set `SUBMILLI_HOME` to keep this exercise's server state separate from your usual Submilli files.

```sh
mkdir follow-execution
cd follow-execution
export SUBMILLI_HOME="$PWD/.submilli"
export SUBMILLI_SERVER_TOKEN=tutorial-local-only-000000000000000000000000
submilli-server --bind 127.0.0.1 --port 18137
```

The fixed token is for this local exercise. `--bind` restricts the listener to your computer, and `--port` selects the port the Python program will use. Leave this terminal running.

Open another terminal in `follow-execution`. Save this as `blueprint.yaml`:

```yaml title="blueprint.yaml"
kind: blueprint
name: sum
default: deny
```

This Blueprint grants no Capabilities. Arithmetic does not need them. Register it with the server using its address and the same local token:

```sh
export SUBMILLI_SERVER_TOKEN=tutorial-local-only-000000000000000000000000
submilli server blueprint apply blueprint.yaml --server http://127.0.0.1:18137
```

```text
Added blueprint 'sum'
```

## 2. Save the harness exercise

Create a Python environment and install the SDK version used here. Activating the environment makes `python` and `pip` use that environment.

```sh
python3.12 -m venv .venv
. .venv/bin/activate
pip install "openai-agents==0.8.4" "openai==2.19.0" "mcp==1.30.0" "pydantic==2.12.3"
```

Save this as `trace.py`. `ObservedServer` prints requests the harness actually dispatches. `ScriptedModel` saves the tools and instructions it receives, requests one execution, then prints the returned result. Tracing is disabled so the SDK does not upload the exercise to a provider.

```python title="trace.py"
import asyncio
import builtins
import json
from pathlib import Path

from agents import Agent, ModelResponse, Runner, Usage, set_tracing_disabled
from agents.mcp import MCPServerStreamableHttp
from agents.models.interface import Model
from openai.types.responses import (
    ResponseFunctionToolCall, ResponseOutputMessage, ResponseOutputText,
)

EXECUTE = "submilli__typescript__execute"
CODE = "function main(): number { return 19 + 23; }"
TOKEN = "tutorial-local-only-000000000000000000000000"


class ObservedServer(MCPServerStreamableHttp):
    async def call_tool(self, tool_name, arguments, meta=None):
        print("Harness sends:", json.dumps({"name": tool_name, "arguments": arguments}))
        return await super().call_tool(tool_name, arguments, meta)


class ScriptedModel(Model):
    async def get_response(self, system_instructions, input, model_settings, tools, *args, **kwargs):
        outputs = [item for item in input if isinstance(item, dict)
                   and item.get("type") == "function_call_output"]
        if outputs:
            output = str(outputs[-1]["output"])
            print("Model receives:", output)
            item = ResponseOutputMessage(
                id="answer", type="message", role="assistant", status="completed",
                content=[ResponseOutputText(type="output_text", text=output, annotations=[])],
            )
        else:
            catalog = [{"name": tool.name, "description": tool.description,
                        "parameters": tool.params_json_schema} for tool in tools]
            Path("tools.json").write_text(json.dumps(catalog, indent=2))
            Path("instructions.txt").write_text(system_instructions or "")
            print("Saved model input to tools.json and instructions.txt.")
            builtins.input("Inspect those files, then press Enter to request execution. ")
            arguments = json.dumps({"code": CODE})
            print("Model requests:", json.dumps({"name": EXECUTE, "arguments": {"code": CODE}}))
            item = ResponseFunctionToolCall(
                type="function_call", call_id="sum-1", name=EXECUTE, arguments=arguments,
            )
        return ModelResponse(output=[item], usage=Usage(), response_id=None)

    def stream_response(self, *args, **kwargs):
        raise NotImplementedError("This exercise uses Runner.run, without streaming.")


async def main():
    set_tracing_disabled(True)
    async with ObservedServer(
        name="submilli",
        params={"url": "http://127.0.0.1:18137/mcp/sum",
                "headers": {"Authorization": f"Bearer {TOKEN}"}},
        use_structured_content=True,
        client_session_timeout_seconds=120,
    ) as server:
        agent = Agent(
            name="sum",
            instructions="Use Submilli to calculate the sum. Return the result.",
            model=ScriptedModel(),
            mcp_servers=[server],
        )
        await Runner.run(agent, "What is 19 + 23?", max_turns=2)


asyncio.run(main())
```

## 3. Inspect what the model receives

Run the program:

```sh
python trace.py
```

It pauses before requesting execution:

```text
Saved model input to tools.json and instructions.txt.
Inspect those files, then press Enter to request execution.
```

Open `tools.json` in your editor. These are the tool definitions passed to the model callback by the harness. Find `submilli__typescript__execute` and inspect its `description` and `parameters`.

The description begins:

```text
Run a submilli program — strict TypeScript subset compiled to
WebAssembly.
```

Read the rest of that description in the file. Notice that the language instructions arrive with the tool. Submilli builds this description from its maintained instructions and the selected Blueprint. You do not paste those instructions into the harness yourself.

The execution tool's `parameters` contain this schema (with its `$schema` and `title` fields omitted here):

```json
{
  "type": "object",
  "properties": {
    "code": {
      "description": "TypeScript-subset source with a `main()` entry point. Its return value\nis the tool result.",
      "type": "string"
    }
  },
  "required": ["code"],
  "additionalProperties": false
}
```

This schema tells the model which arguments it may send. Notice that it takes source code. It has no argument for choosing a Blueprint or granting permissions.

Open `instructions.txt` too:

```text
Use Submilli to calculate the sum. Return the result.
```

You supplied this task instruction in `Agent(instructions=...)`. The SDK fetched the tool definitions through MCP from the connection in `mcp_servers=[server]`. The `/mcp/sum` address selects the Blueprint you registered.

Before continuing, identify the two sources of instructions. Which file contains your agent's task? Which field tells the model how to write a Submilli program?

## 4. Follow the request and result

Return to the paused terminal and press Enter. The remaining output is:

```text
Model requests: {"name": "submilli__typescript__execute", "arguments": {"code": "function main(): number { return 19 + 23; }"}}
Harness sends: {"name": "submilli__typescript__execute", "arguments": {"code": "function main(): number { return 19 + 23; }"}}
Model receives: {"console": [], "error": null, "execution_id": "3d0485e9-6d10-4991-9ebf-26c5dae250d8", "result": "42"}
```

The first line is the scripted model’s request. It names a tool and supplies the program as an argument.

The second line comes from `ObservedServer.call_tool`. The SDK dispatches the request to Submilli, which compiles and runs the program and returns the value from `main`.

The third line is the result the harness passes back on the next model call. `result` is the string `"42"`, `console` is empty, and `error` is `null`. Submilli assigns an `execution_id` to identify the run, so yours will differ. The scripted model ends the turn by echoing that result.

We set `use_structured_content=True` so the SDK returns the server's structured result as JSON text. This setting controls the result format the model receives.

Notice that the program ran under `default: deny`. Arithmetic needs no Capability, while operations such as reading a file require permission from the Blueprint. See [Blueprints](/docs/blueprints) for how those checks work.

Compare the two request lines. Find the matching tool name in `tools.json`, then find the server address in `trace.py`. The name identifies what to call. The configured address tells the harness where to send it.

## 5. Stop the exercise

The Python program closes its MCP connection when the run ends. Check the server from the terminal where you registered the Blueprint:

```sh
submilli server status --server http://127.0.0.1:18137
```

Confirm that `active sessions` is `0`, then press Ctrl+C in the server terminal to stop it.

You have traced one code request from model input to a runtime result. For a harness connection in your own application, use [Connect OpenAI Agents](/docs/tutorials/connect-openai-agents) or choose another supported harness in [Harness integrations](/docs/tutorials/connect-a-harness).
