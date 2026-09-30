# Vercel AI SDK harness smoke test

Uses the real `@ai-sdk/mcp` HTTP transport and `generateText` loop against the
local `support-read` fixture; only the model is `MockLanguageModelV3`. Run
`skills/evals/harnesses/serve_fixture.py` first, then from a temporary directory:

```sh
npm init -y
npm install ai@6.0.0 @ai-sdk/mcp@2.0.0 tsx@4.19.2 zod@4.1.8
cp /path/to/submilli-runtime/skills/evals/harnesses/vercel/validate.ts .
export SUBMILLI_SERVER_URL=http://127.0.0.1:18128 SUBMILLI_USER_TOKEN=...   # the line the fixture prints
npx tsx validate.ts
```

It asserts discovery with the description and `code` schema intact, an allowed
`6150` read directly and through the mock-model loop, cross-customer denial,
and that a client without the binding is rejected at `createMCPClient` with
the token present (HTTP 400), and a client without the token with HTTP 401 and
no OAuth flow. `transport warning` lines for `GET SSE failed: 400`, empty SSE
events, and the two refused clients' 400 and 401 are expected with the current
server (see `references/vercel.md`); the run ends
with `vercel MCP + mock model validation passed`. The script is run with
`tsx` and is not held to strict `tsc`.
