"""A LangChain deepagents agent that runs its programs on submilli-server, over MCP."""

import asyncio
import os
import pathlib

from deepagents import create_deep_agent
from deepagents.middleware.filesystem import FilesystemPermission
from langchain_mcp_adapters.sessions import create_session
from langchain_mcp_adapters.tools import load_mcp_tools

SUBMILLI_SERVER = os.environ.get("SUBMILLI_SERVER", "http://127.0.0.1:8128")
# The API token this application was given for the server.
SUBMILLI_SERVER_TOKEN = os.environ["SUBMILLI_SERVER_TOKEN"]
BLUEPRINT = "research"


def instructions(user_id: str) -> str:
    # The agent's brief, kept beside the blueprint; `{userId}` names the user.
    brief = (pathlib.Path(__file__).parent.parent / "prompt.txt").read_text()
    return brief.replace("{userId}", user_id)


async def answer(question: str, user_id: str, model="anthropic:claude-haiku-4-5") -> str:
    submilli = {
        "transport": "streamable_http",
        "url": f"{SUBMILLI_SERVER}/mcp/{BLUEPRINT}",
        "headers": {
            "Authorization": f"Bearer {SUBMILLI_SERVER_TOKEN}",
            "submilli-variables": f"userId={user_id}",
        },
    }

    # One session per user: the binding is fixed when the session opens.
    async with create_session(submilli) as session:
        await session.initialize()
        agent = create_deep_agent(
            model=model,
            tools=await load_mcp_tools(session),
            system_prompt=instructions(user_id),
            # deepagents has file tools of its own, which keep files in the
            # conversation. Deny them, so that notes go through Submilli.
            permissions=[FilesystemPermission(operations=["read", "write"], paths=["/**"], mode="deny")],
        )
        result = await agent.ainvoke({"messages": [{"role": "user", "content": question}]})
        return result["messages"][-1].text


if __name__ == "__main__":
    # In a real application the user comes from the signed-in session.
    print(asyncio.run(answer("What is new in the latest stable release of Rust? Save a note with your sources.", "u_ada")))
