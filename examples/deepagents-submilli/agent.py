#!/usr/bin/env python3
"""A deepagents agent that writes and runs code in the Submilli sandbox.

The agent reaches a running ``submilli-server`` over MCP streamable HTTP. The
blueprint you pass on the CLI is the last path segment of the MCP URL
(``/mcp/<blueprint>``) and fixes the sandbox the agent's code executes in — its
filesystem mode, and (later) its capabilities. The model is Gemini, via Google
AI Studio.

Usage::

    export GOOGLE_API_KEY=...            # Google AI Studio key
    export SUBMILLI_SERVER_TOKEN=...     # the token the server was started with
    python agent.py "What is the 30th Fibonacci number?" --blueprint demo

Prerequisites: a running server with the blueprint registered — see README.md.
"""

from __future__ import annotations

import argparse
import asyncio
import os
import sys
import textwrap

from pathlib import Path

from deepagents import create_deep_agent
from langchain_google_genai import ChatGoogleGenerativeAI
from langchain_mcp_adapters.client import MultiServerMCPClient
from langchain_mcp_adapters.tools import load_mcp_tools


def _load_local_env() -> None:
    """Load a gitignored `.env` sitting next to this script (KEY=VALUE lines),
    without overriding anything already set in the real environment."""
    env_file = Path(__file__).with_name(".env")
    if not env_file.exists():
        return
    for line in env_file.read_text().splitlines():
        line = line.strip()
        if not line or line.startswith("#") or "=" not in line:
            continue
        key, _, value = line.partition("=")
        os.environ.setdefault(key.strip(), value.strip().strip("'\""))

SYSTEM_PROMPT = """\
You are a capable assistant with access to **Submilli**, a secure sandbox that
compiles and runs small TypeScript programs and returns their result.

The tool `submilli__typescript__execute` takes one argument, `code`: a strict
TypeScript-subset program. Reach for it whenever a step needs real computation,
data wrangling, file IO, or an HTTP call — don't do arithmetic or string work in
your head, write and run a program.

## Program shape

- Exactly one `function main(): T` is the entry point; its return value is the
  result. **Return types are mandatory on every function, including `main`.**
- No top-level statements — put logic inside `main()` or helper functions.
- `console.log(...)` is a separate debug stream, not the return value.

## Language deltas (it is NOT Node.js / browser JS)

- No NPM, no Node APIs, no browser globals.
- No `undefined`, no non-null `!`. Model absence with `T | null` and narrow
  against `null` (no `in`, no `.hasOwnProperty()`).
- No `async`/`await`/`Promise` — calls are synchronous.
- No `Date` — use the `Temporal` global (ISO 8601 + IANA timezones).
- `unknown` requires narrowing; there is no unsafe `any`. `==` aliases `===`.

## Standard library

Globals (no import): `Math`, `JSON` (`stringify` / `parse`), `Temporal`.

Importable modules (this is the full set — there is no package browser, so use
only what's listed here):

- `submilli:fs` — sandbox filesystem. `readText(path)`, `writeText(path, text)`,
  `appendText(path, text)`, `readBytes(path)`, `write(path, bytes)`,
  `exists(path)`, `list(path)`, `mkdir(path)`, `remove(path)`,
  `move(from, to)`, `copy(from, to)`, `size(path)`, `stat(path)`. Under a
  `per_session` blueprint these files persist across your tool calls this run.
- `submilli:http` — `get(url)`, `post(url, body)`, also `put` / `patch` /
  `delete` / `head`; each returns a `Response` (inspect its fields via the
  types). Don't add `Authorization` headers — the operator's policy injects
  credentials. HTTP may be restricted by the bound blueprint.
- `submilli:crypto` — `sha256(data)`, `sha512(data)`, `hmacSha256(key, data)`,
  `randomBytes(n)`, `timingSafeEqual(a, b)`.
- `submilli:url` — `parse(str)`, `build(...)`, `encodeQuery` / `decodeQuery`,
  `encodeComponent` / `decodeComponent`.
- `submilli:uuid` — `v4()`, `v7()`, `validate(s)`.

Import the named functions you need, e.g. `import { writeText, readText } from "submilli:fs";`.

Write the smallest program that answers the step you're on, run it, read the
result, and continue. If a program fails to compile, the error tells you what to
fix — adjust and rerun. When you have the final answer, explain it plainly.
"""


def build_model(model_name: str) -> ChatGoogleGenerativeAI:
    api_key = os.environ.get("GOOGLE_API_KEY") or os.environ.get("GEMINI_API_KEY")
    if not api_key:
        sys.exit(
            "error: set GOOGLE_API_KEY (or GEMINI_API_KEY) to your Google AI Studio key"
        )
    return ChatGoogleGenerativeAI(model=model_name, temperature=0, google_api_key=api_key)


def server_token() -> str:
    """The API token this agent was given for the server. Read from the
    environment, never from a flag, so it stays out of the process list."""
    token = os.environ.get("SUBMILLI_SERVER_TOKEN")
    if not token:
        sys.exit("error: set SUBMILLI_SERVER_TOKEN to the token the server was started with")
    return token


async def run(prompt: str, blueprint: str, server_url: str, model_name: str) -> None:
    mcp_url = f"{server_url.rstrip('/')}/mcp/{blueprint}"
    client = MultiServerMCPClient(
        {
            "submilli": {
                "transport": "streamable_http",
                "url": mcp_url,
                "headers": {"Authorization": f"Bearer {server_token()}"},
            }
        }
    )

    print(f"· model={model_name}  blueprint={blueprint}  mcp={mcp_url}\n")

    # One persistent MCP session for the whole run: a `per_session` blueprint's
    # sandbox filesystem is keyed by the MCP-Session-Id, so keeping a single
    # session lets files survive across the agent's tool calls.
    async with client.session("submilli") as session:
        tools = await load_mcp_tools(session)
        agent = create_deep_agent(
            tools=tools,
            system_prompt=SYSTEM_PROMPT,
            model=build_model(model_name),
        )
        result = await agent.ainvoke(
            {"messages": [{"role": "user", "content": prompt}]}
        )

    print_transcript(result["messages"])


def print_transcript(messages) -> None:
    """Show each program the agent ran, its result, and the final answer."""
    for msg in messages:
        for call in getattr(msg, "tool_calls", None) or []:
            if call["name"].endswith("execute") and "code" in call.get("args", {}):
                print("┌─ submilli__typescript__execute ──────────────────")
                print(textwrap.indent(call["args"]["code"].rstrip(), "│ "))
                print("└─────────────────────────────────────────────────")
            else:
                print(f"🔧 {call['name']}({call.get('args', {})})")
        if msg.__class__.__name__ == "ToolMessage":
            print("   ↳ " + _short(_text(msg.content)) + "\n")

    print("=== answer " + "=" * 38)
    print(_text(messages[-1].content))


def _text(content) -> str:
    """LangChain message content is a string or a list of content parts."""
    if isinstance(content, str):
        return content
    if isinstance(content, list):
        parts = [p.get("text", "") if isinstance(p, dict) else str(p) for p in content]
        return "".join(parts)
    return str(content)


def _short(text: str, limit: int = 240) -> str:
    text = " ".join(text.split())
    return text if len(text) <= limit else text[:limit] + " …"


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("prompt", help="the task for the agent")
    parser.add_argument(
        "--blueprint",
        default="demo",
        help="Submilli blueprint to bind (the /mcp/<blueprint> path segment)",
    )
    parser.add_argument(
        "--server-url",
        default=os.environ.get("SUBMILLI_SERVER_URL", "http://127.0.0.1:8128"),
        help="base URL of the running submilli-server",
    )
    parser.add_argument(
        "--model",
        default=os.environ.get("SUBMILLI_EXAMPLE_MODEL", "gemini-2.5-flash"),
        help="Gemini model id (Google AI Studio)",
    )
    args = parser.parse_args()
    _load_local_env()
    asyncio.run(run(args.prompt, args.blueprint, args.server_url, args.model))


if __name__ == "__main__":
    main()
