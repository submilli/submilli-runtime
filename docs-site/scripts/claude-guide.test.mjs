import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { EventEmitter } from "node:events";
import { stripTypeScriptTypes } from "node:module";
import test from "node:test";

// Execute the published code fence so lifecycle regressions in the guide fail CI.
const markdown = readFileSync(
  new URL("../../docs/part-5-tutorials/connect-a-harness/06-connect-claude-agent-sdk.md", import.meta.url),
  "utf8",
);
const source = markdown.match(/```typescript title="agent.ts"\n([\s\S]*?)\n```/)[1];
const executable = stripTypeScriptTypes(source.replace(/^import .*;\n/gm, ""));
const AsyncFunction = Object.getPrototypeOf(async function () {}).constructor;
const execute = new AsyncFunction(
  "query", "Client", "StreamableHTTPClientTransport", "McpServer",
  "CallToolRequestSchema", "ListToolsRequestSchema", "process", "console",
  executable,
);
const list = {};
const call = {};
const catalog = {
  tools: [{
    name: "submilli__typescript__execute",
    inputSchema: { type: "object", properties: { code: { type: "string" } } },
  }],
};

function fixture(failure) {
  const active = new Set();
  const state = { models: 0, clientClosed: 0, queryClosed: 0, bridgeClosed: 0, deletes: 0, cleanupClosed: 0 };
  const process = Object.assign(new EventEmitter(), {
    argv: ["node", "agent.ts", "calculate"],
    env: { SUBMILLI_BLUEPRINT: "test", SUBMILLI_SERVER_TOKEN: "placeholder" },
  });
  let transport;
  class Transport {
    constructor(endpoint, options) {
      assert.equal(endpoint.href, "http://127.0.0.1:8128/mcp/test");
      assert.deepEqual(options.requestInit.headers, { Authorization: "Bearer placeholder" });
      this.sessionId = options.sessionId;
      this.cleanup = Boolean(options.sessionId);
    }
    setProtocolVersion(version) { this.protocolVersion = version; }
    async close() {
      this.closed = true;
      if (this.cleanup) state.cleanupClosed++;
    }
    async terminateSession() {
      assert.equal(this.closed, undefined, "DELETE must use an open transport");
      assert.equal(this.protocolVersion, "2025-11-25");
      state.deletes++;
      if (failure === "delete") throw new Error("delete failed");
      active.delete(this.sessionId);
      this.sessionId = undefined;
    }
  }
  class Client {
    async connect(value, options) {
      transport = value;
      if (["unknown Blueprint", "missing context", "unauthorized"].includes(failure)) {
        throw new Error(failure);
      }
      transport.sessionId = `session-${state.models}`;
      transport.protocolVersion = "2025-11-25";
      active.add(transport.sessionId);
      if (failure?.startsWith("setup SIG")) {
        process.emit(failure.slice("setup ".length));
        options.signal.throwIfAborted();
      }
      if (failure === "initialize") {
        // The real SDK closes its transport before rejecting initialization.
        await this.close();
        throw new Error("initialize failed after session allocation");
      }
    }
    async listTools() {
      if (failure === "catalog") throw new Error("catalog unavailable");
      return failure === "missing tool" ? { tools: [] } : catalog;
    }
    getInstructions() { return "Use these Submilli tools."; }
    async callTool(params) {
      assert.deepEqual(params, { name: "submilli__typescript__execute", arguments: { code: "2 + 2" } });
      return { content: [{ type: "text", text: "4" }], isError: false };
    }
    async close() {
      state.clientClosed++;
      await transport.close();
    }
  }
  class McpServer {
    handlers = new Map();
    server = { setRequestHandler: (schema, handler) => this.handlers.set(schema, handler) };
    constructor(_info, options) {
      assert.equal(options.instructions, "Use these Submilli tools.");
    }
    async close() { state.bridgeClosed++; }
  }
  function query({ options }) {
    state.models++;
    assert.deepEqual(Object.keys(options.mcpServers), ["submilli"]);
    assert.equal(options.strictMcpConfig, true);
    assert.deepEqual(options.allowedTools, ["mcp__submilli__*"]);
    assert.deepEqual(options.tools, []);
    assert.deepEqual(options.settingSources, []);
    const server = options.mcpServers.submilli.instance;
    return {
      close() { state.queryClosed++; },
      async *[Symbol.asyncIterator]() {
        assert.deepEqual(await server.handlers.get(list)({}), catalog);
        if (failure?.startsWith("SIG")) {
          process.emit(failure);
          process.emit(failure); // A second signal must not bypass cleanup.
          options.abortController.signal.throwIfAborted();
        }
        if (failure === "model") throw new Error("model failed");
        const result = await server.handlers.get(call)({
          params: { name: "submilli__typescript__execute", arguments: { code: "2 + 2" } },
        });
        assert.equal(result.content[0].text, "4");
        // Provider failures can have subtype "success" with is_error: true.
        yield { type: "result", subtype: "success", is_error: failure === "result" };
      },
    };
  }
  const run = () => execute(
    query, Client, Transport, McpServer, call, list,
    process,
    { dir() {} },
  );
  return { run, state, active, process };
}

test("repeated guide runs forward Submilli tools and leave no active sessions", async () => {
  const f = fixture();
  for (let i = 1; i <= 3; i++) {
    await f.run();
    assert.equal(f.active.size, 0);
    assert.equal(f.state.deletes, i);
    assert.equal(f.state.clientClosed, i);
    assert.equal(f.state.queryClosed, i);
    assert.equal(f.state.bridgeClosed, i);
    assert.equal(f.state.cleanupClosed, i);
  }
});

for (const failure of ["unknown Blueprint", "missing context", "unauthorized", "initialize", "catalog", "missing tool"]) {
  test(`setup failure (${failure}) stops before model generation and closes allocated sessions`, async () => {
    const f = fixture(failure);
    await assert.rejects(f.run(), failure === "initialize" ? /initialize failed/ : undefined);
    assert.equal(f.state.models, 0);
    assert.equal(f.active.size, 0);
    assert.equal(f.state.clientClosed, failure === "initialize" ? 2 : 1);
  });
}

for (const failure of ["model", "result"]) {
  test(`${failure} failure still closes the query and Submilli session`, async () => {
    const f = fixture(failure);
    await assert.rejects(f.run(), failure === "result" ? /Claude reported an error/ : /model failed/);
    assert.equal(f.active.size, 0);
    assert.equal(f.state.queryClosed, 1);
    assert.equal(f.state.bridgeClosed, 1);
    assert.equal(f.state.clientClosed, 1);
  });
}

test("a failed session deletion is reported and the HTTP client still closes", async () => {
  const f = fixture("delete");
  await assert.rejects(f.run(), /delete failed/);
  assert.equal(f.state.clientClosed, 1);
  assert.equal(f.state.cleanupClosed, 1);
  assert.equal(f.active.size, 1);
});

for (const signal of ["SIGINT", "SIGTERM"]) {
  for (const phase of ["", "setup "]) {
    test(`${phase}${signal} cancels work, deletes the session, and removes signal handlers`, async () => {
      const f = fixture(`${phase}${signal}`);
      await f.run();
      assert.equal(f.active.size, 0);
      assert.equal(f.state.deletes, 1);
      assert.equal(f.state.clientClosed, 1);
      assert.equal(f.process.exitCode, signal === "SIGINT" ? 130 : 143);
      assert.equal(f.process.listenerCount("SIGINT"), 0);
      assert.equal(f.process.listenerCount("SIGTERM"), 0);
      assert.equal(f.state.models, phase ? 0 : 1);
      assert.equal(f.state.queryClosed, phase ? 0 : 1);
    });
  }
}

test("the Deep Agents guide retains the Python README's application-tools anchor", () => {
  const deepagents = readFileSync(
    new URL("../../docs/part-5-tutorials/connect-a-harness/04-connect-deepagents.md", import.meta.url),
    "utf8",
  );
  assert.match(deepagents, /<a id="your-tools-in-programs"><\/a>\s+To reuse application tools/);
});
