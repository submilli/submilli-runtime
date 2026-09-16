# Vercel AI SDK

Read [harnesses](harnesses.md) first. Use the app's installed `ai` and MCP
adapter versions. Current SDKs expose `createMCPClient` from `@ai-sdk/mcp`;
older versions used experimental exports from `ai`. Check installed types.

Create the MCP client inside the authenticated server request handler, with
`transport: { type: "http", url, headers }`. Set the blueprint URL and trusted
`submilli-variables` header before initialization. Obtain tools using
`await client.tools()` and pass them unchanged to `generateText`, `streamText`,
or the project's existing agent loop. Use the existing provider/model and
configure a finite multi-step stopping condition supported by that SDK version
(for example `stopWhen: stepCountIs(8)`); a single tool step may not return the
agent's final answer. Handle exhausted budgets explicitly.

For non-streaming execution, close the client with `await client.close()` in
`finally`. For streams, arrange cleanup after completion, cancellation, and
failure using the SDK/HTTP framework lifecycle. Avoid closing in a handler's
`finally` before the stream consumes its tool calls. Never put business
credentials or the trusted Submilli connection in browser code.

Verify the adapter with a mock model/tool call sequence and a real local
Submilli server, then a live model when available. Assert the serialized
tool-call schema contains code but no model-controlled blueprint/identity
binding. Test separate concurrent tenant requests to catch shared closures.

Official [MCP integration guide](https://ai-sdk.dev/docs/ai-sdk-core/mcp-tools)
and [tool calling](https://ai-sdk.dev/docs/ai-sdk-core/tools-and-tool-calling).
