# SUB-633 item 12: bound compiler walks and generated work

Research base: `0dde88f341e7f20149ab8d43ae15fbfdce2f1b24`. Tracking: [SUB-633,
item 12](https://linear.app/submilli/issue/SUB-633/no-panic).

## Findings

The parser's 128-entry recursion budget (item 01) bounds recursive grammar, not
tree height. Operator, postfix and call chains are parsed in loops, and later
phases lower flat syntax into nested operations. Every compiler phase after
parsing recurses over these trees and over `Type` values, whose derived
`Clone`/`Eq`/`Drop` are recursive too. Compilation also runs on the caller's
stack: 2 MiB Tokio workers for HTTP/MCP and a main thread for the CLI.

Confirmed at the research base (debug and release CLI, `ulimit -s 2048` for the
server-sized stack; child processes, core dumps disabled):

| Input | Behavior |
| -- | -- |
| `return 1 + 1 + … + 1` with 200 terms (debug) or 500 terms (release) | stack overflow abort in inference |
| `&&`/`??` chains, 10,000 chained calls `f()()…` | stack overflow abort |
| template literal with 2,000 (run) or 5,000 (check) substitutions | abort; inference lowers it to a left-deep concatenation |
| 500 chained aliases `type Ai = { v: A(i-1) }` or `A(i-1)[]` | abort |
| `number[][]…` with 20,000 suffixes | abort; the suffix loop builds a boxed chain |
| 500-class `extends` chain | 36 s, then a WasmGC load error (subtype depth > 63) |
| 100,000 spreads of an optional field | unbounded boxed fallback chain |
| `D<D<…16…>>` with `type D<T> = { a: T; b: T }` | > 60 s: exponential alias instantiation |
| `w(w(…20…))` with `w<T>(x: T): { a: T; b: T }`, nested or flat | > 30 s: exponential substitution |
| 300 statements `const oi = { v: o(i-1) }` | check 0.4 s, run > 60 s: `TypeInfoTable::object_type_id` rescans types |
| 2,000-interface `extends` chain | 9 s (debug) |

## Stage 1 (implemented)

- The interpreter creates no threads. `compiler_limits::COMPILER_STACK_BYTES`
  (128 MiB unoptimized, 16 MiB optimized) documents the stack a thread needs to
  run any compile entry point on a program within the structural limits;
  embedders provide it. The server runs each request's compile, and each package
  install, on a short-lived scoped thread of that size
  (`submilli-server/src/compiler_thread.rs`): spawn failure or a panicking
  compile is an internal failure. The CLI runs its command on one such thread;
  unoptimized builds overflow at the limits on an 8 MiB main thread. Test
  harnesses that compile fixtures size their workers the same way. SUB-1123
  tracks pooling the server's compile threads. Parsing stays on the Tokio
  worker: its recursion is bounded by the parser depth limit.
- `tree_height::check_syntax` rejects source trees taller than 256 levels,
  counting expressions, statements and type annotations, before any recursive
  post-parse pass, including for ASTs handed directly to a phase API. A flat
  operator chain fits 253 operands in a function body; each postfix call adds
  two levels. A chain rooted in `?.` is one node, but code generation nests each
  of its links, so every link counts as a level. `check_typed` bounds typed
  trees at 1,024 levels after inference (including its early return), at the
  entry of each typed phase API, on desugaring's output (loop lowering adds
  nesting) and at codegen entry; `typecheck`/`submilli check` runs capture and
  desugaring so it rejects the same programs compilation does. Both checks walk
  iteratively and treat cycles and out-of-arena children as internal failures. A
  limit is reported from the tallest node over the limit, as a caret at the
  start of the expression on its deepest path that adds the most levels without
  an intervening statement (counting a chain's own links), so it names the chain
  or template to split rather than an enclosing function or a closure inside the
  long expression.
- The `[]` type suffix loop spends the parser recursion budget.
- `resolve_type_inner` bounds nested annotation and alias-body resolution at
  256 levels, which also bounds the depth of alias-inlined types. Each alias
  reference costs a level, as does each type constructor (object, array,
  tuple, union, function, generic type or `readonly`) wrapping the next
  reference, so over
  a primitive base plain renames chain 254 aliases and `{ v: Previous }` bodies
  127. The limit is reported at the annotation that started resolving.
- Inheritance chains are limited to 62 classes, counting the class and every
  ancestor including library classes such as `Error`: WasmGC rejects subtype
  depths above 63, and the root class subtypes the intrinsic object struct.
- At most 512 optional spreads may override one field of an object literal,
  below the typed-tree limit so the specific diagnostic applies. Typed-tree
  height adds each field's fallback links to the height of its terminal
  expression.
- Generic parameter IDs are allocated with a checked increment.

Evidence: subprocess tests for the direct API and CLI on the documented compiler
stack, and for HTTP and MCP handlers on 2 MiB Tokio workers that hand
compilation to the sized thread, each followed by a healthy compile or request;
reducing the server compile thread to 2 MiB, or the CLI command thread to 8 MiB,
makes these tests abort in unoptimized builds; programs exactly at each limit
compile, and one step over fails; near-limit programs match TypeScript
6.0.3/Node 24.14.1; fixtures cover each limit, including a 62-class chain that
loads and runs.

## Stage 2: type size and type work

Bound the size and depth of types produced by alias instantiation and generic
substitution, and the depth of inferred binding and return types. The
exponential cases arise where a substitution or an alias body duplicates its
argument, so a size check after each instantiation stops growth within one
doubling. Non-generic aliases blow up too (`type Ai = { a: A(i-1); b: A(i-1) }`
takes 3 s at 16 aliases and 92 s at 20 in release), so check or memoize at every
alias reference, not only generic ones. Even an identity generic doubles per
level (`type G<T> = T; type Ai = G<A(i-1)>`: 9 GB at 20 aliases), because a
resolved `Type::Alias` stores both its arguments and its substituted body, so
alias types need sharing or a size check even without a duplicating body.
Calibrate the limits against the fixture suite and document them as compiler
limits. Types recorded for codegen must also be bounded, because codegen's shape
lookup is superlinear in type depth.

`TypeParamSubstitution::apply` has 141 callers and is infallible. Prefer checks
at the instantiation sites (alias references, generic calls and methods,
binding declarations) over making every substitution fallible, unless review
finds an unbounded path that bypasses them.

## Stage 3: remaining work bounds

- Dependency declarations: validate the depth of types in supplied
  `PackageDeclaration`s at inference entry. JSON-loaded declarations are
  already bounded by `serde_json`'s recursion limit; in-process ones are not.
- Interface `extends` chains and other superlinear passes within the limits.
- Token and node counts: arenas cap IDs at `u32::MAX`; request-level budgets
  belong to item 38.
- Pre-existing superlinear inference within the limits: switch statements
  with 100,000 cases and thousands of sequential null guards spend their time
  in `Inferer::forget_later_writes` (`typechecker/infer/assign_expr.rs`).

Recursive walkers on `Type` remain protected only by the embedder's compiler
stack until stage 2 bounds type depth. The server's compile threads return only
flat outputs (Wasm bytes, the `TypeInfoTable` and diagnostics); package
declarations are read back from artifact JSON under `serde_json`'s recursion
limit of 128, so a package with deeper types than that installs but fails to
load with a typed error.
