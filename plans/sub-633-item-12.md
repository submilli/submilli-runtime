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
  compile is an internal failure. The CLI runs its command on one such thread,
  and `submilli run` sizes the thread that compiles and executes the program to
  at least this; unoptimized builds overflow at the limits on an 8 MiB main
  thread. Test harnesses that compile fixtures size their workers the same way.
  SUB-1123 tracks pooling the server's compile threads. Parsing stays on the
  Tokio worker: its recursion is bounded by the parser depth limit.
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

## Stage 2 (implemented): type size and type work

Confirmed after stage 1 (release CLI, `check` unless noted):

| Input | Before | After |
| -- | -- | -- |
| `type Ai = { a: A(i-1); b: A(i-1) }`, 18 aliases | 23 s | 0.2 s, size limit |
| `type G<T> = T; type Ai = G<A(i-1)>`, 18 aliases | 5 s, growing 4x per 2 levels | 0.1 s, size limit |
| `D<D<…16…>>`, `type D<T> = { a: T; b: T }` | > 60 s | 0.1 s, size limit |
| `w(w(…20…))` or 20 flat `const ri = w(r(i-1))` | 35 s / 58 s | < 1 s, size limit |
| 22 `const oi = { a: o(i-1), b: o(i-1) }`; arrow returns likewise | > 60 s | < 1 s, size limit |
| `interface Ii<T> { a: I(i-1)<{ p: T; q: T }> }`, 16 interfaces | > 60 s | a few s, work limit |
| two families of 16 fan-out interfaces over reversed 1,000-literal unions | > 120 s | 2 s, work limit |
| 3,000 `const oi = { v: o(i-1) }` | > 60 s | 0.7 s, depth limit |

- `type_size.rs` measures types iteratively. `compiler_limits` adds
  `MAX_TYPE_NODES` (65,536) and `MAX_TYPE_DEPTH` (512). The largest type in the
  fixture and package suites has under 100 nodes. Annotation resolution nests at
  most 256 levels, but a `[]` suffix is not one of them, so an alias chain
  `type Ai = A(i-1)[]` adds two levels per alias and is now cut at 254 aliases
  (stage 1 accepted 256).
- Substitution cannot build an oversized type. `TypeParamSubstitution::apply`
  and `substitute_typevars` build through a `TypeBudget` that charges each node
  (and each copied binding by its measured size) before building it, so the
  result stops at the limit instead of after one more doubling. A body that
  mentions its parameter m times with an argument of size S would otherwise
  build m x S nodes, and short-lived substitutions (interface expansion in
  assignability, runtime-test recording, the void-argument scan) doubled
  without ever being stored, so checks at storing sites alone were not enough.
  Recursive-alias expansion (`rehydrate_alias_refs`), cast-target interface
  expansion (`reduce_interfaces_to_shapes`, which inlines interface chains and
  overflowed the stack on 20,000 interfaces) and annotation substitution for
  interface `extends` build under the same bounds.
- Each substitution node and each assignability step also draws from its
  phase's work allowance, `MAX_TYPE_WORK` (2^24; the largest inference in the
  suites spends under 2 million). A per-type bound does not bound how often a
  program rebuilds or compares large types: 12 nested generic interfaces checked
  in 6 s within the size limit.
- `TypeLimits` holds the allowance and the first limit met where the caller
  could not return an error. Callers that can return errors report a `Limit` at
  their source span. Callers that cannot (assignability, class-chain walks, the
  void scan) record the failure and continue with `Type::Error`; type-dependent
  error reports are dropped while a failure is recorded, and checkpoints report
  it at the annotation, expression, statement or declaration that met it (or,
  failing those, at the end of the module or of inference), so compilation
  never succeeds past it. A limit met only while rendering help for a real
  error is discarded, and the help falls back to the plain type.
- Every resolved annotation, alias reference and expression type is checked,
  which bounds composed types such as `{ a: x, b: x }` and inferred depth.
- Declared limitations: an alias instance keeps its arguments beside its
  substituted body, so each layer of generic alias doubles the measured and the
  stored size (five single-use layers around a 2,000-node type exceed the
  limit); removing that duplication needs shared or label-only alias arguments
  and is follow-up work. A function's type includes its parameter types, so a
  function with several near-limit parameters cannot be used as a value.

## Stage 3 (implemented): remaining work bounds

- Dependency declarations: `PackageDeclaration::check_type_limits` walks every
  type a declaration holds, without recursion and destructuring each structure
  exhaustively, and inference entry rejects an oversized direct or transitive
  dependency with a `Limit`.
- Inline runtime type checks: codegen inlines an interface's structural test
  with its members' tests, so interfaces whose members repeat a nested interface
  emitted exponential code (16 levels built a 700 MB module; 20 levels
  timed out). `MAX_INLINE_VALIDATOR_STEPS` (32,768 per check, counting each
  test and each failure-path segment it emits, long field names more) bounds
  the work of emitting each check, naming it; many small checks in a module are
  not limited by it. Every test also records the path to the value it tests, so
  a chain of interfaces emitted code in proportion to tests times depth (a
  500-level chain built a 2 GB module, and one wide chain panicked in the Wasm
  encoder on a body over 4 GiB). Emission also stops, with a located error,
  before a function would have more locals or instructions than the engine
  accepts (`MAX_FUNCTION_LOCALS`, 50,000; `MAX_FUNCTION_BODY_BYTES`, with the
  body measured by encoding its instructions as they are checked), which it
  previously reported without source context (for example three checks of a
  large webhook interface in one function). When the check itself crosses a
  limit, the error names the type the program wrote and points at the cast;
  when the code before it did, the error is the function's, at that code.
  Every built function is also measured exactly (encoded bytes and locals)
  before the module is published, so code that passes the engine's limits
  outside any check fails with a located error too, at the statement that
  crossed the limit (desugared code reports the statement it came from).
  Every compiler-limit error returned by the compile and typecheck entry
  points, from any stage, is cut to the first line of its span, since limits
  are met in large programs whose statements and literals can run to thousands
  of lines. The cast's span is now mapped while its check is emitted, so a
  failed cast reports the cast's start rather than the last part of its value;
  code after the cast maps as before.
- Checks emitted as functions of their own (recursive validators, generic type
  descriptors for calls such as `id<I>(v)`, instance field guards) report their
  limits without a location: those functions map no source, and the error
  names the checked type instead. They previously failed with the engine's raw
  error; locating them at the requesting call is left to item 38.
- A cast to a chain of single-field objects nested 200 deep, which compiled in
  18 s, now reaches the step limit (180 deep: 0.5 s, was 12 s).
- Runtime-check inference for an interface tried a `Map` carrier for every
  pair of candidate types found in its fields, so a field typed as a union of
  a few hundred literals made it cubic (250 literals: seconds; with the work
  limit, a rejection). Collection carriers are now decided without trying each
  instantiation when member names rule them out (a required member missing, or
  a weak target sharing none) or when none of the members the target names
  mentions the carrier's type parameters (one instantiation answers for all).
  An index signature of `unknown` does not prevent the shortcut. A target
  naming a `Map` member that uses `K` or `V` (`get`, `iterator`) still tries
  every pair, and with a union of more than about 170 literals reaches the work
  limit (the old compiler took 2.6 to 18 seconds). An `Iterable<U>` target still
  tries every `Map` pair, because assignability
  ignores iterator element types (a separate, pre-existing unsoundness that
  makes every `Map` an `Iterable` of anything); with a union `U` of more than
  about 170 literals that now reaches the work limit, where the old compiler
  took 10 to 23 seconds.
- Superlinear passes within the limits:
  - validator discovery's interface reachability kept a per-path visiting set
    and re-explored shared interfaces (26 fan-out levels: > 60 s); it now keeps
    every visited interface;
  - codegen's dependency collection and validator discovery cloned every
    subtree of every expression type (500 nested bindings: 34 s); both skip
    subtrees already collected, the latter only for reference-free objects,
    whose walk has no per-root state;
  - object `TypeInfo` lookup scanned every entry per type; codegen now looks
    collected types up by key (`TypeInfoIndex`);
  - a switch rescanned every earlier case body in `forget_later_writes`, which
    now skips comparisons with a constant operand (nothing is evaluated after
    the tested read), and joined case exits left to right; exits are now joined
    in a balanced tree (100,000 cases: 1.6 s, was > 60 s at 30,000).
- Residual, for item 38 (request-level budgets on declaration and statement
  counts and on memory):
  - a stored copy of a near-limit type costs up to about 1 KiB per node, so
    many references to one (or hundreds of nested bindings at the depth limit)
    use gigabytes;
  - 500 nested bindings at the depth limit take about 4 s to compile in release;
  - interface `extends` chains copy every inherited member, and the
    closure-arity scan walks every interface reachable from each annotation,
    so thousands of chained interfaces are quadratic (5,000: about 3.5 s;
    20,000: about a minute before the depth limit);
  - runtime checks are inlined per occurrence, with about one local per field
    and union literal, so a few checks of a large API type in one function (a
    page of orders whose addresses use a 250-literal country union) exceed the
    engine's per-function locals, now with a located error; emitting each
    named type's check as a shared validator function would remove that.

Evidence: fixtures for each limit and one just below the size limit (also run
under TypeScript 6.0.3 strict and Node 24.14.1); end-to-end tests that pin
where each limit is reported (alias reference, generic call, inferred value,
returned value, overriding member) and cover the work limit and dependency
declarations through script and package compilation; unit tests for exact
limits, copy charging, the shared allowance, recorded failures, the declaration
walk and the `TypeInfo` index; a 120-level conditional whose branches have types
at the depth limit compiles on the documented compiler stack in debug and
release.

Package declarations read from artifact JSON are also bounded by
`serde_json`'s recursion limit of 128, so a package with deeper types than that
installs but fails to load with a typed error.
