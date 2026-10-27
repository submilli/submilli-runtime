---
title: "Connect OpenAI Agents"
description: "Connect an OpenAI Agents SDK application to a registered Submilli Blueprint with openai-agents-submilli."
slug: tutorials/connect-openai-agents
docType: how-to
sidebar:
  order: 5
authorship:
  label: ai-assisted
  confirmed: true
  contentHash: "bf4109995a4852742915f1b5d91a82e4c7737d3f3d592d9bbca95e7f3f6c7f4a"
  confirmedAt: "2026-10-05T13:01:53.009Z"
---

## Use your coding agent

Choose an icon to open a prefilled prompt. Select your project, review, and send.

```text agent-prompt
Read https://submilli.ai/docs/tutorials/connect-openai-agents/ as the source for this setup.

Inspect my project and follow this guide to create agent.py.
Reuse valid local Submilli connection settings and provider credentials. If
SUBMILLI_SERVER, SUBMILLI_SERVER_TOKEN, or SUBMILLI_BLUEPRINT is missing, ask
for that value or its local configuration source. Do not invent values,
expose secrets, create a Blueprint, or change the server. Ask for missing
provider credentials without printing them. Install only the
missing dependencies shown in this guide. Preserve its tool restrictions and
session cleanup. Run the file with --check to verify the connection without
a model request.
Leave the files ready for the exact Try it out command.
```

After setup, [try it out](#try-it-out).

## Set up manually

The files read this terminal's environment; they do not automatically load
`.env` files. Use your running server's base URL and registered Blueprint.
Replace these values before running the files:

```sh
export SUBMILLI_SERVER='your-server-base-url'
export SUBMILLI_SERVER_TOKEN='your-server-token'
export SUBMILLI_BLUEPRINT='your-registered-blueprint'
export OPENAI_API_KEY='your-openai-api-key'
```

Connect an OpenAI Agents SDK application to Submilli, ask it to run a small
arithmetic program, and close the server session when the conversation
ends. You need Python 3.11 or later, `submilli-server` 0.4.1 or later, an
OpenAI API key, and a Blueprint already registered on your server. For this
example, use a Blueprint with no required variables. The calculation does not
need Package or tool permissions.

Start the [server](/docs/server/run-the-server), [create a Blueprint with no
grants](/docs/blueprints/start-a-blueprint#start-from-nothing-allowed), and
[register it](/docs/server/register-a-blueprint#register-it) before running this
example. Keep `SUBMILLI_SERVER_TOKEN` exported in the shell where you run this
example.

If your Blueprint requires variables or harness secrets, pass them with the
toolkit's `variables=` and `secrets=` callables, which read the `context` you
pass to `Runner.run`. The values are bound when the session opens; the model
cannot set them. The [`openai-agents-submilli` toolkit reference](https://github.com/submilli/submilli-python/tree/main/packages/openai-agents-submilli#toolkit-reference)
describes the binding options.

### Install the integration

Create a project and install the Submilli toolkit, which installs the OpenAI
Agents SDK with it:

```sh
mkdir openai-agents && cd openai-agents
python3 -m venv .venv
. .venv/bin/activate
pip install openai-agents-submilli
```

### Write the agent

Create `agent.py` in the project directory and paste the code below:

```python title="agent.py"
"""An OpenAI Agents SDK application backed by Submilli."""

import asyncio
import os
import sys
import uuid

from agents import Agent, Runner
from openai_agents_submilli import SubmilliTools

args = sys.argv[1:]
checking = args == ["--check"]
prompt = " ".join(args)
if not checking and not prompt.strip():
    raise SystemExit("Usage: python agent.py '<message>' (or --check)")

BLUEPRINT = os.environ["SUBMILLI_BLUEPRINT"]


async def main() -> None:
    submilli = SubmilliTools(blueprint=BLUEPRINT)
    try:
        if checking:
            await submilli.client.blueprints.prompt(BLUEPRINT)
            print("Connected to Submilli.")
            return
        agent = Agent(
            name="submilli-agent",
            # Change this model ID to use another OpenAI model.
            model="gpt-5-mini",
            instructions="Use Submilli to run the requested TypeScript code.",
            tools=submilli.tools,
        )
        conversation_id = str(uuid.uuid4())
        try:
            result = await Runner.run(
                agent,
                prompt,
                context={"conversation_id": conversation_id},
                max_turns=20,
            )
            for item in result.new_items:
                if item.type == "tool_call_output_item":
                    print(f"Submilli tool result: {item.output}", file=sys.stderr)
            print(result.final_output)
        finally:
            await submilli.aclose_conversation(conversation_id)
    finally:
        await submilli.aclose()


if __name__ == "__main__":
    asyncio.run(main())
```

`SubmilliTools` reads the server address from `SUBMILLI_SERVER` or uses
`http://127.0.0.1:8128`, and reads the token from `SUBMILLI_SERVER_TOKEN`.
Before the first model call, it fetches the tool descriptions for the
Blueprint from the server.

The `context` you pass to `Runner.run` is for your code, not for the model.
The toolkit reads `conversation_id` from it: programs in one conversation
share a server session, and later turns that pass the same id reuse it.
`aclose_conversation` closes that session in the `finally` block, and
`aclose` closes the HTTP client. To keep sessions across restarts of your
application, give the toolkit a `store`, as the
[sessions reference](https://github.com/submilli/submilli-python/tree/main/packages/openai-agents-submilli#sessions-retries-and-results)
describes.

`tools=submilli.tools` gives the model only Submilli's tools. Keep application
tools out of `Agent.tools` if their calls must pass through Blueprint
permissions: tools added there are called directly by the agent and bypass
Submilli. `max_turns` bounds the model's agent loop.

### Check the connection

With the setup environment active, run this without a model request:

```sh
python agent.py --check
```

Look for `Connected to Submilli.`. This checks the server, the token, and the
registered Blueprint; the model runs only when you supply a message.

## Try it out

Run this from the project directory with the setup environment active:

```sh
python agent.py \
  'Run this program through Submilli and report its result:
export function main(): number { return 2 + 2; }'
```

The agent should make a `submilli__typescript__execute` tool call and print
its result on standard error. The final answer should contain:

```text
4
```

The model passed the supplied program to Submilli, which compiled and ran it in the
registered Blueprint's session. Check the printed tool result as well
as the final answer to confirm that the connection completed.

You now have an OpenAI Agents SDK application whose program execution is
governed by an already registered Blueprint and whose conversation closes its
server session. For the same connection through another harness, see the
[Deep Agents](/docs/tutorials/connect-deepagents) tutorial.

## Use application tools

<a id="your-tools-in-programs"></a>

To reuse application tools under Blueprint permissions, pass your function
tools to `SubmilliTools`'s `ptc` parameter. They become callable from
Submilli programs; the toolkit does not expose them directly to the model.
Keep them out of `Agent.tools`: adding the same tools there creates a direct
path that bypasses Submilli.

Each call from a program runs as an ordinary tool call of the SDK, with your
run's context. A tool created with `needs_approval` pauses the run at each call
a program makes to it: the run result's `interruptions` holds the call, and you
approve or reject it on the run state and run again, as for a direct tool
call. The [harness tools overview](/docs/tutorials/connect-a-harness#call-harness-tools-from-programs)
describes the shared lifecycle; the [`openai-agents-submilli` reference](https://github.com/submilli/submilli-python/tree/main/packages/openai-agents-submilli#application-tools)
covers the integration contract.
