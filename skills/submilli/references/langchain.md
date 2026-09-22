# LangChain and LangGraph

Read [harnesses](harnesses.md) first. Submilli is the execution boundary; keep
the application's model loop, authentication, and framework. Use the
versions the application already has; for a new project install the current
releases. The examples below were verified on 2026-09-17 with
`langchain 1.4.1`, `langgraph 1.2.11` and `langchain-mcp-adapters 0.3.2`; if
an import or signature differs, the installed version's official API wins
over this text.

The official integration guide is [LangChain MCP](https://docs.langchain.com/oss/python/langchain/mcp).
The official persistence guide is [LangGraph memory](https://docs.langchain.com/oss/python/langgraph/add-memory).

## LangChain agent

Install the framework and adapter into the application's environment:

```sh
python3 -m pip install langchain langchain-mcp-adapters langchain-openai
```

Bind the already-authenticated customer before creating the client. The value
must come from the request's authorized identity, never from a model message.
Create one client and explicit session per identity and keep that session open
for the complete agent invocation:

```python
import asyncio
import os
import re
from typing import Any

from langchain.agents import create_agent
from langchain_mcp_adapters.client import MultiServerMCPClient
from langchain_mcp_adapters.tools import load_mcp_tools
from langchain_openai import ChatOpenAI


def trusted_customer_id() -> str:
    # In production, read this from the authenticated application context.
    value = os.environ["SUBMILLI_CUSTOMER_ID"]
    if not re.fullmatch(r"cus_[A-Za-z0-9_-]+", value) or any(c in value for c in ";\r\n"):
        raise ValueError("invalid customer id")
    return value


async def main() -> None:
    customer_id = trusted_customer_id()
    base_url = os.environ.get("SUBMILLI_SERVER_URL", "http://127.0.0.1:8128").rstrip("/")
    blueprint = os.environ.get("SUBMILLI_BLUEPRINT", "support-read")
    client = MultiServerMCPClient({
        "submilli": {
            "transport": "streamable_http",
            "url": f"{base_url}/mcp/{blueprint}",
            "headers": {"submilli-variables": f"customerId={customer_id}"},
        }
    })
    try:
        async with client.session("submilli") as session:
            tools = await load_mcp_tools(session)
            agent = create_agent(
                model=ChatOpenAI(model=os.getenv("OPENAI_MODEL", "gpt-4o-mini"), temperature=0),
                tools=tools,
                system_prompt="Use the supplied Submilli tools for business data. Never use raw HTTP or shell.",
            )
            result: dict[str, Any] = await agent.ainvoke(
                {"messages": [{"role": "user", "content": "Read my balance."}]},
                config={"configurable": {"thread_id": f"customer:{customer_id}"}},
            )
            print(result["messages"][-1].content)
    finally:
        close = getattr(client, "close", None)
        if close is not None:
            await close()


if __name__ == "__main__":
    asyncio.run(main())
```

`load_mcp_tools(session)` preserves the server's names, schemas, and full
descriptions. `client.get_tools()` is convenient for stateless servers, but it
opens a new session for each tool call; use the explicit session above when a
Submilli binding and filesystem state must cover the whole run. Close the
session after a streaming run completes or is cancelled.

The JavaScript adapter has the same identity rule. Its current API uses
`new MultiServerMCPClient({ mcpServers: ... })`, `await client.getTools(...)`,
and `await client.close()`; see the [official TypeScript MCP integration](https://docs.langchain.com/oss/javascript/langchain/mcp)
for the version-specific example. Keep the trusted header in the server-side
constructor and close the client in `finally`.

## LangGraph checkpoint and resume

LangGraph's `thread_id` identifies a conversation checkpoint. It is not a
Submilli session ID and it is not a customer ID. Live MCP sessions, clients,
model credentials, and trusted identity values should not be serialized into
graph state. On resume, load the authorized customer from application state,
construct a fresh client/session, and rebuild the graph's tool node.

This complete pattern pauses before a tool call, closes the first MCP session,
then resumes the same checkpoint with a newly constructed client bound to the
same server-side identity:

```python
import asyncio
from typing import Any

from langchain_core.messages import AIMessage
from langchain_mcp_adapters.client import MultiServerMCPClient
from langchain_mcp_adapters.tools import load_mcp_tools
from langgraph.checkpoint.memory import InMemorySaver
from langgraph.graph import END, START, MessagesState, StateGraph
from langgraph.prebuilt import ToolNode


def build_graph(tools: list[Any], checkpointer: InMemorySaver):
    # The model node is a scripted stand-in here; replace it with the
    # application's model node. It cannot choose the customer binding.
    def scripted_model(state: MessagesState) -> dict[str, list[Any]]:
        return {"messages": [AIMessage(
            content="",
            tool_calls=[{
                "name": tools[0].name,
                "args": {
                    "code": 'import { readBalance } from "@acme/billing"; '
                            'function main(): number { return readBalance("cus_northwind"); }',
                },
                "id": "balance-call",
                "type": "tool_call",
            }],
        )]}

    def route(state: MessagesState) -> str:
        return "tools" if state["messages"][-1].tool_calls else END

    builder = StateGraph(MessagesState)
    builder.add_node("model", scripted_model)
    builder.add_node("tools", ToolNode(tools))
    builder.add_edge(START, "model")
    builder.add_conditional_edges("model", route, ["tools", END])
    builder.add_edge("tools", END)
    return builder.compile(checkpointer=checkpointer)


async def run_with_resume(customer_id: str) -> dict[str, Any]:
    # customer_id is obtained from the authenticated request, not checkpoint
    # messages. Validate it before putting it in a request header.
    if customer_id != "cus_northwind":
        raise ValueError("demo only: customer must be authorized by the host")
    endpoint = "http://127.0.0.1:8128/mcp/support-read"
    checkpointer = InMemorySaver()
    config = {"configurable": {"thread_id": "trusted-resume-demo"}}

    first_client = MultiServerMCPClient({"submilli": {
        "transport": "streamable_http", "url": endpoint,
        "headers": {"submilli-variables": f"customerId={customer_id}"},
    }})
    async with first_client.session("submilli") as first_session:
        first_tools = await load_mcp_tools(first_session)
        graph = build_graph(first_tools, checkpointer)
        # Checkpoint the scripted model output and pause before the real tool.
        graph.invoke({"messages": [{"role": "user", "content": "Read my balance."}]},
                     config, interrupt_before=["tools"])

    second_client = MultiServerMCPClient({"submilli": {
        "transport": "streamable_http", "url": endpoint,
        "headers": {"submilli-variables": f"customerId={customer_id}"},
    }})
    try:
        async with second_client.session("submilli") as second_session:
            second_tools = await load_mcp_tools(second_session)
            resumed_graph = build_graph(second_tools, checkpointer)
            return resumed_graph.invoke(None, config)
    finally:
        close = getattr(second_client, "close", None)
        if close is not None:
            await close()


if __name__ == "__main__":
    result = asyncio.run(run_with_resume("cus_northwind"))
    print(result["messages"][-1].content)
```

The production graph should use a persistent checkpointer (for example
`langgraph-checkpoint-postgres`) rather than `InMemorySaver`; the identity
lookup remains an application concern in either case. Keep the graph loop
bounded with an explicit stop condition and a suitable `recursion_limit`.

## Deterministic verification

Before a live model run, script the model and keep everything else real: the
`langchain-mcp-adapters` Streamable HTTP session, tool discovery, execution,
and the local Submilli server from [harnesses](harnesses.md). A plain graph
node that returns an `AIMessage` with one `tool_calls` entry for
`submilli__typescript__execute` is enough of a model. Assert:

1. Discovery returns `submilli__typescript__execute` (plus Submilli's docs,
   search, files and last-run tools) and the allowed program returns `6150`.
2. The same session requesting `cus_initech` gets a tool result containing
   `permission denied` and `acme.com/balance.read`. With the adapter default
   `handle_tool_errors=True` this is returned content, not an exception.
   `handle_tool_errors` is an argument of `MultiServerMCPClient(...)` and
   `load_mcp_tools(...)`, not a key of the connection dict.
3. A session without `submilli-variables` fails at `initialize`: entering
   `client.session(...)` raises an exception group wrapping an HTTP `400`, so
   no tools are listed. Test for the raised error, not a tool result.
4. An interrupted thread (`interrupt_before=["tools"]`) resumes with
   `ainvoke(None, config)` on a graph rebuilt from a new session for the same
   trusted identity, after the first session has closed.

The adapter logs `Session termination failed: 202` when a session closes;
the server has already accepted the close and the message is harmless.
A successful scripted run is adapter and policy evidence, not a live model
quality result.
