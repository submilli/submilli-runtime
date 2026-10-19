"""Runs agent.py's middleware against a local submilli-server with a scripted model
in place of a real one, so it needs no API key. Start the server as the README says."""

import asyncio
import pathlib
import time

import submilli
from deepagents import create_deep_agent
from langchain_core.language_models.fake_chat_models import GenericFakeChatModel
from langchain_core.messages import AIMessage
from langchain_submilli import SubmilliMiddleware, SubmilliSessionMismatchError
from langgraph.checkpoint.memory import InMemorySaver

import agent

PROGRAM = (pathlib.Path(__file__).parent.parent / "note.ts").read_text()
SUBMILLI_TOOLS = {
    "submilli__typescript__execute",
    "submilli__typescript__last_run",
    "submilli__typescript__packages__search",
    "submilli__typescript__packages__docs",
    "submilli__typescript__builtins__list",
    "submilli__typescript__builtins__docs",
}


class Scripted(GenericFakeChatModel):
    def bind_tools(self, tools, **kwargs):
        # deepagents' file tools are hidden; its `task` tool for subagents stays.
        assert {tool.name for tool in tools} == SUBMILLI_TOOLS | {"task"}
        execute = next(tool for tool in tools if tool.name == "submilli__typescript__execute")
        assert "strict TypeScript subset" in execute.description
        return self


def scripted(turns: int = 1) -> Scripted:
    messages = []
    for _ in range(turns):
        call = {"name": "submilli__typescript__execute", "args": {"code": PROGRAM}, "id": f"call-{time.time_ns()}"}
        messages += [AIMessage(content="", tool_calls=[call]), AIMessage(content="done")]
    return Scripted(messages=iter(messages))


class Thread:
    """One conversation thread on its own graph, built with agent.py's middleware."""

    def __init__(self, turns: int = 1, middleware: SubmilliMiddleware = agent.submilli) -> None:
        self.middleware = middleware
        self.graph = create_deep_agent(
            model=scripted(turns),
            middleware=[middleware],
            system_prompt=agent.INSTRUCTIONS,
            checkpointer=InMemorySaver(),
            context_schema=agent.Context,
        )
        self.config = {"configurable": {"thread_id": f"check-{time.time_ns()}"}}

    async def turn(self, context: dict) -> str:
        """What the model was shown after running note.ts with `context`."""
        result = await self.graph.ainvoke({"messages": [{"role": "user", "content": "note"}]}, self.config, context=context)
        return [message.text for message in result["messages"] if message.type == "tool"][-1]

    async def session_id(self) -> str:
        return (await self.graph.aget_state(self.config)).values["submilli_session"]["id"]

    async def close(self) -> bool:
        return await self.middleware.aclose_thread(self.graph, self.config)


async def note(context: dict, middleware: SubmilliMiddleware = agent.submilli) -> str:
    thread = Thread(middleware=middleware)
    try:
        return await thread.turn(context)
    finally:
        await thread.close()


async def main() -> None:
    # Two users write the same note; each sees only their own.
    ada, grace = f"ada_{time.time_ns()}", f"grace_{time.time_ns()}"
    assert "notes before: none" in await note({"user_id": ada})
    assert "notes before: none" in await note({"user_id": grace})
    # A thread keeps one session across turns, and refuses another user.
    alan = f"alan_{time.time_ns()}"
    thread = Thread(turns=2)
    assert "notes before: none" in await thread.turn({"user_id": alan})
    first = await thread.session_id()
    assert "notes before: check.md" in await thread.turn({"user_id": alan})
    assert await thread.session_id() == first, "the thread's second turn opened a new session"
    try:
        await thread.turn({"user_id": ada})
    except SubmilliSessionMismatchError:
        pass
    else:
        raise AssertionError("another user's run was accepted on the thread")
    assert await thread.close(), "the thread's session was not ended"
    try:
        await note({})
    except submilli.BadRequestError:
        pass  # The server refuses the session: the blueprint requires a user.
    else:
        raise AssertionError("a session without the user binding was accepted")
    try:
        await note({"user_id": ada}, SubmilliMiddleware(blueprint="research", token="a-token-the-server-does-not-know"))
    except submilli.AuthenticationError:
        pass  # The server answers 401 before it looks at anything else.
    else:
        raise AssertionError("a token the server does not know was accepted")
    print("deepagents: ok")


asyncio.run(main())
