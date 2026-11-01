# Mastra harness smoke test

## The `@submilli/mastra` package

`verify-package.ts` uses Mastra, its memory, and the real package against the
local `support-read` fixture; only the model is `MockLanguageModelV4` from
`ai/test`. Run `skills/evals/harnesses/serve_fixture.py` first, then from a
temporary directory:

```sh
npm init -y && npm pkg set type=module
npm install @submilli/mastra @mastra/core @mastra/memory ai tsx
cp /path/to/submilli-runtime/skills/evals/harnesses/mastra/verify-package.ts .
export SUBMILLI_SERVER_URL=http://127.0.0.1:18128 SUBMILLI_SERVER_TOKEN=...   # the line the fixture prints
SUBMILLI_SERVER=$SUBMILLI_SERVER_URL npx tsx verify-package.ts
```

It asserts the matrix in `references/mastra.md` ("Deterministic verification
with a scripted model") and ends with
`mastra package + mock model validation passed`. Verified on 2026-11-01 with
`@submilli/mastra 0.1.0`, `@mastra/core 1.81.0` and `@mastra/memory 1.40.0`.

## MCP with `@mastra/mcp`

This maintainer check uses the real `@mastra/mcp` adapter and the local
Submilli `support-read` fixture. It does not call a model or a business
service. The scripted program is the model boundary: the MCP client, tool
discovery, tool execution, policy denial, and cleanup are real.

From a temporary directory, install the versions documented in
`skills/submilli/references/mastra.md`:

```sh
npm install --no-save @mastra/mcp@1.18.0 @mastra/core@1.67.0
export SUBMILLI_SERVER_URL=http://127.0.0.1:18128 SUBMILLI_SERVER_TOKEN=...   # the line the fixture prints
node /path/to/submilli-runtime/skills/evals/harnesses/mastra/verify.mjs
```

When the temporary install is outside the repository, set
`MASTRA_MCP_ENTRY` to its `node_modules/@mastra/mcp/dist/index.js` path so the
script can resolve the adapter from that dependency directory.

Run `skills/evals/harnesses/serve_fixture.py --port 18128` first; it prints
the `export` line with the token the script sends. The script asserts
that a `pied_piper` client can read its own balance, that the same client
receives a `PermissionDeniedError` when its program requests `hooli`,
that a missing binding fails for a reason other than authentication, that a
client without the token is refused with HTTP 401 and no OAuth flow, and that
every client is disconnected in a `finally` block.
