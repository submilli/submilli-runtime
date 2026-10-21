# Deep Agents

Read [harnesses](harnesses.md) first. Deep Agents is the model loop and orchestration layer; Submilli is the typed, policy checked execution boundary. Keep the application's provider, authentication, and framework choices.

In Python, connect with `langchain-submilli`: its `SubmilliMiddleware` adds Submilli's tools to `create_deep_agent` (or LangChain's `create_agent`) and runs every program on submilli-server over the HTTP API. Use the MCP route ("Alternative: MCP with langchain-mcp-adapters", below) only for an application that already loads Submilli through `langchain-mcp-adapters`, and for Deep Agents in TypeScript, which has no middleware package.

## Before writing the harness

For a new project, follow [setup](setup.md), then [packages](packages.md), then [blueprints](blueprints.md). The [shared harness setup](harnesses.md) builds the `@pied-piper/billing` / `support-read` fixture these examples use. A useful first slice is a fixture package whose exported function has a `@capability` annotation and calls `check(...)`. Run `submilli build check`, `submilli build test`, and `submilli blueprint lint` before involving a model.

The middleware needs submilli-server 0.4.1 or later and Python 3.11 or later. It is async only: call the agent with `ainvoke` or `astream`. Use the versions the application already has; for a new project install the current releases. These examples were verified on 2026-10-15 with `langchain-submilli 0.1.0`, `deepagents 0.7.25`, `langchain 1.4.6` and submilli-server 0.4.1; if an import or signature differs, the installed version's official API wins over this text. The package README is the API reference: [langchain-submilli](https://github.com/submilli/submilli-python/tree/main/packages/langchain-submilli#readme). Deep Agents requires a tool-calling model.

## Runnable Python harness

```sh
python3 -m venv .venv
. .venv/bin/activate
python -m pip install "langchain-submilli[deepagents]" langchain-openai
export OPENAI_API_KEY='...'
export SUBMILLI_SERVER='http://127.0.0.1:8128'
# SUBMILLI_SERVER_TOKEN must already hold the token the server was started with.
```

The middleware reads `SUBMILLI_SERVER`, not the `SUBMILLI_SERVER_URL` the MCP recipes and the fixture's export line use; set it to the fixture's URL when testing against it. Save as `agent.py` and run `python agent.py`:

```python
import asyncio
import os
import re
from typing import TypedDict

from deepagents import create_deep_agent
from langchain_openai import ChatOpenAI
from langchain_submilli import SubmilliMiddleware
from langgraph.checkpoint.memory import InMemorySaver


class Context(TypedDict):
    customer_id: str


def customer_binding(context: Context | None) -> dict[str, str]:
    # The run's context comes from the authenticated request, never from chat.
    value = (context or {}).get("customer_id", "")
    if not re.fullmatch(r"cus_[A-Za-z0-9_-]+", value):
        raise ValueError("the run's context has no valid customer_id")
    return {"customerId": value}


# Server and token come from SUBMILLI_SERVER and SUBMILLI_SERVER_TOKEN.
submilli = SubmilliMiddleware(
    blueprint=os.environ.get("SUBMILLI_BLUEPRINT", "support-read"),
    variables=customer_binding,
    hide_filesystem_tools=True,
)

# Built once and shared: each run brings its customer in its context.
agent = create_deep_agent(
    model=ChatOpenAI(model=os.getenv("OPENAI_MODEL", "gpt-5.5")),
    system_prompt="Use the Submilli tools for business data; never bypass them with shell, HTTP, or SDK calls.",
    middleware=[submilli],
    checkpointer=InMemorySaver(),
    context_schema=Context,
)


async def main() -> None:
    customer_id = "pied_piper"  # demo only: take it from the signed-in session
    config = {"configurable": {"thread_id": f"customer:{customer_id}"}}
    try:
        result = await agent.ainvoke(
            {"messages": [{"role": "user", "content": f"Read the balance for customer {customer_id} and report the amount."}]},
            config,
            context=Context(customer_id=customer_id),
        )
        for message in result["messages"]:
            if message.type == "tool" and message.artifact:
                print("program run:", message.artifact.get("execution_id"))
        print("final:", result["messages"][-1].text)
    finally:
        await submilli.aclose_thread(agent, config)


if __name__ == "__main__":
    asyncio.run(main())
```

The model should call `submilli__typescript__execute` with a program that imports `@pied-piper/billing`, and report `6150` (or the service's formatted equivalent). That is evidence of a live model run only when `OPENAI_API_KEY` is configured; a scripted model proves wiring only.

What the middleware does, and the rules that follow from it:

- **Identity comes from the run's context.** `variables` maps `runtime.context` to the Blueprint's variables; the default maps `user_id` to `userId`. `secrets` does the same for `harness` secrets. The model never sees or sets either. Build the agent once and share it; do not build one per user or put the identity in a header.
- **One session per thread.** The first `execute` in a thread opens a server session bound to the Blueprint and that run's variables, and stores its id in the agent's state. Pass a checkpointer and a `thread_id`: without them every `ainvoke` opens a new session, so files outside a named volume vanish between turns, and `aclose_thread` raises `ValueError: No checkpointer set`. A run whose context gives different variables than the thread's session raises `SubmilliSessionMismatchError` before the model is called; a different user gets a different thread. End the session with `await submilli.aclose_thread(agent, config)` when the conversation is over, or the Blueprint's `idle_timeout` ends it.
- **Six tools, no file tools.** The model gets `submilli__typescript__execute`, `__last_run`, `__packages__search`, `__packages__docs`, `__builtins__list` and `__builtins__docs`, with the descriptions the server writes for the Blueprint. Programs read and write files through `submilli:fs`; there are no `submilli__files__*` tools.
- **`hide_filesystem_tools=True`** removes Deep Agents' own `ls`, `read_file`, `write_file`, `edit_file`, `delete`, `glob`, `grep` and `execute`, which keep files in the conversation's state where Submilli never sees them. It does not reach the built-in `general-purpose` subagent; to cover it too, also pass `permissions=[FilesystemPermission(operations=["read", "write"], paths=["/**"], mode="deny")]` (from `deepagents.middleware.filesystem`).
- **Retries are safe.** Each execute call is sent with an `Idempotency-Key` derived from the tool call id, so a repeated call replays its recorded result. The tool message's `artifact` carries `execution_id` and `session_id`, for the server's audit trail.
- **Policy denials are results.** A denied capability returns a tool result whose JSON has `denied: true` and `error.kind` `permission_denied`, naming the capability; the agent does not raise.
- **Application tools.** `ptc=[...]` lets programs call the application's own LangChain tools as the `@tools` Package, and `interrupt_on={...}` asks a person to approve those calls through LangChain's human-in-the-loop flow. The Blueprint must grant `tools.call`. See the README's "Your tools in programs".

## Deterministic verification with a scripted model

Before a live run, keep `create_deep_agent`, its default middleware, `SubmilliMiddleware` and the local server real, and script only the model. Deep Agents calls `bind_tools`, which LangChain's stock fake chat models do not implement, so subclass one:

```python
import time

from langchain_core.language_models.fake_chat_models import GenericFakeChatModel
from langchain_core.messages import AIMessage


class Scripted(GenericFakeChatModel):
    offered: list[str] = []

    def bind_tools(self, tools, **kwargs):
        self.offered.extend(tool.name for tool in tools)
        return self


def scripted(code: str) -> Scripted:
    call = {"name": "submilli__typescript__execute", "args": {"code": code}, "id": f"call-{time.time_ns()}"}
    return Scripted(messages=iter([AIMessage(content="", tool_calls=[call]), AIMessage(content="done")]))
```

Build a graph per case with `model = scripted(code)` and `create_deep_agent(model=model, middleware=[submilli], checkpointer=InMemorySaver(), context_schema=Context)`, `ainvoke` it with a `context` and a fresh `thread_id`, end the case with `await submilli.aclose_thread(graph, config)` so no session is left open, and assert on the tool message (`next(m.text for m in result["messages"] if m.type == "tool")`):

1. Bound to `pied_piper`, a program reading `pied_piper` returns a result of `6150`.
2. The same binding reading `hooli` returns a tool result containing `permission_denied` and `pied-piper.example/balance.read`, with `"denied": true`; the agent does not raise.
3. A context without `customer_id` raises `ValueError` from `customer_binding` before any session opens. To prove the server's own check, build a middleware with `variables={}`: the server refuses the session and `ainvoke` raises `submilli.BadRequestError`. A middleware built with a `token` the server does not know raises `submilli.AuthenticationError` (HTTP 401). Assert on the raised errors.
4. `model.offered` records the model's real tool surface: the six Submilli tools and Deep Agents' `task`, and no `ls`, `read_file`, `write_file`, `edit_file`, `glob`, `grep` or `execute`.

A scripted run is adapter and policy evidence, not a live model result.

## Alternative: MCP with langchain-mcp-adapters

An application that already loads Submilli through `langchain-mcp-adapters` can keep doing so; the MCP endpoint is `http://127.0.0.1:8128/mcp/<blueprint-name>`. The middleware's rules then become the application's job. Every connection sends `Authorization: Bearer <token>`, read from `SUBMILLI_SERVER_TOKEN`. Bind identity from authenticated application state with the header `submilli-variables: customerId=pied_piper`; validate the value and reject semicolon, carriage return, and line-feed characters. Never derive this header from chat. Make a new client/session for every identity and hold one session open for the entire agent run: tools loaded with `MultiServerMCPClient.get_tools()` open a new session per call, and each program would run in a separate session. This recipe was verified on 2026-10-15 with `deepagents 0.7.25` and `langchain-mcp-adapters 0.3.2`.

```python
import asyncio
import os
import re
from typing import Any

from deepagents import create_deep_agent
from deepagents.middleware.filesystem import FilesystemPermission
from langchain_mcp_adapters.client import MultiServerMCPClient
from langchain_mcp_adapters.tools import load_mcp_tools
from langchain_openai import ChatOpenAI

def required(name: str) -> str:
    value = os.environ.get(name, "")
    if not value:
        raise RuntimeError(f"missing required environment variable {name}")
    return value

def trusted_customer_id() -> str:
    value = required("SUBMILLI_CUSTOMER_ID")
    if not re.fullmatch(r"cus_[A-Za-z0-9_-]+", value) or any(c in value for c in ";\r\n"):
        raise ValueError("SUBMILLI_CUSTOMER_ID is not a valid customer id")
    return value

async def main() -> None:
    server_url = required("SUBMILLI_SERVER_URL").rstrip("/")
    blueprint = required("SUBMILLI_BLUEPRINT")
    customer_id = trusted_customer_id()
    client = MultiServerMCPClient({"submilli": {
        "transport": "streamable_http",
        "url": f"{server_url}/mcp/{blueprint}",
        "headers": {
            "Authorization": f"Bearer {required('SUBMILLI_SERVER_TOKEN')}",
            "submilli-variables": f"customerId={customer_id}",
        },
    }})
    async with client.session("submilli") as session:
        tools = await load_mcp_tools(session)
        model = ChatOpenAI(model=os.getenv("OPENAI_MODEL", "gpt-5.5"))
        agent = create_deep_agent(
            model=model, tools=tools,
            system_prompt="Use supplied Submilli tools for business data; never bypass them with shell, HTTP, or SDK calls.",
            # Deep Agents' own file tools keep files in the conversation, outside Submilli.
            permissions=[FilesystemPermission(operations=["read", "write"], paths=["/**"], mode="deny")],
        )
        result: dict[str, Any] = await agent.ainvoke(
            {"messages": [{"role": "user", "content": f"Read the balance for customer {customer_id} and report the amount."}]},
            config={"configurable": {"thread_id": f"customer:{customer_id}"}},
        )
        print("final:", result["messages"][-1].content)

if __name__ == "__main__":
    asyncio.run(main())
```

Pass the returned tools unchanged so Submilli's names, schemas, and full descriptions reach the model. Over MCP the model also gets Submilli's `submilli__files__list` and `submilli__files__read`. The `permissions` line denies Deep Agents' own file tools; they stay in the model's tool list, and a call to one is refused. Without the `submilli-variables` header the server rejects `initialize` with HTTP 400, so entering `client.session(...)` raises an exception group before any agent exists; without `Authorization` it is a 401 raised the same way.

## Concrete TypeScript harness

`langchain-submilli` is Python only; Deep Agents in TypeScript connects over MCP. The current JS adapter does not expose Python's `session()` context. It keeps connections on `MultiServerMCPClient`; call `getTools()` after construction and `close()` in `finally` after the run.

```sh
npm install deepagents @langchain/mcp-adapters @langchain/openai
export OPENAI_API_KEY='...'
export SUBMILLI_SERVER_URL='http://127.0.0.1:8128'
export SUBMILLI_BLUEPRINT='support-read'
export SUBMILLI_CUSTOMER_ID='pied_piper'
# SUBMILLI_SERVER_TOKEN must already hold the token the server was started with.
```

`agent.ts`:

```ts
import { createDeepAgent } from "deepagents";
import { MultiServerMCPClient } from "@langchain/mcp-adapters";
import { ChatOpenAI } from "@langchain/openai";

function required(name: string): string {
  const value = process.env[name];
  if (!value) throw new Error(`missing required environment variable ${name}`);
  return value;
}
function trustedCustomerId(): string {
  const value = required("SUBMILLI_CUSTOMER_ID");
  if (!/^cus_[A-Za-z0-9_-]+$/.test(value) || /[;\r\n]/.test(value)) throw new Error("invalid customer id");
  return value;
}

async function main(): Promise<void> {
  const baseUrl = required("SUBMILLI_SERVER_URL").replace(/\/$/, "");
  const customerId = trustedCustomerId();
  const client = new MultiServerMCPClient({
    throwOnLoadError: true,
    mcpServers: { submilli: {
      url: `${baseUrl}/mcp/${required("SUBMILLI_BLUEPRINT")}`,
      headers: {
        Authorization: `Bearer ${required("SUBMILLI_SERVER_TOKEN")}`,
        "submilli-variables": `customerId=${customerId}`,
      },
      automaticSSEFallback: false,
    }},
  });
  try {
    const tools = await client.getTools("submilli");
    console.log("MCP tools:", tools.map((tool) => tool.name));
    const model = new ChatOpenAI({ model: process.env.OPENAI_MODEL ?? "gpt-5.5" });
    const agent = await createDeepAgent({
      model, tools,
      systemPrompt: "Use supplied Submilli tools for business data; never bypass them with shell, HTTP, or SDK calls.",
    });
    const result = await agent.invoke(
      { messages: [{ role: "user", content: `Read the balance for customer ${customerId} and report the amount.` }] },
      { configurable: { thread_id: `customer:${customerId}` } },
    );
    for (const message of result.messages) {
      if ("tool_calls" in message && message.tool_calls?.length) console.log("tool calls:", message.tool_calls);
    }
    console.log("final:", result.messages.at(-1)?.content);
  } finally {
    await client.close();
  }
}
main().catch((error: unknown) => { console.error(error); process.exitCode = 1; });
```

Run with `npx tsx agent.ts` or the project's existing TypeScript runner. The declarations verified on 2026-09-17 (`deepagents 1.13.5`, `@langchain/mcp-adapters 1.1.4`) confirm `createDeepAgent` is async, `getTools(...servers)` returns LangChain tools, and `close()` cleans up. Keep `automaticSSEFallback: false` when Streamable HTTP is required so transport failure is visible. The model should make the same tool call and report `6150` for the fixture.

## Defaults, backends, and subagents

Deep Agents automatically adds filesystem, summarization, and delegation middleware. The default StateBackend is thread-scoped and keeps its files in the conversation's state, outside Submilli; hide or deny those tools as above so the model keeps files through programs.

Do not give a server agent a credential-bearing FilesystemBackend, local shell, or unrestricted sandbox and then claim Submilli governs all effects. Deep Agents' own `execute` tool is separate from Submilli and can bypass blueprint policy; `hide_filesystem_tools=True` removes it. Human approval for writes requires a checkpointer; it does not replace package `check(...)`.

The built-in general-purpose subagent receives the main agent's configured tools but not its middleware. A custom subagent is a separate tool surface: explicitly give it the tools it needs, or give it none. Custom subagents are stateless, so include the complete instruction in each `task` call. Test child-agent access when delegation is enabled.

## Verification and troubleshooting

Run in order: start the server; run one allowed program with a scripted model; repeat it with a cross-identity value; leave the identity out of the context; then run one model task and inspect tool messages and final output. `connection refused` means the server or port is wrong; the middleware reads `SUBMILLI_SERVER` and defaults to `http://127.0.0.1:8128`. `AuthenticationError` (401) means `SUBMILLI_SERVER_TOKEN` is missing or not one of the server's tokens. `BadRequestError` when the session opens means a required variable is missing: check `variables` and the context. `SubmilliSessionMismatchError` means a thread was reused for another identity. Files that vanish between turns, or `ValueError: No checkpointer set` from `aclose_thread`, mean the agent has no checkpointer. A note the model says it saved but the server never wrote means Deep Agents' file tools are still visible. A model tool error means the provider/model lacks tool calling.

Authoritative references: [langchain-submilli README](https://github.com/submilli/submilli-python/tree/main/packages/langchain-submilli#readme), [Deep Agents Python quickstart](https://docs.langchain.com/oss/python/deepagents/quickstart), [Deep Agents customization](https://docs.langchain.com/oss/python/deepagents/customization), [LangChain Python MCP](https://docs.langchain.com/oss/python/langchain/mcp), [Deep Agents JavaScript customization](https://docs.langchain.com/oss/javascript/deepagents/customization), and the [LangChain JS MCP adapter](https://github.com/langchain-ai/langchainjs/tree/main/libs/langchain-mcp-adapters).
