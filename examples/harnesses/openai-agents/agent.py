"""An OpenAI Agents SDK agent that runs its programs on submilli-server, through openai-agents-submilli."""

import asyncio
import pathlib
import sys

from agents import Agent, Runner
from agents.items import ToolCallOutputItem
from openai_agents_submilli import SubmilliTools

# The agent's brief, kept beside the blueprint.
INSTRUCTIONS = (pathlib.Path(__file__).parent.parent / "prompt.txt").read_text()

# Server and token come from SUBMILLI_SERVER and SUBMILLI_SERVER_TOKEN.
submilli = SubmilliTools(blueprint="research")


def build(model=None) -> Agent:
    # Built once and shared: the user and the conversation come with each
    # run, in its context. Only Submilli's tools are given to the model.
    return Agent(name="researcher", instructions=INSTRUCTIONS, model=model, tools=submilli.tools)


agent = build()


async def answer(question: str, user_id: str, conversation_id: str, assistant: Agent = agent) -> str:
    context = {"user_id": user_id, "conversation_id": conversation_id}
    result = await Runner.run(assistant, question, context=context, max_turns=20)
    for item in result.new_items:
        if isinstance(item, ToolCallOutputItem) and item.custom_data:
            # Where each program's run is in the server's audit trail.
            print(f"[submilli] execution {item.custom_data.get('execution_id')}", file=sys.stderr)
    return str(result.final_output)


async def main() -> None:
    question = "What is new in the latest stable release of Rust? Save a note with your sources."
    try:
        # In a real application the user comes from the signed-in session.
        print(await answer(question, user_id="u_ada", conversation_id="example"))
    finally:
        # The session belongs to the conversation. This one is done with it.
        await submilli.aclose_conversation("example")
        await submilli.aclose()


if __name__ == "__main__":
    asyncio.run(main())
