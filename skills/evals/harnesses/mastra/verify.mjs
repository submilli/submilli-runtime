#!/usr/bin/env node

import assert from 'node:assert/strict';
import { pathToFileURL } from 'node:url';

const mastraMcpEntry = process.env.MASTRA_MCP_ENTRY;
const { MCPClient } = await import(
  mastraMcpEntry === undefined ? '@mastra/mcp' : pathToFileURL(mastraMcpEntry).href,
);

const base = process.env.SUBMILLI_SERVER_URL ?? 'http://127.0.0.1:18128';
const blueprint = `${base}/mcp/support-read`;

function program(customerId) {
  return `import { readBalance } from "@acme/billing"; function main(): number { return readBalance("${customerId}"); }`;
}

async function clientFor(binding) {
  const requestInit = binding === null
    ? undefined
    : { headers: { 'submilli-variables': `customerId=${binding}` } };
  return new MCPClient({
    id: `submilli-smoke-${binding ?? 'missing'}`,
    servers: { submilli: { url: new URL(blueprint), requestInit } },
  });
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

console.log('Mastra MCP smoke passed: allowed, cross-tenant denial, missing binding, cleanup');
