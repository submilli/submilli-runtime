"""A LangChain deepagents agent that runs its programs on submilli-server, over MCP."""

import asyncio
import os

from deepagents import create_deep_agent
from deepagents.middleware.filesystem import FilesystemPermission
from langchain_mcp_adapters.sessions import create_session
from langchain_mcp_adapters.tools import load_mcp_tools

SUBMILLI_SERVER = os.environ.get("SUBMILLI_SERVER", "http://127.0.0.1:8128")
# The application's token for the server: the `user` role, never the admin one.
SUBMILLI_USER_TOKEN = os.environ["SUBMILLI_USER_TOKEN"]
BLUEPRINT = "research"


def instructions(user_id: str) -> str:
    return (
        "You are a research assistant. Search the web and read pages by writing programs for Submilli. "
        "Do the whole job in one program where you can, and return only what you need to answer. "
        f"Keep a note of what you learn, with its sources, under /{user_id}/notes. "
        "Read your earlier notes before you search again."
    )


async def answer(question: str, user_id: str, model="google_genai:gemini-3.8-flash") -> str:
    submilli = {
        "transport": "streamable_http",
        "url": f"{SUBMILLI_SERVER}/mcp/{BLUEPRINT}",
        "headers": {
            "Authorization": f"Bearer {SUBMILLI_USER_TOKEN}",
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
