"""Runs agent.py's toolkit against a local submilli-server with a scripted model
in place of a real one, so it needs no API key. Start the server as the README says."""

import asyncio
import json
import pathlib
import time

import submilli
from agents import Agent, ModelResponse, Runner, Usage, UserError, set_tracing_disabled
from agents.models.interface import Model
from openai.types.responses import ResponseFunctionToolCall, ResponseOutputMessage, ResponseOutputText
from openai_agents_submilli import SubmilliSessionMismatchError, SubmilliTools

import agent

PROGRAM = (pathlib.Path(__file__).parent.parent / "note.ts").read_text()
EXECUTE = "submilli__typescript__execute"
SUBMILLI_TOOLS = {
    EXECUTE,
    "submilli__typescript__last_run",
    "submilli__typescript__packages__search",
    "submilli__typescript__packages__docs",
    "submilli__typescript__builtins__list",
    "submilli__typescript__builtins__docs",
}


class Scripted(Model):
    """Calls the execute tool once, then answers with what the tool returned."""

    async def get_response(self, system_instructions, input, model_settings, tools, *args, **kwargs):
        assert {tool.name for tool in tools} == SUBMILLI_TOOLS
        execute = next(tool for tool in tools if tool.name == EXECUTE)
        assert "strict TypeScript subset" in execute.description
        outputs = [item for item in input if isinstance(item, dict) and item.get("type") == "function_call_output"]
        if outputs:
            text = ResponseOutputText(type="output_text", text=str(outputs[-1]["output"]), annotations=[])
            item = ResponseOutputMessage(
                id="message-1", type="message", role="assistant", status="completed", content=[text]
            )
        else:
            item = ResponseFunctionToolCall(
                type="function_call",
                call_id=f"call-{time.time_ns()}",
                name=EXECUTE,
                arguments=json.dumps({"code": PROGRAM}),
            )
        return ModelResponse(output=[item], usage=Usage(), response_id=None)

    def stream_response(self, *args, **kwargs):
        raise NotImplementedError


async def note(context: dict, assistant: Agent | None = None) -> str:
    """What the model was shown after running note.ts with `context`."""
    result = await Runner.run(assistant or agent.build(Scripted()), "note", context=context)
    return str(result.final_output)


async def session_id(conversation_id: str) -> str:
    stored = await agent.submilli.store.get(conversation_id)
    assert stored is not None
    return stored["id"]


async def main() -> None:
    set_tracing_disabled(True)
    run = time.time_ns()
    # Two users write the same note; each sees only their own.
    ada, grace = f"ada_{run}", f"grace_{run}"
    assert "notes before: none" in await note({"user_id": ada, "conversation_id": f"ada-{run}"})
    assert "notes before: none" in await note({"user_id": grace, "conversation_id": f"grace-{run}"})
    # A conversation keeps one session across turns, and refuses another user.
    alan = {"user_id": f"alan_{run}", "conversation_id": f"alan-{run}"}
    assert "notes before: none" in await note(alan)
    first = await session_id(f"alan-{run}")
    assert "notes before: check.md" in await note(alan)
    assert await session_id(f"alan-{run}") == first, "the conversation's second turn opened a new session"
    try:
        await note({"user_id": ada, "conversation_id": f"alan-{run}"})
    except SubmilliSessionMismatchError:
        pass
    else:
        raise AssertionError("another user's run was accepted in the conversation")
    for conversation in (f"ada-{run}", f"grace-{run}", f"alan-{run}"):
        assert await agent.submilli.aclose_conversation(conversation), "the conversation's session was not ended"
    try:
        await note({"conversation_id": f"nobody-{run}"})
    except UserError as error:
        # The server refuses the session: the blueprint requires a user.
        assert isinstance(error.__cause__, submilli.BadRequestError), error.__cause__
    else:
        raise AssertionError("a session without the user binding was accepted")
    stranger = SubmilliTools(blueprint="research", token="a-token-the-server-does-not-know")
    try:
        await note({"user_id": ada, "conversation_id": f"stranger-{run}"}, Agent(name="r", model=Scripted(), tools=stranger.tools))
    except submilli.AuthenticationError:
        pass  # The server answers 401 before it looks at anything else.
    else:
        raise AssertionError("a token the server does not know was accepted")
    await agent.submilli.aclose()
    await stranger.aclose()
    print("openai-agents: ok")


asyncio.run(main())
