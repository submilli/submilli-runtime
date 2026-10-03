---
title: "Connect Mastra"
description: "Run the research agent on Mastra: its programs executed on the server as the signed-in user, then one real conversation watched end to end."
slug: tutorials/connect-mastra
sidebar:
  order: 3
---

In this tutorial we will run the research agent on Mastra: its programs
executed on the server as the signed-in user, `u_ada` in the examples, then one real conversation watched end to end. You need the server and the `research`
blueprint from [Connect a harness](/docs/tutorials/connect-a-harness),
with `SUBMILLI_SERVER_TOKEN` still exported, Node.js 20 or later, and a
key from your model provider for the conversation. The agent file
names Claude; for Google or OpenAI, the `model` argument takes
`google/gemini-3.8-flash` or `openai/gpt-4o-mini` instead, with that
provider's key in the environment.

## Start the project

In `harnesses`, make a directory for this harness and install the
dependencies; the files use top-level `await`, hence `type=module`. The
full project is
[`examples/harnesses/mastra/`](https://github.com/submilli/submilli-runtime/tree/main/examples/harnesses/mastra).

```sh
mkdir mastra && cd mastra
npm init -y
npm pkg set type=module
npm install @mastra/core @mastra/mcp
npm install --save-dev tsx typescript @types/node
```

## The agent

Save this as `agent.ts`. It reads the brief from `../prompt.txt`:

```typescript title="agent.ts"
// A Mastra agent that runs its programs on submilli-server, over MCP.

import { readFileSync } from "node:fs";
import { Agent } from "@mastra/core/agent";
import { MCPClient } from "@mastra/mcp";

const SUBMILLI_SERVER = process.env.SUBMILLI_SERVER ?? "http://127.0.0.1:8128";
const BLUEPRINT = "research";

type Model = ConstructorParameters<typeof Agent>[0]["model"];

function instructions(userId: string): string {
  // The agent's brief, kept beside the blueprint; `{userId}` names the user.
  const brief = readFileSync(new URL("../prompt.txt", import.meta.url), "utf8");
  return brief.replaceAll("{userId}", userId);
}

export async function answer(
  question: string,
  userId: string,
  model: Model = "anthropic/claude-haiku-4-5",
): Promise<string> {
  const agent = new Agent({
    id: "researcher",
    name: "Researcher",
    instructions: instructions(userId),
    model,
  });

  // One client per user: the binding is fixed when the client connects.
  const submilli = new MCPClient({
    id: `submilli-${userId}`,
    servers: {
      submilli: {
        url: new URL(`${SUBMILLI_SERVER}/mcp/${BLUEPRINT}`),
        requestInit: {
          headers: {
            Authorization: `Bearer ${serverToken()}`,
            "submilli-variables": `userId=${userId}`,
          },
        },
      },
    },
  });

  try {
    const toolsets = await submilli.listToolsets();
    // Mastra logs a refused connection and carries on with no tools.
    if (toolsets.submilli === undefined) throw new Error("submilli-server refused the connection");

    const result = await agent.generate(question, { toolsets, maxSteps: 20 });
    return result.text;
  } finally {
    await submilli.disconnect();
  }
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

Notice where the tools go: to `generate`, as `toolsets`, per request,
and not to the agent when it is constructed. An agent is usually built
once and shared, and tools given to it would carry one user's binding
into another's conversation.

Keep the check on `toolsets.submilli`. When the server refuses the
connection, Mastra logs the error and returns no tools, and the model
would answer without them. `maxSteps` bounds the loop: the agent stops
after twenty rounds of tool calls whether or not it has an answer.

## One conversation

Now the real thing:

```sh
ANTHROPIC_API_KEY=... npx tsx agent.ts
```

This is one real run, with Claude Haiku 4.5 as the model. The programs
are the model's own, unedited; another run, or another model, writes
different ones, so watch for the shape rather than the text. The
application asks, on behalf of `u_ada`:

```text
What is new in the latest stable release of Rust? Save a note with your sources.
```

The model starts by finding out what it can import: the package search
tool, then a first program written from a guess at the package's API,
which doesn't compile. A compile error is an ordinary tool result, and
the model reads it and asks for the documentation of `@submilli/jina`,
`submilli:fs`, and `submilli:llm` before trying again. Two more programs
fail the same way and are repaired. Then one asks the model's own model
by a name the blueprint doesn't list:

```text
error: Error: llm.call: llm.call("claude-3-5-sonnet-20241022"): the provider does not serve that model — call llm.models() and use one of: claude-haiku-4-5
```

The next program is the one that does the work: it reads the Rust blog's
index through the package, hands the text to the model the blueprint
allows, and writes the note:

```typescript
import { read, search } from "@submilli/jina";
import { writeText } from "submilli:fs";
import { call, models } from "submilli:llm";

function main(): string {
  // Get available models
  const availableModels = models();
  console.log("Available models:", availableModels.map((m: { name: string }) => m.name).join(", "));
  
  // Search for the latest Rust stable release
  const searchResults = search("Rust latest stable release 2024");
  
  console.log("Search completed");
  
  // Get the official Rust blog
  const blogContent = read("https://blog.rust-lang.org/");
  
  console.log("Blog content retrieved, length:", blogContent.length);
  
  // Use LLM to extract the latest release information
  const prompt = `From the following Rust blog content, extract information about the latest stable release of Rust. Include:
1. The version number
2. The release date
3. Key features and improvements
4. Any breaking changes
5. Important notes

Content:
${blogContent.slice(0, 3000)}`;
  
  const releaseInfo = call("claude-haiku-4-5", prompt);
  
  console.log("LLM analysis completed");
  
  // Build the note
  let noteContent = "# Latest Rust Stable Release Information\n\n";
  noteContent = noteContent + "## Release Details\n\n";
  
  const releaseText = releaseInfo.text;
  if (releaseText !== null) {
    noteContent = noteContent + releaseText;
  } else {
    noteContent = noteContent + "Unable to retrieve release information.";
  }
  
  noteContent = noteContent + "\n\n## Sources\n";
  noteContent = noteContent + "- https://blog.rust-lang.org/ (Official Rust Blog)\n";
  noteContent = noteContent + "- Search query: 'Rust latest stable release 2024'\n";
  noteContent = noteContent + `- Generated: ${Temporal.Now.instant().toString()}\n`;
  
  // Save to notebook
  writeText("/u_ada/notes/rust_latest_release.md", noteContent);
  
  return "Successfully saved Rust latest release information to rust_latest_release.md";
}
```

```text
Successfully saved Rust latest release information to rust_latest_release.md
```

The model reads the note back with the files tool and answers the user:

```text
## Latest Rust Stable Release

Based on my search and analysis of the official Rust blog, **the latest stable release is Rust 1.99.0**, released on **October 1, 2026**.
…
### Note Saved:
I've saved the information to `/u_ada/notes/rust_latest_release.md` for future reference. The note includes the release details, sources, and links to the official blog post for full feature details.
```

Four things to notice. Discovery came first, and every failure on the
way, a wrong import, a type error, a model the blueprint doesn't serve,
came back as a result the model read and corrected; nothing left the
server. The real work happened in one program, the page read, the
summary, and the note together, which is what the brief asks for. The
run took ten tool calls, which is why the example allows twenty steps.
And the note is a file on the server's volume, there for the next
conversation `u_ada` opens; ask the same question again and the model
reads it before it searches.

## With your coding agent

With the [skill](/docs/install#the-skill) installed, your coding
assistant does this for an application you already have. The project
was a small Mastra app with no Submilli in it: an agent, and an HTTP
handler that takes the signed-in user from an `x-user-id` header set by
the company's login proxy.

```text
Connect this app's Mastra agent to the research blueprint on my local Submilli server, so it does its research by running programs there, as the signed-in user.
```

The assistant adds `@mastra/mcp` and writes a `research` function that
follows this page: a new `MCPClient` for each request, the user in the
`submilli-variables` header, the tools passed to `generate` as
`toolsets`, a check that the Submilli toolset loaded, and `disconnect`
in a `finally`. The handler answers 502 when the tools are missing. It
noticed on its own that the user id ends up in the blueprint's path
filters, and accepts only ids of letters, digits, and `_ . @ -`; then it
says it guessed that format and asks what your ids look like.

It tests with a real MCP client and no model: the eight tools load, the
user's own directory can be read, while another user's directory, the
volume's root, and a look-alike directory that starts with the user's id
are refused, and a connection with no user gets no tools. It reports that
it did not run a model. Run afterwards, the app answered the
question above in about a minute and saved its note under `/u_ada`.

You have the research agent running on Mastra, every program it writes
executed on the server as the signed-in user, and the binding proved on the index
before any model was involved. Project:
[`examples/harnesses/mastra/`](https://github.com/submilli/submilli-runtime/tree/main/examples/harnesses/mastra).
