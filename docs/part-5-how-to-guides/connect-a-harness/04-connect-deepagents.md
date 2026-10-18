---
title: "Connect deepagents to Submilli"
description: "Add Submilli tools to an existing LangChain deepagents harness."
slug: how-to/connect-deepagents
sidebar:
  order: 4
authorship:
  label: ai-generated
  confirmed: true
  contentHash: "5ef1c2a5d3b50d308a943d46686184f4c9a94b4695f9268fe1ab22e78ee1b9da"
  confirmedAt: "2026-10-18T13:09:05.218103+00:00"
# Unpublished AI draft. Human prose rewrite required before publication.
---

Add Submilli to your deepagents harness so the agent can run code through it.
A harness is the software that runs your agent and handles its tool calls.
The connection uses `SubmilliMiddleware`.

## Before you start

Have your harness project and these Submilli settings ready:

| Environment variable | Value |
| --- | --- |
| `SUBMILLI_SERVER` | Server address, such as `http://127.0.0.1:8128`, without a trailing slash |
| `SUBMILLI_SERVER_TOKEN` | A token accepted by that server |
| `SUBMILLI_BLUEPRINT` | The name of a registered Blueprint |

Set these variables in the process that runs your harness. A Blueprint sets
the permissions for code sent to Submilli. If needed,
[run the server](/docs/server/run-the-server) and
[register a Blueprint](/docs/server/register-a-blueprint) first.

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
See [Blueprint variables](/docs/reference/blueprint-file#variables) for the
values your Blueprint needs.

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
