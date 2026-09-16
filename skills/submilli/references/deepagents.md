# Deep Agents

Read [harnesses](harnesses.md) first. Follow the installed Deep Agents version
and the user's Python or TypeScript choice. The repository quickstart uses
Python with `deepagents`, `langchain-mcp-adapters`, and a model-provider package.

For Python, use `MultiServerMCPClient` from `langchain_mcp_adapters.client`
with a named server config containing `transport: "streamable_http"`, the
blueprint URL, and trusted variable headers. Enter
`async with client.session("submilli") as session`, then call
`load_mcp_tools(session)` from `langchain_mcp_adapters.tools`. Pass those tools
to `create_deep_agent` from `deepagents`, along with the application's model.
Await `agent.ainvoke({"messages": [{"role": "user", "content": task}]})`
inside the session context. Preserve tool descriptions and inspect tool-call
messages when verifying which code executed.

Deep Agents may add filesystem, execution, and delegation tools independently
of the supplied MCP tools. Inspect the selected backend and middleware; an
unrestricted execution backend is not protected by a Submilli blueprint.
Do not attach a credential-bearing host filesystem or shell and then claim
Submilli governs everything. Test child agents' tool access too if used.

For TypeScript, adapt the same request-scoped MCP contract through the
installed LangChain MCP adapter; verify API exports rather than translating
Python method names literally. Do not require a framework migration.

Authoritative references:
[Deep Agents customization](https://docs.langchain.com/oss/python/deepagents/customization),
[LangChain MCP](https://docs.langchain.com/oss/python/langchain/mcp),
[TypeScript Deep Agents](https://docs.langchain.com/oss/javascript/deepagents/overview).
