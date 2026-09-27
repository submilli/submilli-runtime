"""An OpenAI Agents SDK agent that runs its programs on submilli-server, over MCP."""

import asyncio
import os

from agents import Agent, Runner
from agents.mcp import MCPServerStreamableHttp

SUBMILLI_SERVER = os.environ.get("SUBMILLI_SERVER", "http://127.0.0.1:8128")
BLUEPRINT = "research"


def instructions(user_id: str) -> str:
    return (
        "You are a research assistant. Search the web and read pages by writing programs for Submilli. "
        "Do the whole job in one program where you can, and return only what you need to answer. "
        f"Keep a note of what you learn, with its sources, under /{user_id}/notes. "
        "Read your earlier notes before you search again."
    )


async def answer(question: str, user_id: str, model=None) -> str:
    # One connection per user: the binding is fixed when it opens.
    async with MCPServerStreamableHttp(
        name="submilli",
        params={
            "url": f"{SUBMILLI_SERVER}/mcp/{BLUEPRINT}",
            "headers": {"submilli-variables": f"userId={user_id}"},
        },
    ) as submilli:
        agent = Agent(
            name="researcher",
            instructions=instructions(user_id),
            model=model,
            mcp_servers=[submilli],
        )
        result = await Runner.run(agent, question, max_turns=20)
        return result.final_output


if __name__ == "__main__":
    # In a real application the user comes from the signed-in session.
    print(asyncio.run(answer("What is new in the latest stable release of Rust? Save a note with your sources.", "u_ada")))
