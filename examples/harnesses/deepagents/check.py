"""Runs agent.py against a local submilli-server with a scripted model in place
of a real one, so it needs no API key. Start the server as the README says."""

import asyncio
import time
import pathlib

from langchain_core.language_models.fake_chat_models import GenericFakeChatModel
from langchain_core.messages import AIMessage

import agent
from agent import answer

PROGRAM = (pathlib.Path(__file__).parent.parent / "note.ts").read_text()


class Scripted(GenericFakeChatModel):
    def bind_tools(self, tools, **kwargs):
        execute = next(tool for tool in tools if tool.name == "submilli__typescript__execute")
        assert "strict TypeScript subset" in execute.description
        return self


def execute_call(code: str) -> dict:
    return {"name": "submilli__typescript__execute", "args": {"code": code}}


def files_list_call(path: str) -> dict:
    return {"name": "submilli__files__list", "args": {"path": path}}


def scripted(call: dict) -> Scripted:
    tool_call = {**call, "id": "call-1"}
    return Scripted(messages=iter([AIMessage(content="", tool_calls=[tool_call]), AIMessage(content="done")]))


async def tool_output(call: dict, user_id: str) -> str:
    """What the model was shown after making `call`."""
    seen = []
    model = scripted(call)
    original = model._generate

    def record(messages, *args, **kwargs):
        seen.extend(str(message.content) for message in messages if message.type == "tool")
        return original(messages, *args, **kwargs)

    object.__setattr__(model, "_generate", record)
    assert await answer("total", user_id, model) == "done"
    return "\n".join(seen)


async def main() -> None:
    # Two users write the same note; each sees only their own.
    ada, grace = f"ada_{time.time_ns()}", f"grace_{time.time_ns()}"
    assert "notes before: none" in await tool_output(execute_call(PROGRAM), ada)
    assert "notes before: none" in await tool_output(execute_call(PROGRAM), grace)
    assert "notes before: check.md" in await tool_output(execute_call(PROGRAM), ada)
    # The file tools see the same filesystem: the user's notes at /notes.
    listed = await tool_output(files_list_call("/notes"), ada)
    assert "check.md" in listed, listed
    try:
        await answer("total", "", scripted(execute_call(PROGRAM)))
    except Exception:
        pass  # The server refuses the connection: the blueprint requires a user.
    else:
        raise AssertionError("a session without the user binding was accepted")
    agent.SUBMILLI_SERVER_TOKEN = "a-token-the-server-does-not-know"
    try:
        await answer("total", "u_ada", scripted(execute_call(PROGRAM)))
    except Exception:
        pass  # The server answers 401 before it looks at anything else.
    else:
        raise AssertionError("a token the server does not know was accepted")
    print("deepagents: ok")


asyncio.run(main())
