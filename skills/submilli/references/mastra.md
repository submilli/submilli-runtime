# Mastra

Read [harnesses](harnesses.md) first. Keep the existing Mastra agent, model,
memory, and workflow. Inspect installed `@mastra/core` and `@mastra/mcp` types.

Configure `MCPClient` from `@mastra/mcp` with a server URL for the blueprint
and `requestInit.headers` containing trusted session variables. Use a client
per authorized identity/request. For per-request credentials or bindings,
load runtime toolsets (`listToolsets()` on versions supporting it) and pass
them into the existing agent's `generate` or `stream` call. `listTools()` is
appropriate when constructing an agent confined to that same binding; do not
cache a customer's tool closures in a global multi-tenant agent.

Disconnect the client on completion and errors; defer cleanup to stream
termination for streaming responses. Preserve MCP tool descriptions and the
agent's existing loop limits. Review any other direct integration tools that
would bypass Submilli policy. Tool approval in Mastra does not replace argument
checks in packages and blueprints.

Implement and test both tenants, missing bindings, denied calls, and client
cleanup. Follow the installed version if method names have changed.

Official [Mastra MCP integration](https://mastra.ai/docs/connections/mcp).
