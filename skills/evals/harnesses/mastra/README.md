# Mastra harness smoke test

This maintainer check uses the real `@mastra/mcp` adapter and the local
Submilli `support-read` fixture. It does not call a model or a business
service. The scripted program is the model boundary: the MCP client, tool
discovery, tool execution, policy denial, and cleanup are real.

From a temporary directory, install the versions documented in
`skills/submilli/references/mastra.md`:

```sh
npm install --no-save @mastra/mcp@1.18.0 @mastra/core@1.67.0
SUBMILLI_SERVER_URL=http://127.0.0.1:18128 node /path/to/submilli-runtime/skills/evals/harnesses/mastra/verify.mjs
```

When the temporary install is outside the repository, set
`MASTRA_MCP_ENTRY` to its `node_modules/@mastra/mcp/dist/index.js` path so the
script can resolve the adapter from that dependency directory.

Run `skills/evals/harnesses/serve_fixture.py --port 18128` first. The script
asserts that a `cus_northwind` client can read its own balance, that the same
client receives a `PermissionDeniedError` when its program requests
`cus_initech`, that a missing binding fails, and that every client is
disconnected in a `finally` block.
