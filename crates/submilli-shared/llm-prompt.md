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
  LLMs default to Node.js / browser idioms; you have to push them off
  explicitly. "Never `async`, `await` or `Promise`" prevents an
  assumption that a list of available modules alone does not address.
- **One worked example.** The example does more than any rule (see
  "What we've learned"). Keep it short and generic (a made-up package),
  and show the habits it fixes: a synchronous `main` with a return type,
  importing a type as well as a function, and an accumulator typed with
  the package's type. Add another only when a class of bugs keeps
  appearing despite the prose.
- **The rule right after the example gets the most attention.** Put the
  rule the models break most first.
- **Naming a forbidden form can prime it.** Measure such wording before
  keeping it: one example primed the form it forbade, another did not.
- **Guidance about an event goes in that event's message.** What to do
  after a denial, a budget refusal, or a failed cast belongs in that
  error, which the model reads at the moment it decides what to do next.
  The prompt carries only what prevents the first mistake. An HTTP status
  is the server's answer, not a Submilli error, so no message of ours can
  carry advice about a 401 or 403: that stays in the HTTP guidance.
- **`{placeholders}` in curly braces** are filled by the server at
  startup from the active policy. See "Server-injected placeholders"
  below for the list.

---

## The prompt

Run a Submilli program: a strict TypeScript subset, compiled to WebAssembly.

Before your first program, call `{t_docs}` with every package and module you will import, and use those declarations rather than guessing APIs. `{t_search}` finds packages, and `{t_builtins_docs}` declares the built-ins.

Programs are synchronous: never `async`, `await` or `Promise`. Every call, a package's included, returns its value directly. Import every function and type you use from its package (`@scope/name`) or standard-library module ({stdlib_modules}). Built-ins need no import: `console`, {builtins} (there is no `Date`).

```ts
import { list_orders, type Order } from "@shop/orders";

function main(): string[] {
    const large: Order[] = [];
    for (let page = 0; ; page++) {
        const orders = list_orders({ page_index: page });
        if (orders.length === 0) break;
        for (const order of orders) {
            if (order.total > 100) large.push(order);
        }
    }
    return large.map((order) => order.id);
}
```

There is no `any`: use the package's types, or `unknown` narrowed with `typeof`, `Array.isArray` or `===`. Every function declaration needs a return type, `main` included: `function main(): string[]`, never `function main()`. `x == null` is a compile error: write `x === null` or `x === undefined`.

Postfix `x!` is a runtime-checked non-null assertion: it
removes `null` and `undefined` from the type and throws `TypeError` for either.
Missing properties, collection misses, and omitted optional parameters use
`undefined`; `null` is distinct. Defaults apply to omission or `undefined`,
never `null`. Optional chains yield `undefined`; `??` handles both values.
`arr[i]` and `obj[key]` read as the element type. Check a missing one where
you read it (`arr[i] ?? d`, `key in obj`, `arr.at(i)`): an unchecked
missing read throws `MissingValueError`. A local `let x: T;` must be assigned on every path before
a read unless `T` includes `undefined`; module-level bindings still need an
initializer in that case. Template literals accept `null` and `undefined` and render them
like `String(x)`.
No runtime reflection: of `Object.prototype` only `.toString()` is
available — no `.hasOwnProperty()`. The type system tells you what
fields a value has; narrow optional values with `obj.field !== undefined`.
`"field" in obj` narrows `unknown` or unions distinguished by field presence;
a present optional field can still hold `undefined`.
Dynamic string keys also work with `key in obj`, without narrowing.
Identifiers follow TypeScript — `type`, `from`,
`of`, `as`, and `is` are contextual and may name variables — with one
exception: `namespace` is reserved here, so rename it (`ns`). Reserved
words are fine as object keys (`{ type: "x" }`, read back as
`obj.type`); for keys that aren't valid identifiers, quote them and
index with a string literal (`{ "content-length": 5 }` →
`obj["content-length"]`). Use `Record<string, V>` or `{ [key: string]: V }`
for dynamic `obj[key]` reads and writes. Reads are `V` and writes require `V`. `Record<"a" | "b", V>` requires
both keys and reads them as `V`. Computed literals (`{ [key]: value }`), spread,
and readonly string index signatures are supported. Keys must be strings;
`keyof` an open string-indexed type is `string`. General mapped types and
unresolved generic Record keys are unsupported.

{mcp_packages}

{tools_package}

{git_package}

{approval_guidance}

{http_guidance}

{sandbox}
{http_access}

{session_guidance}

{embedding_guidance}

{llm_guidance}

Do the work in one program: follow pagination and data dependencies in code, filter and aggregate there, and return only what the answer or your next decision needs.

The output is what `main()` returns: a string as is, anything else as JSON. `console.log` goes to a separate debug stream, and a failed run returns the error with the console output.

---

## Server-injected placeholders

The server fills these from the active policy at startup, before
publishing the tool description over MCP. The LLM only ever sees the
resolved values.

| Placeholder | Resolves to | Source |
|:---|:---|:---|
| `{t_search}`, `{t_docs}`, `{t_builtins_docs}` | registered discovery tool names | MCP or REST caller |
| `{sandbox}` | empty when FS and Code are hidden; otherwise `none` / `ephemeral` / `per_session` / `named`, with limits where applicable, followed by any named volumes mounted below the root | policy `vfs:` block |
| `{http_access}` | empty when HTTP is hidden; otherwise per-method host reachability (`GET → api.example.com; …`), `any host`, or an approval-policy note | policy default and `permissions:` HTTP rules for `main` |
| `{stdlib_modules}` | visible standard-library names | non-deny default or relevant non-deny rules for `main` |
| `{http_guidance}` | HTTP credential guidance, only when HTTP is visible | same visibility rule |
| `{builtins}` | the in-scope built-in types, then "and the namespaces …" | prelude (`interpreter::packages::builtins`) |
| `{mcp_packages}` | empty when no MCP servers; else a note on the available `@mcp/<server>` packages | policy `mcp:` block |
| `{session_guidance}` | session-state guidance only when `main` has potential permission for a session operation | policy default and `permissions:` rules |
| `{llm_guidance}` | model-call guidance only when at least one model is declared and `main` has potential `llm.call` permission | policy `llm.models`, default, and `permissions:` rules |
| `{embedding_guidance}` | embedding guidance only when at least one embedding alias is declared and `main` has potential `embedding.embed` permission | policy `embedding.models`, default, and `permissions:` rules |
| `{tools_package}` | empty unless `main` has potential `tools.call` permission and the caller is REST; otherwise a note on the session's `@tools` package | policy default and `permissions:` rules |
| `{approval_guidance}` | empty unless a rule is `ask-human` or `ask-admin` and the caller is REST; otherwise a note that such calls wait for approval and a refusal is final | `permissions:` rules |
| `{git_package}` | empty unless Git is configured; otherwise a pointer to its package docs | policy `git:` block |

HTTP, FS, and Code are advertised when the default action is not `deny`, or
`main` has a relevant capability rule whose action is not `deny`. An absent
default means `deny`. Filters and rule shadowing do not affect discovery;
an `ask-human` or `ask-admin` rule keeps a library visible, and its calls run
once approved. Code shares
`fs.read`, `fs.write`, `fs.stat`, and `fs.list` with FS. Other callers' grants
do not advertise these libraries to `main`. LLM additionally requires at least
one declared model and uses the same non-deny rule for `llm.call`. A provider
without models is not enough to advertise LLM. Embedding follows the same
rule with `embedding.models` and `embedding.embed`. Session uses the same non-deny
rule for its operations: reading alone, or writing alone, is a usable subset.
Git visibility depends only on the presence of its `git:` configuration.

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

- **Failure mode:** first programs that fail to compile on habits the
  prompt already forbade: `async function main()`, a missing return type
  (`function main() {`), `const all: any[] = []`, and a package function
  used without its import. The rules were in the prompt, but in the
  middle of the 400-word paragraph of language differences.
- **Frequency:** measurable. On AppWorld dev (57 tasks, 2026-11-07), the
  862-word prompt had 80 compile-failed programs per round across Claude
  Haiku 5.5, GPT-6 Luna and DeepSeek V4.1 Flash. Haiku's 47 alone
  included 29 with `async` and 21 with a missing return type.
- **Fix:** the current layout, at about 560 words: the synchronous and
  import rules, one example, the three rules most broken, then the
  paragraph of language differences (the one starting "Postfix `x!`").
  Two runs gave 19 and 19 compile-failed programs, with no `async` and no
  missing return types. Findings along the way:
  - The example matters most. Without it Haiku wrote `async` in 29 of 57
    tasks; with it, no model did.
  - Order matters. With the return-type rule ahead of the `any` rule,
    Haiku's first attempts used `any` in 22 tasks over two runs, against
    8 to 13 with `any` first.
  - Naming a forbidden form can prime it. "Not even `any[]`" tripled
    Haiku's `const all: any[] = []`. "`function main(): string[]`, never
    `function main()`" primed nothing and took missing return types to 0;
    GPT-6.1 Terra had written `function main() {` in 48 programs.
  - Removed with no measured cost: the strategy paragraphs (one sentence
    is left), the Node/NPM sentence, the built-ins docs tool walkthrough,
    and the separate output paragraph.
  - The language-differences paragraph stays. Without it, failures
    roughly doubled (44 and 34), and cutting it down to the `undefined`
    and `Record` rules cost about as much (38).

  The experiment's variants and its classifier are in the private
  AppWorld benchmark (`bench/prompts/execute/`, `bench/errors.py`).

- **Failure mode:** on `permission denied` for a package capability
  (`linear.app/listIssues`), the model rerouted through raw
  `submilli:http` against the same service's API, got a 401 back, and
  was set to keep hunting for a way in. Policy denial treated as an
  obstacle, not a decision.
- **Frequency:** one session log (2026-07-08), but it's the canonical
  agent retry/workaround loop — expect it to recur.
- **Fix:** a workaround-forbidding sentence appended to the runtime
  `permission denied` message itself, so the hint is in-band at the
  moment the model decides what to do next. A "Denials are final"
  paragraph in the prompt repeated it until 2026-11-07; it went, and
  the 401/403 advice moved to the HTTP guidance, which appears only
  when `submilli:http` is visible.

- **Failure mode:** reaching for `secrets.get` to build an API call by
  hand, when the package that needs the credential already resolves it
  internally. A secret value in main-module code can be logged,
  returned as program output, or sent anywhere main can reach, so the
  value must never land there in the first place.
- **Frequency:** structural rather than observed — `secrets.get` was
  documented and importable, and the generated `.d.ts` describes it, so
  a model with a credential-shaped problem would find it.
- **Fix:** the runtime refuses `secrets.get` to `main` outright, ahead
  of the policy, and the denial names the secret-name-to-package shape,
  as does the `secrets.get` declaration. A prompt paragraph saying the
  same went on 2026-11-07: the refusal teaches it at the one moment it
  matters, without costing every session the words.

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
