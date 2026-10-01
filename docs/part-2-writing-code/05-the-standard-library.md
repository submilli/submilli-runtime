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
| `submilli:git` | VFS repositories: history, staging, commits, branches, and HTTPS clone/fetch/pull; requires a Git identity in the blueprint | `git.*` |
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

`readText` returns `null` for a file larger than the read limit, so the program
has to handle that case; a missing file throws. `sha256` returns a `Uint8Array`, and
`toHex()` renders it. The digest differs on each run because the receipt
contains a fresh UUID.

### Git repositories

`submilli:git` works on ordinary Git repositories under the VFS root. It is
available only when the blueprint configures a Git identity.

```ts
import { Repository } from "submilli:git";
import { writeText } from "submilli:fs";

function main(): string {
  const repo = Repository.init("/notes", { branch: "main" });
  writeText("/notes/summary.md", "Customer issue resolved.\n");
  repo.add(["summary.md"]);
  return repo.commit("Record resolution");
}
```

The [blueprint walkthrough](/docs/blueprints#let-the-program-commit) configures
the identity and Git grants for this example; `writeText` also needs `fs.write`.
The `Repository` class has static `init`, `clone`, and `open` factories.
Use `new Repository("/notes")` or `Repository.open("/notes")` to open an existing
repository. Instances support `instanceof Repository`. JSON preserves field
data, not class identity; reopen by path across executions.

| Repository method | What it does |
| --- | --- |
| `status()` | Reports the branch, staged and unstaged changes, untracked files, and whether the tree is clean |
| `log({ limit: 50, offset: 0 })` | Returns commits and `nextOffset` for pagination |
| `diff({ mode: "working" })` | Returns a patch and binary paths; use `"staged"` or `"refs"` with `from` and `to` for other comparisons |
| `show("HEAD", "summary.md")` | Returns a committed file as `Uint8Array` |
| `branches()` / `remotes()` | Lists local branches or named remotes |
| `add(["summary.md"])` | Stages files or directories, including deletions; `"."` selects the working tree |
| `commit("message")` | Commits staged changes and returns the commit ID |
| `createBranch("topic", "HEAD")` | Creates a branch without overwriting an existing one |
| `switchBranch("topic")` | Switches to a local branch with a clean working tree |
| `addRemote("origin", url)` / `setRemoteUrl("origin", url)` | Adds or updates a named HTTPS remote |
| `fetch("origin", "main")` | Updates a remote-tracking branch |
| `pull("origin", "main")` | Fetches and fast-forwards the current branch; refuses divergence |

Switching and pulling require a clean working tree, including untracked files,
so local work is preserved.
Revision arguments accept `HEAD`, named branches or tags, full refs, and full
commit IDs. Revision expressions such as `HEAD~2` or `HEAD^{/message}` are not
supported; use a commit ID returned by `log()` instead.
The current branch must be a direct local branch. Symbolic branch aliases and
`HEAD` pointing into the tag namespace are refused for branch mutations.

For an HTTPS repository, use `Repository.clone(url, "/repo", { branch: "main" })`.
The destination must be empty. Without `options.branch`, clone follows the
remote default branch. Fetch and pull default to the remote `origin`; an
omitted fetch branch requests all branches, while an omitted pull branch uses
the current branch. HTTPS remotes must support Git's smart HTTP protocol v0 or v1.
For private repositories, use the [blueprint's authentication
setup](/docs/blueprints#let-the-program-commit). The [permission
reference](/docs/permissions#git-capabilities) lists each operation's grants and
explains branch and remote filters.

This version has no push, SSH, merge, force checkout, submodule, or linked
worktree support. Finish or abort any native Git merge, rebase, or other
in-progress operation before changing a repository through this API.
Native indexes must use version 2 or 3, without split indexes, sparse entries,
or intent-to-add entries. Convert a version 4 index with native Git's
`git update-index --index-version=2` before handing it to Submilli.
Native packfiles must be self-contained; partial-clone and cruft-pack metadata
are unsupported. See [Git security](/docs/permissions#git-capabilities) for
metadata protection and handing repositories to native Git, and [resource
limits](/docs/resource-limits#git) for repository sizes and timeouts.

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
[Crafting a blueprint](/docs/blueprints) covers sessions and filesystem modes.

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

The model names are the blueprint's: it declares the providers and models a
program may use, holds the provider's key so the program never sees it, and
gates every call through `llm.call`, within a token budget reserved before
each call is sent ([crafting a blueprint](/docs/blueprints) declares them).
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

## Coding-agent workspace tools

`submilli:code` provides structured reads, search results, directory navigation,
and anchored edits within the configured filesystem. It uses the existing
filesystem capabilities; there are no separate `code.*` grants.

```ts
import { writeText } from "submilli:fs";
import { read, edit, diffText, applyPatch } from "submilli:code";

export function main(): string {
  writeText("example.ts", "const count = 1;\n");
  const first = read("example.ts").lines[0];
  assert(first.line === 1);
  const result = edit("example.ts", "count = 1", "count = 2");
  assert(result.success);
  const patch = diffText("const count = 2;\n", "const count = 3;\n");
  assert(applyPatch("example.ts", patch).success);
  return read("example.ts").lines[0].text;
}
```

| Function | Behavior |
| --- | --- |
| `read(path, offset = 1, limit = 200)` | Returns `{path, lines, truncated}`. Lines contain one-based `line` and `text`; reading beyond EOF is empty. |
| `search(pattern, options?)` | Regex search with `path` (default `/`), `include`/`exclude` glob arrays, `caseSensitive` (true), `context` (0), `limit` (1000), and `mode` (`matches`, `files`, or `counts`). Only the selected result array is populated. Counts are matching lines. |
| `glob(pattern)` | Returns `{entries, truncated}` with files sorted newest first, then by path. Patterns are relative to `/`. |
| `tree(path, depth = 3)` | Returns `{entries, truncated}` sorted by path, with `path`, `kind`, `depth`, and `modifiedAt` (Unix milliseconds). |
| `edit(path, oldString, newString, replaceAll = false, nearLine = 0)` | Replaces a unique anchor, every non-overlapping occurrence, or the uniquely nearest occurrence. Equal-distance hints reject; a hint never substitutes for an anchor. |
| `insertAt(path, line, text)` | Inserts literally before a one-based line; one past the last line appends. Empty files accept line 1. No newline is added. |
| `diffText(a, b)` / `diffFiles(a, b)` | Unified diff with three context lines, comparing text values or file paths respectively. |
| `applyPatch(path, patch)` | Applies a single-file unified diff by unique context, ignoring header positions. Every hunk must succeed. |

Mutations return `{success, changed, diff, diagnostics}`. Each diagnostic contains
`hunk` (one-based for patch rejects, otherwise 0), `line` (0 if unavailable), and
repair guidance in `message`. An ambiguous edit lists occurrences; missing
anchors show a candidate without applying it. A unique whole-line match that
only differs in edge whitespace may apply if its indentation adjustment is
consistent. Other fuzzy matches require a corrected anchor. `replaceAll` and
`nearLine` cannot be combined. More than 1000 ambiguous occurrences raises a
resource error asking for a more specific anchor.

Traversal respects nested `.gitignore` and `.ignore` files, excludes hidden
entries, and never follows discovered symlinks. Ignore files themselves require
read permission; those in directories above the search root are skipped when
reading them is denied, so a grant narrowed to the root still works. Global
host ignore configuration is not consulted. Search
skips NUL-containing binary files and rejects invalid UTF-8. Includes/excludes
are relative to the search root; `*` stays within one directory and `**` crosses
directories. Results default to at most 1000 records;
search accepts a smaller limit and at most 1000 context lines. Traversal refuses
more than 20,000 entries, and inputs/results obey `fs.maxReadSize()` and runtime
memory/work limits. Native workspace work deducts directly from the same fuel
budget as guest execution. Diff comparisons additionally refuse more than four million
old-line/new-line pairs. Narrow the root or compare smaller sections on a limit
error. Check `truncated` before assuming discovery is complete.

Reads use `fs.read`; navigation uses `fs.list` and `fs.stat`; mutations require
both `fs.read` and `fs.write`. Capability contexts use operation names such as
`code.edit`, with path, resulting byte length, and a diff on writes. Denied
access fails the operation. Pure `diffText` needs no permission and preserves
UTF-16 code units, including lone surrogates. Files must be valid UTF-8; edits
preserve bytes outside replaced regions, including BOMs and line endings.

Edits target existing files. A rejected patch writes nothing; a successful edit
uses atomic file replacement. This does not provide isolation from concurrent
external writers. Use `submilli:fs` to create or remove files.

Next: [curated packages](/docs/curated-packages), for maintained clients that
connect programs to services such as GitHub, Slack, and Google Drive.
