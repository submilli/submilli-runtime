# Connect the user's harness

Submilli is the execution boundary, not the model loop. Keep the user's harness
and provider. Do not introduce Submilli's own agent-server as a harness.

Choose just the relevant guide:

- [Deep Agents](deepagents.md)
- [Vercel AI SDK](vercel.md)
- [LangChain / LangGraph](langchain.md)
- [Mastra](mastra.md)
- [Custom model loop / REST](custom-loop.md)

Inspect the project's installed dependency versions, lockfile, existing model
construction, and authenticated request handler. Check that version's official
API docs/types before generating code; do not upgrade the whole application
to match an example. Install only the missing integration dependencies using
the project's package manager. Include imports, configuration, a runnable
entrypoint, error handling, and a bounded loop when implementing an agent.

The MCP endpoint is `http://127.0.0.1:8128/mcp/<blueprint-name>` using Streamable
HTTP. Load tools from that endpoint; preserve their names, schemas, and full
descriptions. The execution tool's description teaches the TypeScript subset;
discovery tools supply package and built-in declarations. Do not replace this
with a generic “execute JavaScript” schema or assume there is only one tool.

Bind session variables before MCP initialization, from authenticated and
authorized application state. Header format:
`submilli-variables: customerId=cus_northwind` (multiple bindings use `;`).
Validate values against the application's ID format; never interpolate a
value containing `;`, CR, or LF. Clients that support it can use initialize
`_meta.variables` with string values instead. Values from a chat message are
not a trusted identity source.

Create a session/client for each identity binding; do not mutate headers on a
shared client or reuse one tenant's tool closures for another tenant. Keep the
session alive through the agent run, then close it, including on errors. In a
streaming response, cleanup belongs after stream completion/cancellation, not
immediately after creating the stream.

Audit the harness's other tools and subagents. Raw HTTP, shell execution, or
direct provider tools with business credentials can bypass Submilli entirely.
Restrict those paths if the user expects all business actions to be governed;
explain the actual boundary. Keep model credentials in the host application
and service credentials in the intended runtime/package credential path.

First test initialization, tool discovery, an allowed call, a cross-identity
denial, and missing-variable rejection deterministically. Then run one live
model task if access is available. Inspect transcript and results; never claim
a live model test passed when only a mocked adapter ran. Network-accessible
production servers need trusted ingress; variable binding alone is not auth.
