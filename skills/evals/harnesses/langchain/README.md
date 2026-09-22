# LangChain / LangGraph harness smoke test

Real `langchain-mcp-adapters` session and a LangGraph `ToolNode`; the model is
a scripted node. Run `skills/evals/harnesses/serve_fixture.py` first, then:

```sh
python3 -m venv .venv-langchain && . .venv-langchain/bin/activate
python -m pip install -r skills/evals/harnesses/langchain/requirements.txt
SUBMILLI_SERVER_URL=http://127.0.0.1:18128 python skills/evals/harnesses/langchain/validate.py
```

Asserts discovery, the allowed `6150` read, cross-customer denial,
missing-binding rejection at session initialize, an invalid program returning
an error result, and checkpoint resume on a new session after the first closes.
