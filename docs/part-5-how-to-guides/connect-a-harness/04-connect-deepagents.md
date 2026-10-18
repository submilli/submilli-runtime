---
title: "Connect deepagents to Submilli"
description: "Add Submilli tools to an existing LangChain deepagents harness."
showDescription: false
slug: how-to/connect-deepagents
sidebar:
  order: 4
authorship:
  label: ai-generated
  confirmed: true
  contentHash: "23944832279eac4935f16b3ce0c4243fc745e7a2e16f08a1ec331814d15ea9d4"
  confirmedAt: "2026-10-18T13:09:05.218103+00:00"
# Unpublished AI draft. Human prose rewrite required before publication.
---

Use `SubmilliMiddleware` to connect your existing deepagents agent to the Submilli runtime.
[Middleware](https://docs.langchain.com/oss/python/langchain/middleware/overview)
lets you add code before or after an agent's model and tool calls.

:::note[Before you start]
- An existing deepagents project with a configured model.
- A [running Submilli server](/docs/server/run-the-server) and a token it accepts.
- A [registered Blueprint](/docs/server/register-a-blueprint), which sets the
  permissions for code sent to Submilli.
:::

## Set connection variables

Use the server address, token, and Blueprint name prepared in **Before you start**.
Assign those values to these environment variables in the process that runs your agent:

| Environment variable | Value |
| --- | --- |
| `SUBMILLI_SERVER` | Server address, such as `http://127.0.0.1:8128`, without a trailing slash |
| `SUBMILLI_SERVER_TOKEN` | Server token |
| `SUBMILLI_BLUEPRINT` | Registered Blueprint name |

## Use your coding agent (optional)

The [skill bundled with Submilli 0.4.1](https://github.com/submilli/submilli-runtime/blob/v0.4.1/skills/submilli/references/deepagents.md)
connects deepagents over [MCP](https://docs.langchain.com/oss/python/langchain/mcp),
a protocol for calling server tools. The manual steps below use native middleware.

[Install the Submilli skill](/docs/install#the-skill), restart your coding agent,
and open this project. Make the prepared values available to its terminal through
your local environment or existing project configuration, then give it this prompt:

```text title="Prompt for your coding agent"
Use the Submilli skill to connect this existing deepagents project
to Submilli over MCP. Reuse the prepared server and Blueprint. Read
SUBMILLI_SERVER, SUBMILLI_SERVER_TOKEN, and SUBMILLI_BLUEPRINT from the
local environment or project configuration. Map those values to the
integration's settings.
Keep my model and existing agent configuration. Keep token values out of
source code and responses. Verify the connection without a model request.
```

For manual setup, continue below.

## Install the integration

In your project's Python environment, install the deepagents integration:

```sh
python -m pip install "langchain-submilli[deepagents]>=0.1"
```

## Add Submilli to your agent

Import `SubmilliMiddleware` and add it to the `middleware` argument of your
existing `create_deep_agent` call. Keep your model, prompt, and other agent
settings. In this fragment, `model` is the model you already configured.

```python title="Agent configuration (fragment)"
import os

from deepagents import create_deep_agent
from langchain_submilli import SubmilliMiddleware

submilli = SubmilliMiddleware(
    blueprint=os.environ["SUBMILLI_BLUEPRINT"],
    variables={},
)

agent = create_deep_agent(
    model=model,
    middleware=[submilli],
)
```

The middleware reads `SUBMILLI_SERVER` and `SUBMILLI_SERVER_TOKEN` from the
environment. If you already use middleware, append `submilli` to that list.
Replace `variables={}` with the values your Blueprint requires, if any.
See [Blueprint variables](/docs/reference/blueprint-file#variables).

If your Blueprint requires secrets from your application (the `harness`
source), pass a mapping of their names to values in the middleware's
`secrets` argument. See
[Blueprint secrets](/docs/reference/blueprint-file#secrets).

Run the agent through its async API. The middleware does not support
`agent.invoke` or `agent.stream`. In your existing async call, use
`ainvoke` (or `astream`) with your application's messages:

```python title="Agent call (fragment)"
result = await agent.ainvoke({"messages": messages})
```

## Verify the connection

Before starting an agent run, fetch the Blueprint's tool description through
the same client. Run this in your application's async startup code after
creating `submilli`:

```python
await submilli.client.blueprints.prompt(submilli.blueprint)
print("Connected to Submilli.")
```

`Connected to Submilli.` confirms that the server address, token, and
Blueprint work. No model call is made by this request. The middleware makes
`submilli__typescript__execute` available when your agent runs.

Keep the client open while the agent uses it. Close it during application
shutdown:

```python
await submilli.client.aclose()
```

## Troubleshooting

- Connection refused: check `SUBMILLI_SERVER` and whether the server is running.
- `401`: use a token accepted by that server.
- Blueprint not found: check `SUBMILLI_BLUEPRINT` against the name registered
  on that server.

See [server settings](/docs/reference/server-settings) for server configuration.
