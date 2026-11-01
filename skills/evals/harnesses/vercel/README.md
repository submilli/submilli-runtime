# Vercel AI SDK harness smoke test

## The `@submilli/ai-sdk` package

`validate-package.ts` uses the real package and `generateText` loop against the
local `support-read` fixture; only the model is `MockLanguageModelV4`. Run
`skills/evals/harnesses/serve_fixture.py` first, then from a temporary directory:

```sh
npm init -y && npm pkg set type=module
npm install @submilli/ai-sdk ai tsx
cp /path/to/submilli-runtime/skills/evals/harnesses/vercel/validate-package.ts .
export SUBMILLI_SERVER_URL=http://127.0.0.1:18128 SUBMILLI_SERVER_TOKEN=...   # the line the fixture prints
SUBMILLI_SERVER=$SUBMILLI_SERVER_URL npx tsx validate-package.ts
```

It asserts the matrix in `references/vercel.md` ("Deterministic verification
with a scripted model") and ends with
`vercel package + mock model validation passed`. Verified on 2026-11-01 with
`@submilli/ai-sdk 0.1.0` and `ai 7.0.159`.

## MCP with `@ai-sdk/mcp`

Uses the real `@ai-sdk/mcp` HTTP transport and `generateText` loop against the
local `support-read` fixture; only the model is `MockLanguageModelV3`. Run
`skills/evals/harnesses/serve_fixture.py` first, then from a temporary directory:

```sh
npm init -y
npm install ai@6.0.0 @ai-sdk/mcp@2.0.0 tsx@4.19.2 zod@4.1.8
cp /path/to/submilli-runtime/skills/evals/harnesses/vercel/validate.ts .
export SUBMILLI_SERVER_URL=http://127.0.0.1:18128 SUBMILLI_SERVER_TOKEN=...   # the line the fixture prints
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
