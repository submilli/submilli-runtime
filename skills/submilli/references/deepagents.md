# Deep Agents

Read [harnesses](harnesses.md) first. Deep Agents is the model loop and orchestration layer; Submilli is the typed, policy checked execution boundary. Keep the application's provider, authentication, and framework choices.

## Before writing the harness

For a new project, follow [setup](setup.md), then [packages](packages.md), then [blueprints](blueprints.md). The [shared harness setup](harnesses.md) builds the `@acme/billing` / `support-read` fixture these examples use. A useful first slice is a fixture package whose exported function has a `@capability` annotation and calls `check(...)`. Run `submilli build check`, `submilli build test`, and `submilli blueprint lint` before involving a model. The MCP URL is `http://127.0.0.1:8128/mcp/<blueprint-name>`.

Bind identity from authenticated application state before constructing the MCP client. The header format is `submilli-variables: customerId=cus_northwind`; validate the value and reject semicolon, carriage return, and line-feed characters. Never derive this header from chat. Make a new client/session for every identity and keep it alive for the entire agent run.

Use the versions the application already has; for a new project install the current releases. These examples were verified on 2026-09-17 with `deepagents 0.7.15` and `langchain-mcp-adapters 0.3.2` (PyPI) and `deepagents 1.13.5` with `@langchain/mcp-adapters 1.1.4` (npm); if an import or signature differs, the installed version's official API wins over this text. Deep Agents requires a tool-calling model.

## Runnable Python harness

```sh
python3 -m venv .venv
. .venv/bin/activate
python -m pip install deepagents langchain-mcp-adapters langchain-openai
export OPENAI_API_KEY='...'
export SUBMILLI_SERVER_URL='http://127.0.0.1:8128'
export SUBMILLI_BLUEPRINT='support-read'
export SUBMILLI_CUSTOMER_ID='cus_northwind'
```

Save as `agent.py` and run `python agent.py`:

```python
import asyncio
import os
import re
from typing import Any

from deepagents import create_deep_agent
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
        "headers": {"submilli-variables": f"customerId={customer_id}"},
    }})
    async with client.session("submilli") as session:
        tools = await load_mcp_tools(session)
        print("MCP tools:", [tool.name for tool in tools])
        model = ChatOpenAI(model=os.getenv("OPENAI_MODEL", "gpt-4o-mini"), temperature=0)
        agent = create_deep_agent(
            model=model, tools=tools,
            system_prompt="Use supplied Submilli tools for business data; never bypass them with shell, HTTP, or SDK calls.",
        )
        result: dict[str, Any] = await agent.ainvoke(
            {"messages": [{"role": "user", "content": f"Read the balance for customer {customer_id} and report the amount."}]},
            config={"configurable": {"thread_id": f"customer:{customer_id}"}},
        )
        for message in result.get("messages", []):
            if getattr(message, "tool_calls", None):
                print("tool calls:", message.tool_calls)
        print("final:", result["messages"][-1].content)

if __name__ == "__main__":
    asyncio.run(main())
```

This uses the published signatures `MultiServerMCPClient({...})`, `async with client.session(name)`, `await load_mcp_tools(session)`, and `create_deep_agent(model=..., tools=...)`. Pass the returned tools unchanged so Submilli's names, schemas, and full descriptions reach the model. The local fixture should produce a balance tool call and `6150` (or the service's formatted equivalent). That is evidence of a live model run only when `OPENAI_API_KEY` is configured; a fake model proves adapter wiring only.

## Deterministic verification with a scripted model

Before a live run, keep `create_deep_agent`, its default middleware, the MCP session and the local server real, and script only the model. Deep Agents calls `bind_tools`, which LangChain's stock fake chat models do not implement, so use a small `BaseChatModel`:

```python
from typing import Any
from langchain_core.language_models.chat_models import BaseChatModel
from langchain_core.messages import AIMessage, BaseMessage, ToolMessage
from langchain_core.outputs import ChatGeneration, ChatResult

class ScriptedModel(BaseChatModel):
    code: str
    offered: list[str] = []

    @property
    def _llm_type(self) -> str:
        return "scripted"

    def bind_tools(self, tools: Any, **kwargs: Any) -> "ScriptedModel":
        self.offered.extend(getattr(t, "name", None) or t.get("name") or t["function"]["name"] for t in tools)
        return self

    def _generate(self, messages: list[BaseMessage], stop: Any = None, run_manager: Any = None, **kwargs: Any) -> ChatResult:
        results = [m for m in messages if isinstance(m, ToolMessage)]
        reply = AIMessage(content=f"tool said: {results[-1].content}") if results else AIMessage(
            content="", tool_calls=[{"name": "submilli__typescript__execute", "args": {"code": self.code}, "id": "c1", "type": "tool_call"}])
        return ChatResult(generations=[ChatGeneration(message=reply)])
```

Pass `ScriptedModel(code=..., offered=[])` as `model=` inside the same `async with client.session(...)` block and assert on `result["messages"][-1].content`:

1. Bound to `cus_northwind`, a program reading `cus_northwind` ends with `6150`.
2. The same binding reading `cus_initech` ends with a tool result containing `permission denied` and `acme.com/balance.read`; the adapter returns it as content, the agent does not raise.
3. Without the `submilli-variables` header the server rejects `initialize` with HTTP 400, so entering `client.session(...)` raises an exception group before any agent exists. Assert on the raised error.
4. `offered` records the model's real tool surface. With the defaults verified above it is Submilli's eight MCP tools plus `ls`, `read_file`, `write_file`, `edit_file`, `delete`, `glob`, `grep` and `task`. There is no `execute` tool under the default StateBackend; assert that, because a backend that adds it opens a path around blueprint policy.

`Session termination failed: 202` in the adapter log on close is harmless. A scripted run is adapter and policy evidence, not a live model result.

## Concrete TypeScript harness

The current JS adapter does not expose Python's `session()` context. It keeps connections on `MultiServerMCPClient`; call `getTools()` after construction and `close()` in `finally` after the run.

```sh
npm install deepagents @langchain/mcp-adapters @langchain/openai
export OPENAI_API_KEY='...'
export SUBMILLI_SERVER_URL='http://127.0.0.1:8128'
export SUBMILLI_BLUEPRINT='support-read'
export SUBMILLI_CUSTOMER_ID='cus_northwind'
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
      headers: { "submilli-variables": `customerId=${customerId}` },
      automaticSSEFallback: false,
    }},
  });
  try {
    const tools = await client.getTools("submilli");
    console.log("MCP tools:", tools.map((tool) => tool.name));
    const model = new ChatOpenAI({ model: process.env.OPENAI_MODEL ?? "gpt-4o-mini", temperature: 0 });
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

Run with `npx tsx agent.ts` or the project's existing TypeScript runner. The current declarations confirm `createDeepAgent` is async, `getTools(...servers)` returns LangChain tools, and `close()` cleans up. Keep `automaticSSEFallback: false` when Streamable HTTP is required so transport failure is visible. The model should make the same tool call and report `6150` for the fixture.

## Defaults, backends, and subagents

Deep Agents automatically adds planning, filesystem, summarization, and delegation middleware. The default StateBackend is thread-scoped and appropriate for a web request. Files do not persist across a new `thread_id`; use a StoreBackend and explicitly scoped store only for cross-session memory.

Do not give a server agent a credential-bearing FilesystemBackend, local shell, or unrestricted sandbox and then claim Submilli governs all effects. Deep Agents' `execute` tool is separate from MCP and can bypass blueprint policy. If execution is required, use a restricted sandbox and document that boundary. Human approval for writes requires a checkpointer; it does not replace package `check(...)`.

The built-in general-purpose subagent receives the main agent's configured tools. A custom subagent is a separate tool surface: explicitly give it the MCP tools it needs, or give it none. Custom subagents are stateless, so include the complete instruction in each `task` call. Test child-agent access when delegation is enabled.

Use one request-scoped agent/client per identity. In streaming code, close the session/client after stream completion or cancellation, not after creating the stream. Keep model credentials in the host and service credentials in the package/server path.

## Verification and troubleshooting

Run in order: start the server; initialize and list tools; call an allowed operation; call with a cross-identity value; omit the variable header; then run one model task and inspect tool-call messages and final output. `connection refused` means the server or port is wrong. Missing-variable or denied calls mean the trusted header or blueprint grant is wrong. No tools or incomplete descriptions means the adapter loading path was bypassed. A model tool error means the provider/model lacks tool calling. Unexpected shell/file calls mean middleware or backend exposed capabilities outside Submilli. Leaks or hangs mean cleanup happened before stream completion.

Authoritative references: [Deep Agents Python quickstart](https://docs.langchain.com/oss/python/deepagents/quickstart), [Deep Agents customization](https://docs.langchain.com/oss/python/deepagents/customization), [LangChain Python MCP](https://docs.langchain.com/oss/python/langchain/mcp), [Deep Agents JavaScript customization](https://docs.langchain.com/oss/javascript/deepagents/customization), and the [LangChain JS MCP adapter](https://github.com/langchain-ai/langchainjs/tree/main/libs/langchain-mcp-adapters).
