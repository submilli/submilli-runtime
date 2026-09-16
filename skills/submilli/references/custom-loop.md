# A custom agent loop

Read [harnesses](harnesses.md) first. A custom loop can use an MCP SDK to
initialize the blueprint endpoint, discover tools, translate tool schemas to
the model provider, call tools, and return results until completion or a
finite turn limit. Keep names, schemas, and descriptions intact. Handle MCP
errors and denial results without silently retrying via another tool.

Alternatively expose an application-owned `run_submilli(code)` tool calling
REST. Its model-visible arguments contain only code. The closure supplies
the authorized blueprint and variables:

```javascript
// Ordinary host JavaScript, not code executed inside Submilli.
async function runSubmilli(code, trustedCustomerId) {
  const response = await fetch("http://127.0.0.1:8128/v1/execute", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({
      blueprint: "support-read",
      code,
      variables: { customerId: trustedCustomerId },
    }),
  });
  if (!response.ok) throw new Error(`Submilli HTTP ${response.status}`);
  const body = await response.json();
  if (body.error) throw new Error(body.error.message);
  return { result: body.result, console: body.console ?? [] };
}
```

`trustedCustomerId` comes from the authenticated handler and must not appear
in the model's tool schema. Keep the server URL and blueprint selection
application-owned too. Use a timeout and finite model-loop budget. An HTTP
success can still contain a runtime error; surface it. Avoid logging secret
headers or unnecessary customer data in errors/transcripts.

REST alone does not load MCP tool descriptions. Supply the resolved runtime
prompt from `submilli blueprint prompt` and relevant installed package/builtin
declarations, or implement discovery equivalent to the MCP tools. Do not
tell the model this runs arbitrary JavaScript. Verify the exact CLI options
for rendering the prompt in the installed release.

Implement the provider's ordinary tool-result feedback loop, with tool-call
IDs preserved and runtime results fed back as tool outputs. Keep provider
credentials entirely outside generated code. A thin HTTP function without
that loop is only an execution adapter, not a completed agent implementation.
