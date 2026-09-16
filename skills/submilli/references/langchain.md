# LangChain and LangGraph

Read [harnesses](harnesses.md) first. Keep the existing graph or agent. Use
`langchain-mcp-adapters` in Python or the project's LangChain MCP adapter in
TypeScript, checking the installed version's API.

For Python, initialize `MultiServerMCPClient` with the blueprint's Streamable
HTTP URL and trusted headers. Use an explicit `client.session("submilli")`
context and `load_mcp_tools(session)`; supply the resulting tools to the
existing `create_agent` or graph tool node. Preserve descriptions. Prefer the
explicit session for Submilli's session-bound variables and filesystem state;
do not accidentally turn a multi-step run into unrelated sessions per tool.

For LangGraph, attach tools at the existing execution node and preserve the
graph's state, routing, checkpoints, and user approval flow. Keep model secrets,
MCP clients, and live session objects out of serializable checkpoint state.
On resume after a closed session, reconstruct trusted bindings from authorized
application state; do not trust a stored model message to choose the tenant.

Do not conflate a LangGraph conversation `thread_id` with a Submilli session
ID or an authenticated customer ID. Test success, failure cleanup, and
cross-tenant isolation, including a resumed conversation when relevant.

Official [Python MCP integration](https://docs.langchain.com/oss/python/langchain/mcp)
and [TypeScript MCP integration](https://docs.langchain.com/oss/javascript/langchain/mcp).
