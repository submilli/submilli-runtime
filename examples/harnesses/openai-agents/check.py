"""Runs agent.py against a local submilli-server with a scripted model in place
of a real one, so it needs no API key. Start the server as the README says."""

import asyncio
import json
import time
import pathlib

from agents import ModelResponse, Usage, set_tracing_disabled
from agents.models.interface import Model
from openai.types.responses import ResponseFunctionToolCall, ResponseOutputMessage, ResponseOutputText

import agent
from agent import answer

PROGRAM = (pathlib.Path(__file__).parent.parent / "note.ts").read_text()
EXECUTE = "submilli__typescript__execute"


class Scripted(Model):
    """Calls the execute tool once, then answers with what the tool returned."""

    def __init__(self, code: str):
        self.code = code

    async def get_response(self, system_instructions, input, model_settings, tools, *args, **kwargs):
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
                type="function_call", call_id="call-1", name=EXECUTE, arguments=json.dumps({"code": self.code})
            )
        return ModelResponse(output=[item], usage=Usage(), response_id=None)

    def stream_response(self, *args, **kwargs):
        raise NotImplementedError


async def main() -> None:
    set_tracing_disabled(True)
    # Two users write the same note; each sees only their own.
    ada, grace = f"ada_{time.time_ns()}", f"grace_{time.time_ns()}"
    assert "notes before: none" in await answer("total", ada, Scripted(PROGRAM))
    assert "notes before: none" in await answer("total", grace, Scripted(PROGRAM))
    assert "notes before: check.md" in await answer("total", ada, Scripted(PROGRAM))
    try:
        await answer("total", "", Scripted(PROGRAM))
    except Exception:
        pass  # The server refuses the connection: the blueprint requires a user.
    else:
        raise AssertionError("a session without the user binding was accepted")
    agent.SUBMILLI_SERVER_TOKEN = "a-token-the-server-does-not-know"
    try:
        await answer("total", "u_ada", Scripted(PROGRAM))
    except Exception:
        pass  # The server answers 401 before it looks at anything else.
    else:
        raise AssertionError("a token the server does not know was accepted")
    print("openai-agents: ok")


asyncio.run(main())
