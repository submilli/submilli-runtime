# deepagents + Submilli MCP

A tiny [deepagents](https://github.com/langchain-ai/deepagents) agent that uses
**Submilli** as a code-execution sandbox over MCP. The agent runs on **Gemini**
(Google AI Studio) and is given one tool — `submilli__typescript__execute` — so it
can write and run small TypeScript programs to actually *do* the work instead of
guessing.

You pass it a prompt and a blueprint; the blueprint is the `/mcp/<blueprint>`
path segment and fixes the sandbox the agent's code runs in.

```
examples/deepagents-submilli/
├── agent.py            # the harness
├── requirements.txt
├── blueprints/demo.yaml
└── README.md
```

## How it talks to Submilli

Submilli's server speaks MCP **streamable HTTP** over a normal URL, so
`langchain-mcp-adapters` connects directly — no broker, no stdio:

```python
client = MultiServerMCPClient({
    "submilli": {
        "transport": "streamable_http",
        "url": ".../mcp/demo",
        "headers": {"Authorization": f"Bearer {token}"},
    },
})
async with client.session("submilli") as session:   # one session for the run
    tools = await load_mcp_tools(session)
```

The persistent `session(...)` matters: a `per_session` blueprint keys its
sandbox filesystem by the `MCP-Session-Id`, so holding one session lets files
the agent writes survive across its tool calls.

## Run it

### 1. Start a Submilli server

The server takes a bearer token on every request, and reads its own from
`SUBMILLI_SERVER_TOKEN`. From the repo root:

```bash
export SUBMILLI_SERVER_TOKEN=$(openssl rand -hex 32)
cargo run -p submilli-server
# listens on http://127.0.0.1:8128
```

Run the remaining steps in a second terminal with the same variable exported —
copy the value across, since a fresh `openssl rand` would not match the one
the server read.

That token is an admin token, which is fine while the agent and the server are
both yours on one machine. Before the agent runs anywhere you trust less, add
a `user`-role token with a `token_file` under `api_tokens` in a server config
and export that one for the agent instead: it can run code and cannot change a
blueprint. `agent.py` stays the same. The book's "Submilli server" chapter,
under "Who can reach it", has the details.

### 2. Register the `demo` blueprint

The server's blueprint store is managed over its REST API. Register the
bundled `per_session` blueprint:

```bash
curl -X PUT http://127.0.0.1:8128/v1/blueprints/demo \
  -H "Authorization: Bearer $SUBMILLI_SERVER_TOKEN" \
  -H 'content-type: application/json' \
  -d '{"yaml": "name: demo\nvfs: per_session\ndefault: allow\n"}'
```

(`blueprints/demo.yaml` is the same content, for reference.)

### 3. Install deps and set your key

```bash
cd examples/deepagents-submilli
python -m venv .venv && source .venv/bin/activate
pip install -r requirements.txt
export GOOGLE_API_KEY=...     # from https://aistudio.google.com/apikey
```

`SUBMILLI_SERVER_TOKEN` must be exported here too; the agent exits with a
message if it is not.

### 4. Ask it something

```bash
python agent.py "What is the 30th Fibonacci number? Use code." --blueprint demo
```

```bash
python agent.py \
  "Write 'hello' to /note.txt, then read it back and uppercase it." \
  --blueprint demo
```

The agent prints each program it runs, the result, and the final answer.

## Options

| flag / env | default | meaning |
|---|---|---|
| `--blueprint` | `demo` | the `/mcp/<blueprint>` sandbox to bind |
| `--server-url` / `SUBMILLI_SERVER_URL` | `http://127.0.0.1:8128` | running server |
| `--model` / `SUBMILLI_EXAMPLE_MODEL` | `gemini-2.5-flash` | Gemini model id |
| `GOOGLE_API_KEY` (or `GEMINI_API_KEY`) | — | Google AI Studio key |
| `SUBMILLI_SERVER_TOKEN` | — | the API token the agent sends as `Authorization: Bearer …`; there is no flag for it |

## Notes

- This is an **example**, not a test — it needs a live server and a real API key.
- Point `--blueprint` at any registered blueprint to change the sandbox. A
  `per_session` blueprint keeps its filesystem for the whole run; an `ephemeral`
  one gives each `execute` call a fresh, throwaway filesystem.
