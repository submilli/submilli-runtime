# Deep Agents harness smoke test

Runs real `create_deep_agent` with its default middleware and the real
`langchain-mcp-adapters` session against the `support-read` fixture; only the
model is scripted. Run `skills/evals/harnesses/serve_fixture.py` first, then:

```sh
python3 -m venv .venv-deepagents && . .venv-deepagents/bin/activate
python -m pip install -r skills/evals/harnesses/deepagents/requirements.txt
export SUBMILLI_SERVER_URL=http://127.0.0.1:18128 SUBMILLI_USER_TOKEN=...   # the line the fixture prints
python skills/evals/harnesses/deepagents/validate.py
```

Use a separate environment from the LangChain check. It prints the tool
surface the default middleware offers the model, asserts no `execute` tool is
present, and asserts the allowed read, cross-customer denial,
missing-binding rejection (HTTP 400, token present) and missing-token
rejection (HTTP 401).
