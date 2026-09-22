---
title: "The standard library"
description: "The built-in globals and the submilli: modules a program can use, with a short example of each."
slug: standard-library
sidebar:
  order: 5
---

A Submilli program has two sources of ready-made functionality: the built-ins
that are always in scope, and the standard-library modules you import by their
`submilli:` name. This chapter shows what is available and what each part is
for. It isn't a reference; the exact declarations come from the `submilli docs`
and `submilli builtins` commands at the end.

## Built-ins

The built-ins are the ECMAScript globals a TypeScript programmer expects,
minus the ones that don't fit Submilli's model:

- **Types:** `Array`, `Map`, `Set`, `String`, `Number`, `BigInt`, `Boolean`,
  `Object`, `RegExp`, `Uint8Array`, `TextEncoder`, `TextDecoder`, and the
  error classes `Error`, `TypeError`, `RangeError`, `SyntaxError`, and
  `PermissionDeniedError`.
- **Namespaces:** `JSON`, `Math`, and `Temporal`.

This program uses most of them to summarize some support tickets:

```typescript title="tickets.ts"
interface Ticket {
  tag: string;
  hours: number;
}

interface Summary {
  tags: string[];
  billingHours: number;
  meanHours: number;
  hasUrgent: boolean;
  due: string;
}

function main(): Summary {
  const tickets = JSON.parse(
    '[{"tag":"billing","hours":3},{"tag":"login","hours":1},{"tag":"billing","hours":2}]'
  ) as Ticket[];

  const hoursByTag = new Map<string, number>();
  for (const ticket of tickets) {
    hoursByTag.set(ticket.tag, (hoursByTag.get(ticket.tag) ?? 0) + ticket.hours);
  }

  const total = tickets.reduce((sum, ticket) => sum + ticket.hours, 0);
  const due = Temporal.PlainDate.from("2026-09-18").add({ days: 3 });

  return {
    tags: Array.from(new Set(tickets.map((ticket) => ticket.tag))),
    billingHours: hoursByTag.get("billing") ?? 0,
    meanHours: Math.round(total / tickets.length),
    hasUrgent: tickets.some((ticket) => /^(billing|outage)$/.test(ticket.tag)),
    due: due.toString(),
  };
}
```

```json
{"billingHours":5,"due":"2026-09-21","hasUrgent":true,"meanHours":2,"tags":["billing","login"]}
```

A `Map` has no JSON form, so returning one from `main` or passing it to
`JSON.stringify` throws a `TypeError` that tells you to convert it to entries or
a plain object first. That is why the result reads `hoursByTag` into a number.

### Dates and times with Temporal

There is no `Date`. Submilli provides `Temporal`, the ECMAScript standard
designed to replace it, and skips `Date` altogether rather than carry both.
`Date` is one mutable type that holds a moment in time while pretending to be
a calendar date in whichever time zone the machine happens to be in, and most
date bugs come from that ambiguity. `Temporal` gives each idea its own
immutable type: `Instant` is an exact moment, `PlainDate`, `PlainTime`, and
`PlainDateTime` are calendar values with no time zone, `ZonedDateTime` ties a
moment to a zone, and `Duration` is a span. Converting between them is explicit,
and arithmetic is a method call:

```typescript title="ticket-age.ts"
function main(): string {
  const opened = Temporal.Instant.from("2026-09-15T08:30:00Z");
  const closed = Temporal.Instant.from("2026-09-18T10:00:00Z");
  const hoursOpen = closed.since(opened).total("hours");

  const local = closed.toZonedDateTimeISO("Asia/Jerusalem");
  const followUp = local.toPlainDate().add({ days: 3 });

  return `open ${hoursOpen} hours, closed ${local.toPlainTime().toString()} local, follow up ${followUp.toString()}`;
}
```

```text
open 73.5 hours, closed 13:00:00 local, follow up 2026-09-21
```

`Temporal.Now` reads the clock: `Temporal.Now.instant()` for the current
moment, `Temporal.Now.plainDateISO()` for today's date. In a template literal,
call `toString()` on a Temporal value explicitly; the compiler asks for it.

## Standard-library modules

Each module is imported by name, as `import * as fs from "submilli:fs"` or
`import { sha256 } from "submilli:crypto"`. Modules that reach outside the
program are capability-gated: every call is checked against the blueprint.

| Module | What it does | Gated by |
| --- | --- | --- |
| `submilli:http` | Outbound HTTP: `get`, `post`, `put`, `patch`, `delete`, `head`, `request`, `download` | `http.*` |
| `submilli:fs` | The program's filesystem: read, write, append, list, stat, move, copy, remove | `fs.*` |
| `submilli:session` | Key-value state that survives from one program to the next in an agent session | `session.*` |
| `submilli:llm` | Call a model from inside the program: `call`, `batch`, `models` | `llm.call` |
| `submilli:url` | Parse and build URLs and query strings | Nothing; pure computation |
| `submilli:crypto` | SHA-256, SHA-512, HMAC, random bytes, timing-safe comparison | Nothing; pure computation |
| `submilli:uuid` | UUID v4 and v7 generation and validation | Nothing; pure computation |
| `submilli:secrets` | Read a blueprint-declared secret by name. Packages only | `secrets.get` |

### HTTP

`http.get` and its siblings return the response directly; there is no `await`.
The `Response` has `status`, `ok`, `headers`, `body`, `json()`, and
`throwForStatus()`:

```typescript
import * as http from "submilli:http";

interface Todo {
  title: string;
  completed: boolean;
}

function main(): number {
  const response = http.get("https://jsonplaceholder.typicode.com/todos?userId=1");
  response.throwForStatus();
  const todos = response.json() as Todo[];
  return todos.filter((todo) => !todo.completed).length;
}
```

### Files, URLs, hashes, and IDs

The filesystem is an area the runtime provides for the program; `/` is its
root, not your machine's. This program combines the four modules that need no
network:

```typescript title="receipt.ts"
import * as fs from "submilli:fs";
import * as url from "submilli:url";
import { sha256 } from "submilli:crypto";
import { v4 } from "submilli:uuid";

function main(): string {
  const parsed = url.parse("https://api.acme.com/v1/charges?customer=cus_northwind");
  const customer = parsed.query.get("customer") ?? "unknown";

  fs.writeText("/receipt.txt", `${v4()} ${customer}`);
  const receipt = fs.readText("/receipt.txt") ?? "";

  return `${parsed.host}${parsed.path}: ${sha256(receipt).toHex().slice(0, 12)}`;
}
```

```text
api.acme.com/v1/charges: d6c5f8584539
```

`readText` returns `null` for a missing file or one larger than the read limit,
so the program has to handle that case. `sha256` returns a `Uint8Array`, and
`toHex()` renders it. The digest differs on each run because the receipt
contains a fresh UUID.

### Session state

An agent often runs several programs in one conversation. `submilli:session`
gives those programs a shared key-value store, read with a type the runtime
checks:

```typescript title="progress.ts"
import * as session from "submilli:session";

interface Progress {
  done: number;
}

function main(): number {
  const previous = session.get<Progress | null>("progress");
  const progress: Progress = { done: (previous?.done ?? 0) + 1 };
  session.set("progress", progress);
  return progress.done;
}
```

The store belongs to the agent's session, so this program returns 1, 2, 3
across calls from the same session.

It also stands in for a REPL. Patterns such as Recursive Language Models keep a
long input as a variable in a live REPL, and the model writes one snippet after
another to slice it, examine a piece, or hand a piece to a sub-model, with every
variable surviving between snippets. Submilli has no REPL: each program runs once
and its memory is gone. Keep the input and the intermediate results in the
session instead, and each program picks up where the last one stopped. The
sub-model half of that pattern is the next module.
[Crafting a blueprint](/docs/blueprints) covers session and filesystem modes.

### Model calls

A program can read an input far larger than any model's context and carry state
between runs. `submilli:llm` lets it hand part of the work to a model as well:
`call` sends one prompt, `batch` sends many at once, and `models` lists the
models the program may use. This program classifies every ticket in a file
without any ticket entering the agent's own context, then asks a stronger model
for a typed verdict on the ones that matter:

```typescript title="triage.ts"
import * as fs from "submilli:fs";
import * as llm from "submilli:llm";

interface Severity {
  level: "critical" | "high" | "low";
  rationale: string;
}

function main(): Severity {
  const tickets = Array.from(fs.lines("/tickets.txt"));
  const answers = llm.batch(
    "claude-haiku-4-5",
    tickets.map((ticket) => `Does this ticket report a billing bug? Answer yes or no.\n\n${ticket}`)
  );

  const billing: string[] = [];
  for (let i = 0; i < answers.length; i++) {
    const text = answers[i].text;
    if (text !== null && text.trim().toLowerCase().startsWith("yes")) {
      billing.push(tickets[i]);
    }
  }

  return llm.call<Severity>(
    "claude-sonnet-5",
    `Rate the overall severity of these billing tickets:\n\n${billing.join("\n---\n")}`
  );
}
```

The thousand prompts and answers stay inside the program; only the `Severity`
reaches the agent. The untyped `batch` returns one `Completion` per prompt, in
order. Check `ok` before trusting `text`: a completion cut off at the output
limit or stopped by a content filter is `ok: false` and still carries the text
it produced, with `reason` saying why. One failed element never fails the batch.

The typed form, `call<Severity>`, sends a JSON Schema for `Severity` with the
request and then checks the response against the type field by field, so the
value it returns has that shape. A response that doesn't match throws a
`TypeError`; nothing is coerced.

Models are declared in the blueprint, and a program can call only those the
blueprint's `llm.call` rule allows, within a token budget that is reserved
before each call is sent:

```yaml title="blueprint.yaml (fragment)"
llm:
  providers:
    anthropic: { type: anthropic, api_key: ${secrets.ANTHROPIC_API_KEY} }
  models:
    claude-haiku-4-5:
      provider: anthropic
      description: "Cheap and fast; use for bulk per-item classification."
    claude-sonnet-5:
      provider: anthropic

permissions:
  main:
  - capability: llm.call
    filter: model glob "claude-*"
    action: allow
```

The provider's key stays in the blueprint's secrets; the program never sees it.
Together, session state and model calls give a program the two things the
Recursive Language Model pattern needs: memory across steps, and sub-models to
delegate a slice of the input to.

### Modules for packages and tests

Three modules are not for ordinary programs. `submilli:secrets` lets a package read a
credential by name; the runtime refuses the call from generated code whatever
the blueprint says. `submilli:security` provides the `check` function packages
call to ask permission. Both appear in
[package anatomy](/docs/package-anatomy). `submilli:test` provides `label` and
`expectException` for test files and is importable only under
`submilli build test`; see
[testing packages](/docs/testing-packages).

## Looking things up

The declarations are the reference, and both readers of this book can fetch
them. For you, the CLI:

```sh
submilli builtins            # every built-in type and namespace
submilli builtins Map        # one built-in's declarations
submilli docs Temporal.Instant
submilli docs submilli:fs    # one module's declarations
submilli search http         # find a module or installed package
```

For the agent, the same lookups are tools on Submilli's MCP endpoint, alongside
the tool that runs code: `packages.search` finds a module or package by name,
description, or exported symbol; `packages.docs` returns one module's
declarations and description; `builtins.list` and `builtins.docs` do the same
for the built-ins. The description of the run tool tells the agent to look a
function up before relying on it, so it never has to know this chapter, or a
package, in advance. Both surfaces read the same source, so what you see with
`submilli docs` is what the agent sees.

Next: [crafting a blueprint](/docs/blueprints), where the capabilities in the
table above become rules.
