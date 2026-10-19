"""A LangChain deepagents agent that runs its programs on submilli-server, through langchain-submilli."""

import asyncio
import pathlib
import sys
from typing import TypedDict

from deepagents import create_deep_agent
from langchain_core.runnables import RunnableConfig
from langgraph.checkpoint.memory import InMemorySaver

from langchain_submilli import SubmilliMiddleware

# The agent's brief, kept beside the blueprint.
INSTRUCTIONS = (pathlib.Path(__file__).parent.parent / "prompt.txt").read_text()


class Context(TypedDict):
    """What each run brings: the user it acts for."""

    user_id: str


# Server and token come from SUBMILLI_SERVER and SUBMILLI_SERVER_TOKEN.
submilli = SubmilliMiddleware(
    blueprint="research",
    # deepagents' own file tools keep files in the conversation, not in the
    # user's notebook. Hide them, so that notes go through Submilli.
    hide_filesystem_tools=True,
)

# Built once and shared: the user comes with each run, in its context.
agent = create_deep_agent(
    model="anthropic:claude-sonnet-5",
    system_prompt=INSTRUCTIONS,
    middleware=[submilli],
    checkpointer=InMemorySaver(),
    context_schema=Context,
)


async def answer(question: str, user_id: str, thread_id: str) -> str:
    config: RunnableConfig = {"configurable": {"thread_id": thread_id}}
    result = await agent.ainvoke(
        {"messages": [{"role": "user", "content": question}]}, config, context=Context(user_id=user_id)
    )
    for message in result["messages"]:
        if message.type == "tool" and message.artifact:
            # Where each program's run is in the server's audit trail.
            print(f"[{message.name}] execution {message.artifact.get('execution_id')}", file=sys.stderr)
    return str(result["messages"][-1].text)


async def main() -> None:
    question = "What is new in the latest stable release of Rust? Save a note with your sources."
    config: RunnableConfig = {"configurable": {"thread_id": "example"}}
    try:
        # In a real application the user comes from the signed-in session.
        print(await answer(question, user_id="u_ada", thread_id="example"))
    finally:
        # The session belongs to the thread. This one is done with it.
        await submilli.aclose_thread(agent, config)


if __name__ == "__main__":
    asyncio.run(main())
