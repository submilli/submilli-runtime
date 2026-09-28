# SUB-633 item 02: reject unsupported closure arity

Tracking: [SUB-633, item 02](https://linear.app/submilli/issue/SUB-633/no-panic).

## Research basis and prerequisite

Updated after inspecting item 06 in `/Users/somdoron/git/submilli-wt1` at
`6faec525a43c424220f51e6a1afa435cd9668a73`. That checkout was clean and contained
all four implementation stages: `0c17037`, `e5ef467`, `5b12729`, and `6faec52`.
This is source inspection, not an independent verification of item 06 or a claim
that those commits have shipped. This checkout was at
`c049ac3041adb5ec04ada88122779a2683cbab27` when the plan was written.

Integration prerequisite completed for stage 1: a fresh upstream fetch resolved
to `6faec525a43c424220f51e6a1afa435cd9668a73`, and this branch rebased onto it
without conflicts, preserving this plan. This is the stage 1 review base.
Future stages should check for subsequent item 06 fixes before continuing.

## What item 06 already supplies

- Checked parsed/typed arena operations and source/span validation.
- Explicit failure propagation through inference, capture, desugaring,
  capability analysis, codegen analysis and expression/statement emission.
- `CodegenAnalysis::collect`, `emit_expr`, `emit_function`,
  `emit_closure_function`, and function-adapter `emit_bodies` return
  `Result<_, CompilerFailure>`.
- Script/package boundaries preserve prior diagnostics and reject failed
  artifacts. Source-less failures use `FileId::COMPILER`.
- Worker-stack regressions for the enlarged compiler call paths.

Reuse those contracts. The earlier proposal to introduce broad emitter Result
propagation is substantially superseded. Item 06 does not make every helper
fallible and does not resolve closure or symbol invariants.

## Remaining failure and scope

`codegen/closures.rs::ClosureSig::of` still narrows `usize` to `u8` with
`expect`; `classify` still panics for a non-function type. Closure collection
and `walk_type` remain infallible, as do analysis helpers such as `visit_type`
and `note_method_call`, and class `slot_closure_sig`.

`SymbolTable::value_type`, `slot_value_type`, `slot_wasm_result`, and
`host_value_type` still return plain values. Function-type lowering calls
closure classification. This is the principal remaining propagation expansion;
the prior research counted 127 `.value_type(` calls in codegen, not 127 distinct
functions or confirmed defects. Recount against the rebased tree.

Earlier probes using the existing debug CLI on the pre-item-06 base found that
arrow closures, direct named functions and named-function adapters all run at
255 parameters, while 256 passes `check` and panics during `run`. Repeat with a
fresh rebased build; these results have not been rerun on the item 06 snapshot.

Complete item 02 without claiming completion of items 13/15/18–19. Fix directly
affected closure/type-lowering invariants and error paths; keep unrelated
symbol/layout, arena, recursion, allocation and runtime work separately tracked.
Repository-wide lint enforcement remains item 41, not part of this patch.

## Stage 1: lock down the arity contract and regression cases

Retain the existing closure ABI and its maximum represented arity of 255.
Define the limit once in a compiler module shared by inference and codegen;
avoid making inference depend on codegen internals. Do not widen the integer
just to move the failure threshold.

Count the parameter slots represented by `ClosureSig`, not raw call-site
argument expressions. Confirm the following against the actual lowering paths:

- Optional/default parameters remain declared parameter slots.
- A packed rest parameter occupies one slot; a call with more than 255 source
  arguments is not automatically an unsupported closure signature.
- Closure environments and method receivers must follow their existing ABI
  placement; they must not accidentally reduce the supported user boundary.
- Generic descriptors and generated adapters need separate ABI accounting.
- Named functions, function annotations, class/interface methods, inherited
  members and imported signatures must receive consistent treatment wherever
  they require this representation. Do not introduce an unrelated constructor
  or raw host-function limit without establishing that ABI requirement.

Generate compact test sources for 254/255/256 parameters rather than maintaining
large repetitive fixtures. Supported cases must execute and consume the final
argument, to catch truncation and slot-offset errors. Include void and value
returns, defaults, packed rest parameters, generic instantiation, method binding
and cross-package calls. Reuse existing closure arity fixtures where appropriate.

### Stage 1 implementation and evidence

`crates/interpreter/src/compiler_limits.rs` supplies `MAX_CLOSURE_ARITY` and
`checked_closure_arity`, returning the encoded `u8` or `UnsupportedClosureArity`
with the actual count. The error has no invented source location; stages 2–3
will attach the caller's stage and span. Unit tests cover 0/1/254/255 and reject
256/257/`usize::MAX`, followed by a successful checked conversion.

This is an additive contract, not compiler enforcement: no production caller
uses the new conversion yet. The existing `ClosureSig::of` panic and missing
source diagnostic remain the explicitly scheduled stages 2–3. Do not mark
item 02 complete or claim that oversized programs now fail safely.

`crates/interpreter/tests/closure_arity.rs` generates and runs 21 programs:
254/255-parameter arrows, named functions, adapters, void closures, captures and
interface methods; 255-slot defaulted declarations and a default adapter;
254 fixed parameters plus a rest array receiving three values (257 source
arguments); generic closure descriptors; lexical and explicit receivers;
argument side effects/evaluation order; imported function/closure signatures
and a method inherited from a separately compiled package. Tests consume the
last argument and assert results. The explicit TypeScript `this` parameter is
excluded from the 255 slots, as are the closure environment and captured
generic descriptors. Class receiver adapters are exercised through interface
dispatch and inherited methods.

Source evidence: `closures.rs::closure_func_type` adds the environment before
the declared slots; `function_emitter/mod.rs::emit_closure_function` loads
captured runtime generic descriptors from that environment; class
`slot_closure_sig` classifies `MethodSlot::param_tys` independently of `this`.
Rest/default normalization occurs before the emitted call uses those slots.

Focused unit and integration tests passed on the rebased tree. The exact 21
programs also passed strict TypeScript 6.0.3 and Node v24.14.1 with matching
numeric results, including the void closure's mutation and the argument-order
counter. No successful-case probe throws. Oracle invocation:

```sh
SUBMILLI_SKIP_HTTP_TESTS=1 SUBMILLI_ARITY_ORACLE_DIR=/tmp/sub633-stage1-oracle-final \
  cargo test -p interpreter --test closure_arity --offline
tsc --ignoreConfig --strict --target ES2022 --module commonjs \
  --outDir /tmp/sub633-stage1-oracle-final/js /tmp/sub633-stage1-oracle-final/*.ts
```

Run each generated executable `.js` with Node (the `arity.js` file is only a
library). The export helper appends `console.log(main())` and `export {}` to
isolate each script; the package test changes only its import specifier from
`arity` to `./arity`. The Submilli harness observes the same main return value.

Existing language limitations found while choosing equivalent probes: default
values on arrows, optional function-parameter syntax, and casts from `unknown`
to generic `T` are rejected. Named defaulted declarations, a generic non-null
assertion, and explicit receiver syntax provide supported coverage instead.
Inferring a function value loses default omission information; the default
adapter test follows the existing fixture pattern of casting through `unknown`
to a smaller callable type. These are pre-existing compatibility limitations;
stage 1 changes no TypeScript acceptance or execution semantics.

A fresh debug CLI build on the rebased tree reproduced the 256-parameter arrow
case in a subprocess with a 30-second timeout: `check` exits 0, `run` exits 101
at `closures.rs:56`. A separate healthy CLI invocation returns 42. This remains
covered by SUB-633 item 02 and its closure inventory entry. The permanent
oversize test in this stage checks the new contract directly; source-level
diagnostic regressions must be added when stages 2–3 integrate it. No release
oversize or same-server healthy-follow-up verification is claimed here.

## Stage 2: checked classification and internal propagation

Make `ClosureSig::of` and `classify` return typed failures. Unsupported arity is
`CompilerFailure::Limit`; a non-function passed to classification or a missing
required registration is `CompilerFailure::Internal`. Attach a real source span
where available; preserve a source-less failure when it is not.

Propagate through these remaining routes using the Result APIs from item 06:

1. Closure type walking, local and dependency collectors, inherited methods,
   analysis type/signature helpers, and top-level signature registration.
2. Class method-slot signatures, closure calls/casts/coercions, function
   adapters, and generated dispatch signatures.
3. Symbol value-type lowering and its slot/host/result wrappers, followed by
   their callers in environment/box/subtype/class construction and emission.
   Extend still-infallible emitter helpers only where required by this chain.

Use explicit `?` propagation. Collect fallible iterators into a Result before
emission; never filter out errors. Check directly affected lookup, indexing,
parameter-slot and narrowing assumptions while changing these mechanisms.
Preserve the ordering and producer/consumer agreement of Wasm type registration.

Do not manufacture a placeholder `ValType`, clamp an arity, omit required output,
or emit a guest trap to hide a compiler error. The existing failure latch is not
a substitute for a valid return value from type lowering. Stop on failure and
discard local module construction; no script or package artifact may escape.

This stage can be split into compiling prerequisite commits, but item 02 remains
open until both internal checks and source diagnostics are complete.

## Stage 3: source diagnostics and imported signature validation

Use the shared limit during signature checking so `check` rejects unsupported
programs before codegen. Cover `infer/signatures.rs::resolve_params`, arrow
inference, function-type annotations in `resolve_type.rs`, class/interface
signatures, and relevant generic/import paths. Arrows and function annotations
do not all pass through `resolve_params`; one guard there is insufficient.

Report the actual arity and maximum, highlight the relevant parameter list or
declaration, and suggest grouping arguments into an object or a supported rest
parameter. Avoid duplicate reports when the same signature is visited repeatedly.
Use existing diagnostic aggregation and safe span APIs.

Validate nested function types and instantiated/imported callable signatures,
including inherited dependency methods and adapters created only in consumers.
Use dependency declaration locations when available, otherwise the importing
use site plus package/symbol context. Do not attribute dependency metadata to
an arbitrary script line. Preserve existing dependency usage policy; explicitly
test how unused unsupported declarations are handled instead of accidentally
rejecting an entire unused package surface.

The checked internal conversion remains mandatory even after source validation.
Public phase APIs and malformed external declarations can bypass normal checks.

## Stage 4: verification and completion

Focused tests must cover:

- The supported boundary and one above it for closures, named functions,
  adapters, methods, function types and imported/inherited signatures.
- Defaults, rest packing, generic signatures and both return conventions,
  with no new cap on raw argument count where packing makes it valid.
- Direct codegen/helper calls with excessive arity, non-function classifier
  input and missing closure registration; check typed failure categories.
- Script and package compilation, retaining earlier diagnostics and returning
  no artifact on failure; a healthy compilation after a failed one.
- CLI `check` and `run`, direct library APIs, and in-process HTTP/MCP handlers:
  a compile diagnostic instead of panic/task-join output, followed by a healthy
  request. Use subprocess isolation for panic/abort regression probes.
- Existing debug/release worker-stack regressions on 2 MiB and 8 MiB stacks;
  retain item 06's focused dispatcher structure when adding Result propagation.

Compare supported programs with strict TypeScript/Node for argument and return
semantics. Treat rejection above the Submilli limit as an intentional compiler
limit, not a claim that TypeScript rejects the same program.

Run focused checks while iterating, then the required independent clean-code,
correctness and edge-case review loop on the final implementation, fixing all
in-scope findings. Follow `CLAUDE.md` for final verification:

```sh
cargo fmt --all --check
SUBMILLI_SKIP_HTTP_TESTS=1 cargo clippy --workspace --all-targets -- -D warnings
SUBMILLI_SKIP_HTTP_TESTS=1 SUBMILLI_FULL_TEST=1 cargo test --workspace
SUBMILLI_SKIP_HTTP_TESTS=1 cargo run -p submilli -- build test
```

Full tests are required because compiler behavior changes. Keep in-process
HTTP/MCP tests enabled; this scope does not require live HTTP/socket tests.
Report skipped coverage separately. Use the open-pr skill if creating a PR.

Close only item 02 after its complete boundary, propagation, diagnostic and
verification conditions hold. Record contributions to other inventory entries
without marking their broader work complete.

## Revised investment

Item 06 removes much of the duplicated phase/emitter migration, so this is now
a focused closure and type-lowering change on top of existing propagation.
It is still a multi-day task: symbol lowering and signature coverage remain
substantial. Replace the earlier one-to-two-week estimate with a provisional
three-to-seven engineering days including tests and reviews, and reassess after
stage 2 exposes the remaining infallible callers. This is a planning estimate,
not a measured implementation cost or delivery promise.
