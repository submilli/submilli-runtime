"""Runs agent.py against a local submilli-server with a scripted model in place
of a real one, so it needs no API key. Start the server as the README says."""

import asyncio
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


def scripted(code: str) -> Scripted:
    call = {"name": "submilli__typescript__execute", "args": {"code": code}, "id": "call-1"}
    return Scripted(messages=iter([AIMessage(content="", tool_calls=[call]), AIMessage(content="done")]))


async def tool_output(code: str, user_id: str) -> str:
    """What the model was shown after running `code`."""
    seen = []
    model = scripted(code)
    original = model._generate

    def record(messages, *args, **kwargs):
        seen.extend(str(message.content) for message in messages if message.type == "tool")
        return original(messages, *args, **kwargs)

    object.__setattr__(model, "_generate", record)
    assert await answer("total", user_id, model) == "done"
    return "\n".join(seen)


async def main() -> None:
    assert "check.md" in await tool_output(PROGRAM, "u_ada")
    denied = await tool_output(PROGRAM.replace("u_ada", "u_grace"), "u_ada")
    assert "permission denied" in denied, denied
    try:
        await answer("total", "", scripted(PROGRAM))
    except Exception:
        pass  # The server refuses the connection: the blueprint requires a user.
    else:
        raise AssertionError("a session without the user binding was accepted")
    agent.SUBMILLI_SERVER_TOKEN = "a-token-the-server-does-not-know"
    try:
        await answer("total", "u_ada", scripted(PROGRAM))
    except Exception:
        pass  # The server answers 401 before it looks at anything else.
    else:
        raise AssertionError("a token the server does not know was accepted")
    print("deepagents: ok")


asyncio.run(main())
