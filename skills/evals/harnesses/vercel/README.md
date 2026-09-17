# Vercel AI SDK harness smoke test

Uses the real `@ai-sdk/mcp` HTTP transport and `generateText` loop against the
local `support-read` fixture; only the model is `MockLanguageModelV3`. Run
`skills/evals/harnesses/serve_fixture.py` first, then from a temporary directory:

```sh
npm init -y
npm install ai@6.0.0 @ai-sdk/mcp@2.0.0 tsx@4.19.2 zod@4.1.8
cp /path/to/submilli-runtime/skills/evals/harnesses/vercel/validate.ts .
SUBMILLI_SERVER_URL=http://127.0.0.1:18128 npx tsx validate.ts
```

It asserts discovery with the description and `code` schema intact, an allowed
`6150` read directly and through the mock-model loop, cross-customer denial,
and that a client without the binding is rejected at `createMCPClient`.
`transport warning` lines for `GET SSE failed: 400` and empty SSE events are
expected with the current server (see `references/vercel.md`); the run ends
with `vercel MCP + mock model validation passed`. The script is run with
`tsx` and is not held to strict `tsc`.
