# LangChain / LangGraph harness smoke test

Real `langchain-mcp-adapters` session and a LangGraph `ToolNode`; the model is
a scripted node. Run `skills/evals/harnesses/serve_fixture.py` first, then:

```sh
python3 -m venv .venv-langchain && . .venv-langchain/bin/activate
python -m pip install -r skills/evals/harnesses/langchain/requirements.txt
export SUBMILLI_SERVER_URL=http://127.0.0.1:18128 SUBMILLI_SERVER_TOKEN=...   # the line the fixture prints
python skills/evals/harnesses/langchain/validate.py
```

Asserts discovery, the allowed `6150` read, cross-customer denial,
missing-binding rejection at session initialize (HTTP 400, token present),
missing-token rejection (HTTP 401, no OAuth flow), an invalid program returning
an error result, and checkpoint resume on a new session after the first closes.
