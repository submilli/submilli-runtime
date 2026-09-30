#!/usr/bin/env python3
"""Exercise real Deep Agents + the MCP adapter against the local Submilli fixture.

Only the model is scripted. The agent graph, default middleware, MCP session,
tool execution and policy responses are real.
"""
import asyncio
import os
from typing import Any

from deepagents import create_deep_agent
from langchain_core.language_models.chat_models import BaseChatModel
from langchain_core.messages import AIMessage, BaseMessage, ToolMessage
from langchain_core.outputs import ChatGeneration, ChatResult
from langchain_mcp_adapters.client import MultiServerMCPClient
from langchain_mcp_adapters.tools import load_mcp_tools

URL = os.environ.get("SUBMILLI_SERVER_URL", "http://127.0.0.1:18128").rstrip("/")
# serve_fixture.py prints this. It is a `user`-role token, so a pass shows this surface needs no more.
TOKEN = os.environ["SUBMILLI_SERVER_TOKEN"]
EXECUTE = "submilli__typescript__execute"


def program(customer: str) -> str:
    return ('import { readBalance } from "@acme/billing"; '
            f'function main(): number {{ return readBalance("{customer}"); }}')


class ScriptedModel(BaseChatModel):
    """Calls the Submilli tool once, then repeats the tool result as text."""

    code: str
    offered: list[str] = []

    @property
    def _llm_type(self) -> str:
        return "scripted"

    def bind_tools(self, tools: Any, **kwargs: Any) -> "ScriptedModel":
        names = [getattr(tool, "name", None) or tool.get("name") or tool["function"]["name"]
                 for tool in tools]
        self.offered.extend(name for name in names if name not in self.offered)
        return self

    def _generate(self, messages: list[BaseMessage], stop: Any = None,
                  run_manager: Any = None, **kwargs: Any) -> ChatResult:
        results = [m for m in messages if isinstance(m, ToolMessage)]
        if results:
            reply = AIMessage(content=f"tool said: {results[-1].content}")
        else:
            reply = AIMessage(content="", tool_calls=[{
                "name": EXECUTE, "args": {"code": self.code}, "id": "balance-call", "type": "tool_call",
            }])
        return ChatResult(generations=[ChatGeneration(message=reply)])


async def run_agent(customer: str | None, requested: str,
                    token: str | None = TOKEN) -> tuple[str, list[str]]:
    # The token admits the application; the binding says which customer.
    headers = {} if token is None else {"Authorization": f"Bearer {token}"}
    if customer is not None:
        headers["submilli-variables"] = f"customerId={customer}"
    client = MultiServerMCPClient({"submilli": {
        "transport": "streamable_http", "url": f"{URL}/mcp/support-read", "headers": headers,
    }})
    # The session must outlive the whole agent run; tools call through it.
    async with client.session("submilli") as session:
        tools = await load_mcp_tools(session)
        assert EXECUTE in [tool.name for tool in tools], [tool.name for tool in tools]
        model = ScriptedModel(code=program(requested), offered=[])
        agent = create_deep_agent(model=model, tools=tools, system_prompt="Use Submilli tools.")
        result = await agent.ainvoke({"messages": [{"role": "user", "content": "Read my balance"}]})
        return str(result["messages"][-1].content), model.offered


def flatten(error: BaseException) -> list[str]:
    nested = getattr(error, "exceptions", None)
    if nested is None:
        return [str(error)]
    return [text for child in nested for text in flatten(child)]


async def main() -> None:
    allowed, offered = await run_agent("cus_northwind", "cus_northwind")
    assert "6150" in allowed, allowed

    # Record what the default middleware hands the model besides Submilli.
    # `execute` would run outside blueprint policy; the default StateBackend
    # must not provide it.
    print("tools offered to the model:", sorted(offered))
    assert EXECUTE in offered, offered
    assert "execute" not in offered, offered

    denied, _ = await run_agent("cus_northwind", "cus_initech")
    assert "permission denied" in denied.lower() and "balance.read" in denied, denied

    # The token is still sent here: this must be the 400 for the missing
    # binding, not an authentication error.
    try:
        await run_agent(None, "cus_northwind")
    except BaseException as error:
        assert "400" in "".join(flatten(error)), error
    else:
        raise AssertionError("session without customerId was accepted")
    # Without a token the server answers 401 before it reads the binding. The
    # adapter has no OAuth provider configured, so it raises instead of
    # starting a sign-in.
    try:
        await run_agent("cus_northwind", "cus_northwind", token=None)
    except BaseException as error:
        assert "401" in "".join(flatten(error)), error
    else:
        raise AssertionError("session without a token was accepted")
    print("PASS: deep agent allowed=6150, cross-customer denied, missing binding rejected, missing token rejected")


if __name__ == "__main__":
    asyncio.run(main())
