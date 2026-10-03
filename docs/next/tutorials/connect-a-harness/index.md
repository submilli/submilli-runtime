---
title: "Connect a harness"
description: "Set up the server and the research blueprint the five harness tutorials share, prove it with one program, and know the three things a harness decides when it opens a session."
slug: next/tutorials/connect-a-harness
pagefind: false
sidebar:
  hidden: true
---

Your application, or the agent framework it uses, is the **harness**: the
code that runs the agent's loop. Submilli replaces none of it. The harness
keeps the model and the loop, and gains one tool: it takes a program the
model wrote and runs it on the server under your blueprint.

The tutorials in this part build the same research agent on five
harnesses, one per page. This page sets up what they share, the server
and the blueprint, in seven steps, then says the three things every
harness does when it connects. Take the page for yours when you are done:

| Harness | Language | Connects over |
| --- | --- | --- |
| [Mastra](/docs/next/tutorials/connect-mastra) | TypeScript | MCP |
| [LangChain deepagents](/docs/next/tutorials/connect-deepagents) | Python | MCP |
| [OpenAI Agents SDK](/docs/next/tutorials/connect-openai-agents) | Python | MCP |
| [Claude Agent SDK](/docs/next/tutorials/connect-claude-agent-sdk) | TypeScript | MCP |
| [Vercel AI SDK](/docs/next/tutorials/use-the-http-api) | TypeScript | The HTTP API |

The agent answers questions by searching the web and reading pages, and
keeps notes so that the next conversation can start from what the last
one learned. It runs on behalf of whoever is signed in to your
application; the examples stand in for that person with one user id,
`u_ada`, whose notebook is the directory `/u_ada`.

## 1. Save the blueprint

Make a directory for the tutorials, `harnesses`, and save this in it:

```yaml title="harnesses/blueprint.yaml"
kind: blueprint
name: research

# Bound once per connection by the application, never by the model.
variables:
  userId:
    required: true

packages:
- '@submilli/jina'

secrets:
  JINA_API_KEY:
    store: jina_api_key
  ANTHROPIC_API_KEY:
    store: anthropic_api_key
  # GOOGLE_API_KEY:
  #   store: google_api_key
  # OPENAI_API_KEY:
  #   store: openai_api_key

# A model the programs may call themselves, to summarize a page or sort
# results without bringing the text back into the conversation. One
# provider is enough; uncomment yours.
llm:
  providers:
    anthropic:
      type: anthropic
      api_key: ${secrets.ANTHROPIC_API_KEY}
    # google:
    #   type: google
    #   api_key: ${secrets.GOOGLE_API_KEY}
    # openai:
    #   type: openai
    #   api_key: ${secrets.OPENAI_API_KEY}
  models:
    claude-haiku-4-5:
      provider: anthropic
      description: Fast and cheap; use for summarizing pages and ranking results.
    # gemini-3.8-flash:
    #   provider: google
    #   description: Fast and cheap; use for summarizing pages and ranking results.
    # gpt-4o-mini:
    #   provider: openai
    #   description: Fast and cheap; use for summarizing pages and ranking results.

# Notes outlive the conversation: every session shares the `notes` volume, and
# the rules below give each user one directory of it.
vfs:
  mode: named
  volume: notes

default: deny

permissions:
  # What generated code may do.
  main:
  - capability: jina.ai/search
    action: allow
  - capability: jina.ai/read
    action: allow
  - capability: llm.call
    action: allow
  - capability: fs.read
    filter: path == "/${vars.userId}" or path glob "/${vars.userId}/*"
    action: allow
  - capability: fs.write
    filter: path == "/${vars.userId}" or path glob "/${vars.userId}/*"
    action: allow
  - capability: fs.list
    filter: path == "/${vars.userId}" or path glob "/${vars.userId}/*"
    action: allow
  - capability: fs.stat
    filter: path == "/${vars.userId}" or path glob "/${vars.userId}/*"
    action: allow
  - capability: fs.mkdir
    filter: path == "/${vars.userId}" or path glob "/${vars.userId}/*"
    action: allow

  # What the package itself may do: reach Jina, and read its key. Its
  # downloads save to a path the program chooses, so they get the user's
  # directory too; unscoped, a program could write into another user's
  # notes through the package.
  '@submilli/jina':
  - capability: fs.write
    filter: path glob "/${vars.userId}/*"
    action: allow
  - capability: http.download
    filter: host == "r.jina.ai" and vfs_path glob "/${vars.userId}/*"
    action: allow
  - capability: http.download
    filter: host == "s.jina.ai" and vfs_path glob "/${vars.userId}/*"
    action: allow
  - capability: http.post
    filter: host == "r.jina.ai" and path == "/"
    action: allow
  - capability: http.post
    filter: host == "s.jina.ai" and path == "/"
    action: allow
  - capability: secrets.get
    filter: name == "JINA_API_KEY"
    action: allow
```

In short: the agent may search and read through the curated
`@submilli/jina` package, call one model, and read and write under its
own user's directory on a volume that outlives the session; the package
is confined to that directory too. The file names Anthropic as the
provider and keeps Google and OpenAI entries commented out; uncomment
yours. For what each block does, refer to [Keep files and
state](/docs/next/blueprints/keep-files-and-state), [Allow model
calls](/docs/next/blueprints/allow-model-calls), and [Mount a shared
volume](/docs/next/server/mount-a-shared-volume).

## 2. Save a test program

Beside it, save a program to prove the setup with before any model is
involved. It writes a note in `u_ada`'s directory and lists it:

```typescript title="harnesses/note.ts"
import fs from "submilli:fs";

function main(): string {
    fs.mkdir("/u_ada/notes", true);
    fs.writeText("/u_ada/notes/check.md", "# Written by a check\n");
    const names: string[] = [];
    for (const entry of fs.list("/u_ada/notes", false)) names.push(entry.name);
    return `notes: ${names.join(", ")}`;
}
```

## 3. Save the agent's brief

The harness supplies no system prompt for Submilli: the instructions
that teach a model the language arrive as the execute tool's
description, with this blueprint's packages and rules filled in. What
the harness does supply is the agent's own brief, and every tutorial's
agent reads it from this file, with `{userId}` replaced by the user the
session is for:

```text title="harnesses/prompt.txt"
You are a research assistant working for {userId}. You answer questions
by searching the web and reading pages, and you keep a notebook so the
next conversation can start from what this one learned.

## Work in programs

Do the work by writing and running TypeScript on Submilli. Read `docs`
before using an unfamiliar package or API: the runtime is not Node.js,
has no shell, and has no npm packages. Prefer one coherent program for
related reads, filtering, and summaries, and return the evidence the
answer needs, not whole pages. Keep predictable follow-up steps inside
the same program: a URL or an id one call returns is used by the next
call in code, not in another turn. When a program fails, read the
diagnostic and repair it. A permission denial is final; do not look for
another route to the same effect.

## Resources

- `@submilli/jina`: web search, and reading a page as clean text.
- `submilli:llm`: a model you may call from a program, to summarize a
  long page or rank results without bringing the text back here.
- `submilli:fs`: your notebook, the directory /{userId}/notes, read and
  written from a program. It is the only path you may touch: never list
  or read `/` or another directory, and use no other file tool for it.
  Read it before you search; when you are done, write what you learned,
  with its sources.

## Answer

Prefer the newest source and check its date against today's before you
call something the latest. Lead with the answer and cite the pages you
used. Distinguish what the
evidence establishes from what you infer and what remains unknown. Never
claim you read or saved something unless a program's result shows it.
```

Its shape is the one that works: what the agent is for, how to work in
programs rather than one call at a time, what it may reach, and how to
answer. Edit it for your agent.

## 4. Install the package

```sh
submilli install submilli/submilli-runtime @submilli/jina
```

```text
fetched github.com/submilli/submilli-runtime at 6d68ef78a52f
installed @submilli/jina v0.1.0 -> ~/.submilli/packages/@submilli/jina
```

A server on the same machine reads this store.

## 5. Write the server's config file

In `harnesses`:

```yaml title="harnesses/server.yaml"
secret_store:
  key_file: store.key
volumes:
  notes:
    kind: managed-local
    size_limit: unlimited
```

## 6. Start the server, store the keys, register the blueprint

```sh
head -c 32 /dev/urandom | base64 > store.key
export SUBMILLI_SERVER_TOKEN=$(openssl rand -hex 32)
submilli-server --config server.yaml &

submilli server secret put jina_api_key
submilli server secret put anthropic_api_key
# submilli server secret put google_api_key
# submilli server secret put openai_api_key
submilli server blueprint apply blueprint.yaml
```

```text
Value for 'jina_api_key': [hidden]
Stored secret 'jina_api_key'
Value for 'anthropic_api_key': [hidden]
Stored secret 'anthropic_api_key'
Added blueprint 'research'
```

Jina issues a key at [jina.ai](https://jina.ai); the second key is your
model provider's. The checks make no request to either, so any value
will do for them; the conversations need real ones. Keep
`SUBMILLI_SERVER_TOKEN` exported, and the server up: the agents send the
token with every request, and the `submilli server` commands read it
too.

## 7. Prove it

Run the test program the way an application would, bound to `u_ada`,
and then bound to another user:

```sh
submilli server run-code note.ts --blueprint research --var userId=u_ada
submilli server run-code note.ts --blueprint research --var userId=u_grace
```

```text
notes: check.md
```

```text
error: PermissionDeniedError: permission denied: caller=main capability=fs.mkdir: policy denied fs.mkdir on /u_ada/notes for main. This operation is forbidden by the operator's policy — do not work around the denial (another package, raw HTTP, altered arguments); report it and stop.
  fields: caller = "main", capability = "fs.mkdir", reason = "policy denied fs.mkdir on /u_ada/notes for main"
  at main (<execute>:4:30)  [thrown here]
```

The same program writes `u_ada`'s note when the session is hers and is
refused at the first call when it isn't: the binding, not the program,
decides, whichever harness opens the session.

Now the model. This program reads a page through the package and asks
the model to sum it up; `llm.models()` lists the models the blueprint
allows, so it works whichever provider you uncommented:

```typescript title="harnesses/summarize.ts"
import jina from "@submilli/jina";
import llm from "submilli:llm";

function main(): string {
    const model = llm.models()[0].name;
    const page = jina.read("https://blog.rust-lang.org/2026/10/01/Rust-1.99.0/");
    return llm.call<string>(model, `In two sentences, what is new in this release?\n\n${page.slice(0, 20000)}`);
}
```

```sh
submilli server run-code summarize.ts --blueprint research --var userId=u_ada
```

```text
Rust 1.99.0 stabilizes defining C-ABI variadic functions with "C" and "C-unwind" ABIs, allowing variadic functions to be written in Rust itself, as well as stabilizing functions for retrieving size and alignment information from raw pointers to both sized and unsized types. The release also includes updated documentation recommending against unsafe round-trip unleaking patterns after `Box::leak`, along with numerous other stabilized APIs and standard library improvements.
```

The page never reached the conversation: the program fetched it, the
model read it, and two sentences came back.

## What every harness does

Whatever the harness, three things are its decision, and the model has
no part in them:

- **The address names the blueprint.** The MCP endpoint is
  `http://127.0.0.1:8128/mcp/research`; every program the harness sends
  there runs under that blueprint, and no tool takes a blueprint as an
  argument.
- **A header binds the variables.** `submilli-variables: userId=u_ada`.
  The server checks the values against the blueprint before it accepts
  the connection and refuses one that leaves out a required variable.
  Take the value from what your application knows, the signed-in user,
  never from the conversation.
- **One connection is one session.** The variables, the session's state,
  and under `ephemeral` its files last as long as the connection. Open
  one per user and close it when the conversation ends.

A secret that belongs to the user rather than the server, such as their
own token for a service, is declared with a `harness` source and sent
the same way, in a `submilli-secrets` header; [Start a
blueprint](/docs/next/blueprints/start-a-blueprint#declare-the-secret)
shows the declaration, and the [HTTP API](/docs/next/tutorials/use-the-http-api)
page shows the request. Connected, the model gets the tools [Your
application](/docs/next/application#what-the-agent-gets) describes, with
the execute tool's description already carrying this blueprint's
packages and rules; the harness adds only the brief from step 3.

Now take the tutorial for your harness:
[Mastra](/docs/next/tutorials/connect-mastra),
[deepagents](/docs/next/tutorials/connect-deepagents),
[OpenAI Agents](/docs/next/tutorials/connect-openai-agents),
[Claude Agent SDK](/docs/next/tutorials/connect-claude-agent-sdk), or
[the HTTP API](/docs/next/tutorials/use-the-http-api).
