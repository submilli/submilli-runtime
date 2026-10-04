---
title: "Connect Claude Agent SDK"
description: "Run the research agent on the Claude Agent SDK with every action going through Submilli: the SDK's own tools removed, only this server's tools allowed, then a real conversation."
slug: tutorials/connect-claude-agent-sdk
sidebar:
  order: 6
---

In this tutorial we will run the research agent on the Claude Agent SDK,
with every action going through Submilli: its programs executed on the
server as the signed-in user, `u_ada` in the examples, then one real conversation.
You need the server and the `research` blueprint from [Connect a
harness](/docs/tutorials/connect-a-harness), with
`SUBMILLI_SERVER_TOKEN` still exported, Node.js 20 or later, and an
Anthropic key for the conversation; this SDK speaks to Claude,
whichever provider the blueprint's own model uses.

## Start the project

In `harnesses`, make a directory for this harness and install the
dependencies; the files use top-level `await`, hence `type=module`. The
full project is
[`examples/harnesses/claude-agent-sdk/`](https://github.com/submilli/submilli-runtime/tree/main/examples/harnesses/claude-agent-sdk).

```sh
mkdir claude-agent-sdk && cd claude-agent-sdk
npm init -y
npm pkg set type=module
npm install @anthropic-ai/claude-agent-sdk
npm install --save-dev tsx typescript @types/node
```

## The agent

Save this as `agent.ts`. It reads the brief from `../prompt.txt`:

```typescript title="agent.ts"
// A Claude Agent SDK agent that runs its programs on submilli-server, over MCP.

import { readFileSync } from "node:fs";
import { query, type Options } from "@anthropic-ai/claude-agent-sdk";

const SUBMILLI_SERVER = process.env.SUBMILLI_SERVER ?? "http://127.0.0.1:8128";
const BLUEPRINT = "research";

// The agent's brief, kept beside the blueprint.
const INSTRUCTIONS = readFileSync(new URL("../prompt.txt", import.meta.url), "utf8");

export function options(userId: string): Options {
  return {
    model: "claude-sonnet-5",
    systemPrompt: INSTRUCTIONS,
    mcpServers: {
      // One entry per user: the binding is fixed when the agent connects.
      submilli: {
        type: "http",
        url: `${SUBMILLI_SERVER}/mcp/${BLUEPRINT}`,
        headers: {
          Authorization: `Bearer ${serverToken()}`,
          "submilli-variables": `userId=${userId}`,
        },
      },
    },
    // Only the server above, whatever else the account or machine has configured.
    strictMcpConfig: true,
    allowedTools: ["mcp__submilli__*"],
    // No shell, no file tools, no web fetch: every action goes through Submilli.
    tools: [],
    settingSources: [],
    maxTurns: 20,
  };
}

export async function answer(question: string, userId: string): Promise<string> {
  for await (const message of query({ prompt: question, options: options(userId) })) {
    if (message.type === "result") {
      return message.subtype === "success" ? message.result : `stopped: ${message.subtype}`;
    }
  }
  throw new Error("the agent ended without a result");
}

/** The API token this application was given for the server. */
function serverToken(): string {
  const token = process.env.SUBMILLI_SERVER_TOKEN;
  if (!token) throw new Error("SUBMILLI_SERVER_TOKEN is not set: export the token the server was started with");
  return token;
}

if (import.meta.filename === process.argv[1]) {
  // In a real application the user comes from the signed-in session.
  console.log(await answer("What is new in the latest stable release of Rust? Save a note with your sources.", "u_ada"));
}
```

Notice the four options after the server. This SDK gives its agent a
shell, file access, and web fetch unless told otherwise, and those tools
run outside Submilli, where no blueprint applies. `tools: []` removes
them, and `allowedTools` lets the agent call Submilli's without asking.
The SDK also loads the MCP servers of whoever runs the process: run under
a Claude login that had two connectors attached, this agent started with
84 tools, eight of them Submilli's. `strictMcpConfig` limits it to the
server named here, and `settingSources: []` keeps that person's other
settings out.

The same caution holds for every harness. A blueprint governs what
programs do. It can't govern a tool the harness offers beside Submilli's.

The SDK takes its credentials from `ANTHROPIC_API_KEY`, from Amazon
Bedrock or Google Vertex AI when `CLAUDE_CODE_USE_BEDROCK` or
`CLAUDE_CODE_USE_VERTEX` is set, or from a Claude Code login on the
machine.

## One conversation

```sh
ANTHROPIC_API_KEY=... npx tsx agent.ts
```

This is one real run, with Claude Sonnet 5 as the model, made after
three other tutorials' agents had answered the same question for the
same user. The model's programs are its own, and another run writes
different ones; the answer began and ended:

```text
The latest stable Rust release is **1.99.0**, released 2026-10-01. I checked this against the official blog post and releases.rs today (2026-10-03), and no newer stable release has shipped.
…
**Notebook:** I updated the existing `/notes/rust-latest-release.md` (it already held a 1.99.0 note from an earlier session). I added a line recording today's verification and why the `/releases/latest/` redirect is not a usable source. The existing content is unchanged.

**Not established:** I did not read the Cargo or Clippy changelogs or the full stable release notes, so the list above covers the blog post's highlights, not every change.
```

Notice the notebook paragraph. The note was written by another harness's
agent, in another conversation, under the same user; this one found it
at `/notes`, read it before searching, and added to it. The notebook
belongs to the user and the blueprint, not to the harness.

You have the research agent running on the Claude Agent SDK with nothing
beside Submilli's tools, every program it writes executed on the server
as the signed-in user, and the binding proved on the index before
any model was involved. Project:
[`examples/harnesses/claude-agent-sdk/`](https://github.com/submilli/submilli-runtime/tree/main/examples/harnesses/claude-agent-sdk).
