#!/usr/bin/env python3
"""Exercise the real LangChain MCP adapter against the local Submilli fixture."""
import asyncio
import os
from typing import Any

from langchain_core.messages import AIMessage, ToolMessage
from langchain_mcp_adapters.client import MultiServerMCPClient
from langchain_mcp_adapters.tools import load_mcp_tools
from langgraph.checkpoint.memory import InMemorySaver
from langgraph.graph import END, START, MessagesState, StateGraph
from langgraph.prebuilt import ToolNode

URL = os.environ.get("SUBMILLI_SERVER_URL", "http://127.0.0.1:18128").rstrip("/")
# The fixture's user token; serve_fixture.py prints it.
TOKEN = os.environ["SUBMILLI_USER_TOKEN"]
CODE = 'import { readBalance } from "@acme/billing"; function main(): number { return readBalance("cus_northwind"); }'


def execution_tool(tools: list[Any]) -> Any:
    matches = [tool for tool in tools if "code" in getattr(tool, "args", {})]
    if len(matches) != 1:
        raise AssertionError(f"expected one execution tool, got {[tool.name for tool in tools]}")
    return matches[0]


def flatten(error: BaseException) -> list[str]:
    nested = getattr(error, "exceptions", None)
    if nested is None:
        return [str(error)]
    return [text for child in nested for text in flatten(child)]


async def load(customer: str | None, token: str | None = TOKEN) -> tuple[MultiServerMCPClient, Any, list[Any]]:
    # The token admits the application; the binding says which customer.
    headers = {} if token is None else {"Authorization": f"Bearer {token}"}
    if customer is not None:
        headers["submilli-variables"] = f"customerId={customer}"
    client = MultiServerMCPClient({"submilli": {
        "transport": "streamable_http", "url": f"{URL}/mcp/support-read", "headers": headers,
    }})
    session = client.session("submilli")
    entered = await session.__aenter__()
    try:
        tools = await load_mcp_tools(entered)
    except BaseException:
        await session.__aexit__(None, None, None)
        raise
    return client, session, tools


async def call(customer: str | None, code: str, token: str | None = TOKEN) -> str:
    client, session, tools = await load(customer, token)
    try:
        result = await execution_tool(tools).ainvoke({"code": code})
        if isinstance(result, ToolMessage):
            return str(result.content)
        return str(result)
    finally:
        await session.__aexit__(None, None, None)
        close = getattr(client, "close", None)
        if close is not None:
            await close()


def graph_for(tools: list[Any], saver: InMemorySaver):
    tool = execution_tool(tools)

    def scripted_model(state: MessagesState) -> dict[str, list[Any]]:
        return {"messages": [AIMessage(content="", tool_calls=[{
            "name": tool.name, "args": {"code": CODE}, "id": "balance-call", "type": "tool_call",
        }])]}

    def route(state: MessagesState) -> str:
        return "tools" if state["messages"][-1].tool_calls else END

    builder = StateGraph(MessagesState)
    builder.add_node("model", scripted_model)
    builder.add_node("tools", ToolNode([tool]))
    builder.add_edge(START, "model")
    builder.add_conditional_edges("model", route, ["tools", END])
    builder.add_edge("tools", END)
    return builder.compile(checkpointer=saver)


async def resume_test() -> None:
    saver = InMemorySaver()
    config = {"configurable": {"thread_id": "langchain-submilli-resume"}}
    first_client, first_session, first_tools = await load("cus_northwind")
    try:
        graph = graph_for(first_tools, saver)
        await graph.ainvoke({"messages": [{"role": "user", "content": "Read my balance"}]}, config,
                            interrupt_before=["tools"])
    finally:
        await first_session.__aexit__(None, None, None)
        close = getattr(first_client, "close", None)
        if close is not None:
            await close()

    second_client, second_session, second_tools = await load("cus_northwind")
    try:
        result = await graph_for(second_tools, saver).ainvoke(None, config)
        content = str(result["messages"][-1].content)
        assert "6150" in content, content
    finally:
        await second_session.__aexit__(None, None, None)
        close = getattr(second_client, "close", None)
        if close is not None:
            await close()


async def main() -> None:
    tools_result = await call("cus_northwind", CODE)
    assert "6150" in tools_result, tools_result
    denied = await call("cus_northwind", CODE.replace("cus_northwind", "cus_initech"))
    assert "permission denied" in denied.lower() and "balance.read" in denied, denied
    # The server rejects a session without the required binding at initialize,
    # so the failure surfaces from the transport before any tool is listed.
    # The token is still sent: this must be the 400, not an authentication error.
    try:
        await call(None, CODE)
    except BaseException as error:
        assert "400" in "".join(flatten(error)), error
    else:
        raise AssertionError("session without customerId was accepted")
    # Without a token the server answers 401 before it reads the binding. The
    # adapter has no OAuth provider configured, so it raises instead of
    # starting a sign-in.
    try:
        await call("cus_northwind", CODE, token=None)
    except BaseException as error:
        assert "401" in "".join(flatten(error)), error
    else:
        raise AssertionError("session without a token was accepted")
    invalid = await call("cus_northwind", "bad program")
    assert "6150" not in invalid and "error" in invalid.lower(), invalid
    await resume_test()
    print("PASS: discovery, allowed=6150, denied, missing binding, missing token, error cleanup, checkpoint resume")


if __name__ == "__main__":
    asyncio.run(main())
