# Connect the user's harness

Submilli is the execution boundary, not the model loop. Keep the user's harness
and provider. Do not introduce Submilli's own agent-server as a harness.

Choose just the relevant guide:

- [Deep Agents](deepagents.md)
- [Vercel AI SDK](vercel.md)
- [LangChain / LangGraph](langchain.md)
- [Mastra](mastra.md)
- [Custom model loop / REST](custom-loop.md)

## Prepare a working Submilli endpoint

For an existing app, reuse its package, blueprint and authenticated identity.
For a fresh project, use this offline billing fixture before connecting a real
service. Install the CLI/server using [setup](setup.md), then:

1. In a new project directory, run
   `submilli build init @acme/billing packages/billing`.
2. Replace `packages/billing/src/lib.ts` with the **smallest slice**
   `readBalance` example in [packages](packages.md). Replace the scaffold's
   `hello` test in `packages/billing/tests/lib.test.ts` with:

   ```typescript
   import { readBalance } from "@acme/billing";
   function main(): void { assert(readBalance("cus_northwind") === 6150); }
   ```

3. Save the **smallest slice** YAML in [blueprints](blueprints.md) as
   `blueprint.yaml`. It registers `support-read`, requires `customerId`, and
   permits only the bound customer's balance.
4. Run `submilli build check`, `submilli build test`,
   `submilli build publish-local`, and `submilli blueprint lint blueprint.yaml`.
5. The server refuses to start without API tokens ([setup](setup.md)). Save
   `server.yaml`:

   ```yaml
   api_tokens:
     - { name: ops, role: admin, token_env: SUBMILLI_ADMIN_TOKEN }
     - { name: app, role: user, token_env: SUBMILLI_USER_TOKEN }
   ```

   Export both tokens, then start the server in the background:

   ```sh
   export SUBMILLI_ADMIN_TOKEN=$(openssl rand -hex 32)
   export SUBMILLI_USER_TOKEN=$(openssl rand -hex 32)
   submilli-server --config server.yaml --bind 127.0.0.1 --port 8128 &
   ```

   Any other shell that runs the CLI or the harness needs the same values.
   Use the same `SUBMILLI_HOME` for the CLI and server if overriding the default:
   the server must see the package store where you published the fixture.
6. Run `submilli server blueprint apply blueprint.yaml --server http://127.0.0.1:8128`;
   the CLI sends `SUBMILLI_ADMIN_TOKEN`.
   The MCP URL is now `http://127.0.0.1:8128/mcp/support-read`.

Before involving a model, verify the endpoint:

```sh
curl --fail-with-body http://127.0.0.1:8128/v1/execute \
  -H "Authorization: Bearer $SUBMILLI_USER_TOKEN" \
  -H 'content-type: application/json' \
  -d '{"blueprint":"support-read","variables":{"customerId":"cus_northwind"},"code":"import { readBalance } from \"@acme/billing\"; function main(): number { return readBalance(\"cus_northwind\"); }"}'
```

Expect `result: "6150"`. Change only the program's argument to `cus_initech`:
expect a capability denial. Remove `variables`: expect `invalid_request` for
the missing binding. Drop the `Authorization` header: expect `401`. The fixture returns a constant; these checks prove the
policy path, not connectivity to a billing service. Replace it with reviewed
service operations only after this slice works. Each harness guide below
uses this fixture and labels its hard-coded demo identity explicitly.

## Adapt the harness

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

Send `Authorization: Bearer <token>` on every MCP and REST request, with the
`user` token read from `SUBMILLI_USER_TOKEN` in the host process. The harness
and agent never get the admin token (it could rewrite the blueprint), and no
token goes into a prompt, tool schema, or generated code. A missing or unknown
token is `401` before the binding is read; it is not an OAuth challenge.

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
denial, missing-variable rejection (token still sent), and missing-token
rejection deterministically. Then run one live
model task if access is available. Inspect transcript and results; never claim
a live model test passed when only a mocked adapter ran. The token
authenticates the application, not its end user, and variable binding is not
auth; a network-reachable server also needs TLS and restricted ingress.
