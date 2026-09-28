# Submilli LLM prompt

The canonical text we use to teach an LLM about Submilli in one shot.

This is what ships as the `description` field of the
`submilli__typescript__execute` MCP tool. The `submilli-shared` crate embeds this file at
build time (`include_str!`), extracts the `## The prompt` section, and fills
the placeholders below per bound blueprint. The server serves the result as
the tool description; `submilli blueprint prompt [--blueprint <path>]` prints
the same resolved text for debugging.

We keep it as a separate document so it
can evolve quickly as we observe how LLMs fail and adjust the wording in
response. Compile-error rate on LLM-generated `.subm` is the success
metric — improving this prompt is a primary lever.

## Editing principles

- **Terse over thorough.** Every word here is loaded on every tool
  inspection. Aim for the minimum that gets the LLM to write code that
  compiles on the first try.
- **Negative space first.** State what's NOT available before what is.
  LLMs default to Node.js / NPM / browser idioms; you have to push them
  off explicitly. "No `undefined`" and "no NPM" are load-bearing in a
  way that "use `submilli:fs` for file I/O" isn't.
- **No worked examples by default.** Examples are easy to add and hard
  to remove without breaking something downstream. If a class of bugs
  keeps appearing despite the prose, *then* add a one-liner example
  targeted at it. Otherwise lean on the type system + `packages.docs`.
- **`{placeholders}` in curly braces** are filled by the server at
  startup from the active policy. See "Server-injected placeholders"
  below for the list.

---

## The prompt

Run a submilli program — strict TypeScript subset compiled to
WebAssembly.

Prefer one coherent program over a sequence of one-call executions.
Batch related API calls, follow data dependencies and pagination in
code, and reuse fetched data. An ID or path returned by one call can
feed the next call in the same program. Return to the model when
results require judgment, not merely to pass data between operations.

Preserve the model's context window: execution output becomes part of
the conversation and is read again on later turns. Filter, search,
join, and aggregate inside the program; return only relevant fields,
counts, and excerpts needed for the answer or next decision. Include
source identifiers and material errors or gaps, and state when results
are partial. Avoid dumping whole files or collections and repeatedly
fetching a file just to return different slices. Batch focused work,
not an exhaustive investigation; stop once there is enough evidence
to fulfill the request.

You do NOT have access to Node.js APIs, browser globals, or NPM
packages. Submilli ships its own standard library — modules are:
{stdlib_modules}. Submilli native
packages and discovered `@mcp/<server>` packages may also be available;
use the `{t_search}` and `{t_docs}` tools to discover them before importing.

Built-ins already in scope, no `import` needed: {builtins}. Their
signatures are a strict subset and differ from Node/TS in places (no
`Date` — use the `Temporal` global, ISO 8601 + IANA timezones; `Math`
includes `Math.random()`; `JSON` is `stringify` / `parse`). Call the
`{t_builtins_docs}` tool for each name you'll use to get their exact
`.d.ts` declarations before relying on a method. It takes a dotted
member path too (`Temporal.Instant`), which returns just that member
instead of the whole namespace. Asking it for a package name returns
the `{t_docs}` call to make, and asking `{t_docs}` for a
built-in serves it — the two tools cross-check each other, so a name
in the wrong one is never a dead end.

{http_guidance}

Denials are final. A `permission denied … capability=…` error means
that operation is forbidden — for you and for every other route to the
same effect. Do not retry it, call the same service through
a different package, or change arguments to slip
past the filter. Stop and report the denial. Most denials are the
operator's policy; a few are runtime rules no policy can change, and
the message says which. Likewise an HTTP 401/403 or "authentication
required" response means credentials are not configured for that host:
do not hunt for tokens or build auth headers — stop and report it.

You never handle secret values. `submilli:secrets` is not available to
you: calling `secrets.get` is refused whatever the policy says. When a
package API needs a credential, pass it the secret **name** and the
package resolves the value internally and never returns it — e.g.
`notion.createPage(secretName, …)`, not `get("NOTION_TOKEN")`. There is
no route around this: not another package, not asking a package to
fetch the value and hand it back, not raw HTTP.

Language deltas: no `undefined`, no `async`/`await`, no `Symbol` /
`Proxy`, no `any` (`unknown` requires narrowing), no `Date` (use the
`Temporal` global), `==` aliases `===`. Truthiness and `&&` / `||` /
`??` use JS/TS truthiness and short-circuit semantics (an `unknown` condition
must be narrowed first).
Postfix `x!` is a runtime-checked non-null assertion: it
narrows `T | null` to `T` and throws `TypeError` if the value is `null`.
Return types are mandatory on function declarations (including `main`) and
class methods; arrow functions can infer them.
No runtime reflection: of `Object.prototype` only `.toString()` is
available — no `.hasOwnProperty()`. The type system tells you what
fields a value has; narrow optional values with `obj.field !== null`.
`"field" in obj` narrows `unknown` or unions distinguished by field presence.
Dynamic string keys also work with `key in obj`, without narrowing.
Identifiers follow TypeScript — `type`, `from`,
`of`, `as`, and `is` are contextual and may name variables — with one
exception: `namespace` is reserved here, so rename it (`ns`). Reserved
words are fine as object keys (`{ type: "x" }`, read back as
`obj.type`); for keys that aren't valid identifiers, quote them and
index with a string literal (`{ "content-length": 5 }` →
`obj["content-length"]`). Use `Record<string, V>` or `{ [key: string]: V }`
for dynamic `obj[key]` reads and writes. Missing reads return `null`, so their
read type is `V | null`; writes require `V`. `Record<"a" | "b", V>` requires
both keys and reads them as `V`. Computed literals (`{ [key]: value }`), spread,
and readonly string index signatures are supported. Keys must be strings;
`keyof` an open string-indexed type is `string`. General mapped types and
unresolved generic Record keys are unsupported.
Every program needs `function main()`; its return
value is the output, delivered as a string: a `string` is emitted
verbatim (so don't `JSON.stringify` it yourself — that double-encodes),
`number`/`boolean` use `toString`, and objects/arrays serialize to
JSON. `console.log` is a separate debug stream.

Reading JSON text: `JSON.parse(s)`
returns `unknown`. Validate and type the result with a runtime cast:
`const x = JSON.parse(s) as T`. Do not write `JSON.parse<T>(s)` or
rely on `const x: T = JSON.parse(s)`.
`T` can be a primitive (`string`, `number`, `boolean`), an object type,
an array, or a data-only `interface` you declare (no methods) — and
combinations like `Issue[]`.{mcp_packages}{git_package}

{sandbox}
{http_access}

Session state (`submilli:session`): a key-value store scoped to this
session — `set(key, value)` writes, `get<T>(key)` reads it back checked
against `T` (throws a catchable `TypeError` on a shape mismatch),
`has`/`remove`/`list` round it out. There is no `set<T>`; the value's
type is inferred. It is **memory-only**: it does not survive a server
restart, so a key you wrote on an earlier call may legitimately be
missing. Read a key that may be absent as `get<T | null>(key)` and
handle the `null`.

{llm_guidance}

Output: success returns just `main()`'s value as a string (a `string`
return verbatim; numbers/booleans via `toString`; objects/arrays as
JSON). Failure returns result + console + error details inline.
`console.log` output is dropped from a *successful* result — read it
back with the last-run tool.

---

## Server-injected placeholders

The server fills these from the active policy at startup, before
publishing the tool description over MCP. The LLM only ever sees the
resolved values.

| Placeholder | Resolves to | Source |
|:---|:---|:---|
| `{t_search}`, `{t_docs}`, `{t_builtins_docs}` | registered discovery tool names | MCP or REST caller |
| `{sandbox}` | empty when FS and Code are hidden; otherwise `none` / `ephemeral` / `per_session` / `persistent`, with limits where applicable | policy `vfs:` block |
| `{http_access}` | empty when HTTP is hidden; otherwise per-method host reachability (`GET → api.example.com; …`), `any host`, or an approval-policy note | policy default and `permissions:` HTTP rules for `main` |
| `{stdlib_modules}` | visible standard-library names | non-deny default or relevant non-deny rules for `main` |
| `{http_guidance}` | HTTP credential guidance, only when HTTP is visible | same visibility rule |
| `{builtins}` | comma-separated catalog of in-scope built-in types + namespaces | prelude (`interpreter::packages::builtins`) |
| `{mcp_packages}` | empty when no MCP servers; else a note on the available `@mcp/<server>` packages | policy `mcp:` block |
| `{llm_guidance}` | model-call guidance only when at least one model is declared and `main` has potential `llm.call` permission | policy `llm.models`, default, and `permissions:` rules |
| `{git_package}` | empty unless Git is configured; otherwise a pointer to its package docs | policy `git:` block |

HTTP, FS, and Code are advertised when the default action is not `deny`, or
`main` has a relevant capability rule whose action is not `deny`. An absent
default means `deny`. Filters and rule shadowing do not affect discovery;
`ask-human` keeps a library visible without authorizing execution. Code shares
`fs.read`, `fs.write`, `fs.stat`, and `fs.list` with FS. Other callers' grants
do not advertise these libraries to `main`. LLM additionally requires at least
one declared model and uses the same non-deny rule for `llm.call`. A provider
without models is not enough to advertise LLM. Git visibility depends only on
the presence of its `git:` configuration.

Add new placeholders here when the resolved value is policy-dependent
and the LLM needs it during planning. Keep the list short — most
configuration should be hidden from the LLM, not surfaced to it.

## What we've learned

A running list of observed LLM failure modes and the prompt change (if
any) made in response. Be sparing — only add when the same failure
keeps appearing despite the existing prose. Record:

- **Failure mode** — what the LLM does wrong.
- **Frequency** — how often we see it (anecdote vs. measurable).
- **Fix** — the wording change, if any. (Sometimes the fix is in
  another tool's description, in `packages.docs` output, or in the
  language itself, not here.)

- **Failure mode:** on `permission denied` for a package capability
  (`linear.app/listIssues`), the model rerouted through raw
  `submilli:http` against the same service's API, got a 401 back, and
  was set to keep hunting for a way in. Policy denial treated as an
  obstacle, not a decision.
- **Frequency:** one session log (2026-07-08), but it's the canonical
  agent retry/workaround loop — expect it to recur.
- **Fix:** "Denials are final" paragraph in the prompt (also covers
  401/403 responses), plus a workaround-forbidding sentence appended
  to the runtime `permission denied` message itself, so the hint is
  in-band at the moment the model decides what to do next.

- **Failure mode:** reaching for `secrets.get` to build an API call by
  hand, when the package that needs the credential already resolves it
  internally. A secret value in main-module code can be logged,
  returned as program output, or sent anywhere main can reach, so the
  value must never land there in the first place.
- **Frequency:** structural rather than observed — `secrets.get` was
  documented and importable, and the generated `.d.ts` describes it, so
  a model with a credential-shaped problem would find it.
- **Fix:** the runtime refuses `secrets.get` to `main` outright, ahead
  of the policy, and the denial names the secret-name-to-package shape.
  This paragraph exists because the prompt is always in context while
  the generated types require the model to go looking, so it is the
  cheapest place to stop the call being written at all.

- **Failure mode:** `import { Temporal } from "temporal"` despite "no
  `import` needed" in this prompt, and truthiness tests
  (`if (message.internalDate)`) despite the no-coercion delta. The
  training prior (ecosystem TS imports Temporal; truthiness is
  idiomatic JS) beats prompt text.
- **Fix:** in the language, not the prompt — prelude-namespace imports
  are now forgiven as no-ops, and the language adopted full JS
  truthiness with value-returning `&&`/`||` (SUB-706/SUB-707). The
  "no truthy/falsy coercion" delta paragraph was deleted; fighting the
  prior with prose had already failed.

### Workspace editing

Use `submilli:code` for numbered reads, regex search, glob/tree discovery, and
anchor-based edits. Results are structured: `read(path).lines` carries `line`
and `text`; search returns `matches`, `files`, or `counts` according to `mode`.
Check `truncated` before treating a result as complete. `edit` uses exact text,
with `nearLine` only disambiguating repeated anchors. Check mutation `success`
and `diagnostics`; rejected patches leave the file unchanged. `insertAt` is the
only line-addressed write. Use `diffText(a,b)` for text or `diffFiles(a,b)` for
paths; `applyPatch(path,patch)` locates unified hunks by context, not header
positions. These tools use existing `fs.read`, `fs.list`, `fs.stat`, and
`fs.write` grants, and never expose host paths.
