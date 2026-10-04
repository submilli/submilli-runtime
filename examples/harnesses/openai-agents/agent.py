"""An OpenAI Agents SDK agent that runs its programs on submilli-server, over MCP."""

import asyncio
import os
import pathlib

from agents import Agent, Runner
from agents.mcp import MCPServerStreamableHttp

SUBMILLI_SERVER = os.environ.get("SUBMILLI_SERVER", "http://127.0.0.1:8128")
# The API token this application was given for the server.
SUBMILLI_SERVER_TOKEN = os.environ["SUBMILLI_SERVER_TOKEN"]
BLUEPRINT = "research"


# The agent's brief, kept beside the blueprint.
INSTRUCTIONS = (pathlib.Path(__file__).parent.parent / "prompt.txt").read_text()


async def answer(question: str, user_id: str, model=None) -> str:
    # One connection per user: the binding is fixed when it opens.
    async with MCPServerStreamableHttp(
        name="submilli",
        params={
            "url": f"{SUBMILLI_SERVER}/mcp/{BLUEPRINT}",
            "headers": {
                "Authorization": f"Bearer {SUBMILLI_SERVER_TOKEN}",
                "submilli-variables": f"userId={user_id}",
            },
        },
        # The SDK gives up on a tool call after 5 seconds by default; a
        # program that searches and reads pages takes longer.
        client_session_timeout_seconds=120,
    ) as submilli:
        agent = Agent(
            name="researcher",
            instructions=INSTRUCTIONS,
            model=model,
            mcp_servers=[submilli],
        )
        result = await Runner.run(agent, question, max_turns=20)
        return result.final_output


if __name__ == "__main__":
    # In a real application the user comes from the signed-in session.
    print(asyncio.run(answer("What is new in the latest stable release of Rust? Save a note with your sources.", "u_ada")))
