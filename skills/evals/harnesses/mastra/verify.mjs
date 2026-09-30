#!/usr/bin/env node

import assert from 'node:assert/strict';
import { pathToFileURL } from 'node:url';

const mastraMcpEntry = process.env.MASTRA_MCP_ENTRY;
const { MCPClient } = await import(
  mastraMcpEntry === undefined ? '@mastra/mcp' : pathToFileURL(mastraMcpEntry).href,
);

const base = process.env.SUBMILLI_SERVER_URL ?? 'http://127.0.0.1:18128';
const blueprint = `${base}/mcp/support-read`;
// The fixture's user token; serve_fixture.py prints it.
const token = process.env.SUBMILLI_USER_TOKEN;
assert.ok(token, 'SUBMILLI_USER_TOKEN is required');

function program(customerId) {
  return `import { readBalance } from "@acme/billing"; function main(): number { return readBalance("${customerId}"); }`;
}

// The token admits the application; the binding says which customer.
async function clientFor(binding, bearer = token) {
  const headers = {};
  if (bearer !== null) headers.Authorization = `Bearer ${bearer}`;
  if (binding !== null) headers['submilli-variables'] = `customerId=${binding}`;
  return new MCPClient({
    id: `submilli-smoke-${binding ?? 'missing'}-${bearer === null ? 'anonymous' : 'token'}`,
    servers: { submilli: { url: new URL(blueprint), requestInit: { headers } } },
  });
}

// Mastra logs a refused connection and offers no toolset; this returns why.
async function refusal(binding, bearer) {
  const client = await clientFor(binding, bearer);
  try {
    const { toolsets, errors } = await client.listToolsetsWithErrors();
    assert.equal(toolsets.submilli, undefined, 'a refused connection offers no tools');
    return errors.submilli ?? '';
  } finally {
    await client.disconnect();
  }
}

async function execute(binding, requestedCustomer) {
  const client = await clientFor(binding);
  try {
    const toolsets = await client.listToolsets();
    const tool = toolsets.submilli?.submilli__typescript__execute;
    assert.ok(tool, 'Submilli execution tool was not discovered');
    return await tool.execute({ code: program(requestedCustomer) });
  } finally {
    await client.disconnect();
  }
}

const allowed = await execute('cus_northwind', 'cus_northwind');
assert.equal(allowed.error, null, JSON.stringify(allowed));
assert.equal(allowed.result, '6150', JSON.stringify(allowed));

const denied = await execute('cus_northwind', 'cus_initech');
assert.equal(denied.result, null, JSON.stringify(denied));
assert.match(denied.error?.message ?? '', /PermissionDeniedError/);
assert.match(denied.error?.message ?? '', /acme\.com\/balance\.read/);

await assert.rejects(
  () => execute(null, 'cus_northwind'),
  /required|variable|invalid_request|connect|toolsets|discovered/i,
);

// The missing binding is refused with the token present, so not as a 401.
assert.doesNotMatch(await refusal(null, token), /401|unauthorized/i);
// Without a token the server answers 401 before it reads the binding. The
// adapter has no OAuth provider, so it reports the refusal and starts no sign-in.
assert.match(await refusal('cus_northwind', null), /unauthorized[\s\S]*HTTP 401/);

console.log('Mastra MCP smoke passed: allowed, cross-tenant denial, missing binding, missing token, cleanup');
