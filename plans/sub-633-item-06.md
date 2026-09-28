# SUB-633 item 06: safe arenas, spans and source access

Research base: `3ca8e039e9148baf3285fd617dac1a5edf2de9c2`.
Tracking: [SUB-633, item 06](https://linear.app/submilli/issue/SUB-633/no-panic).

## Findings and scope

`Ast` and `TypedAst` index vectors directly and narrow allocation indices after
debug assertions. `Span` asserts ordered ranges and same-file merges.
`LineIndex` narrows lengths and can slice source text unrelated to its index.
`Sources` narrows file IDs without excluding reserved IDs. Inference also slices
source using unchecked metadata. `Type::union` expects a singleton to exist.

A heuristic scan excluding test files and content after the first test module
found 604 arena-access calls in 53 files and 228 allocation calls in 20 files.
These are migration estimates, not confirmed bug counts.

Item 06 necessarily changes callers in files covered by later checklist items.
Those edits cover arena/source failures and directly affected cleanup. Unrelated
symbol, closure and runtime invariants remain separate work. Compiler recursion,
recursive destruction and global allocation budgets remain items 12/37/38.

## Stage 1: checked operations and error mapping

Add checked arena reads, mutable access, allocation and ID iteration alongside
the existing APIs. Shared storage errors identify arena, operation, ID and size;
callers attach the compiler stage. Invalid IDs map to `CompilerFailure::Internal`,
capacity/reservation failures to `CompilerFailure::Limit`. Error reporting must
not dereference a failed node or trust its span. Check ID capacity and reserve
storage before inserting a node. Supply small-limit tests without huge allocations.

This is an additive API foundation: existing compiler callers retain their
current behavior until stages 3–4. It does not complete item 06 or make existing
execution paths panic-free. Do not replace legacy APIs with panic wrappers around
the new APIs. Source/span checks are implemented with their callers in stage 2.

## Stage 2: spans, sources and structural type normalization

Check span construction/merging, source byte lengths, file-ID capacity, source
bounds and UTF-8 boundaries. Bind line-text access to its owning source. Preserve
generated zero-length spans and reserved stdlib IDs. Update inference, diagnostics,
backtrace and debug-info callers. Invalid metadata is fatal; rendering retains the
original failure without fabricating a source location. Make singleton union
normalization structurally safe without making all type operations fallible.

Implementation: `SourceError` distinguishes invalid metadata from source/file
capacity and reservation limits. `LineIndex` owns its text; `Sources` reserves
IDs below the virtual-file range. Checked span/line APIs propagate through CLI,
HTTP/MCP preparation, diagnostic rendering, and codegen. Source-less compiler
failures use a dedicated virtual file instead of pointing at script line 1.

Type annotations retain their parsed names rather than slicing source during
inference. An iterative metadata validator checks all AST nodes and annotations
before inference, including callers of direct phase APIs. Package DWARF resolves
each span against its owning source and checks writer path/address preconditions.
Reserved definitions retain virtual paths without claiming source coordinates.

Boundary coverage includes reversed/cross-file/UTF-8 spans, invalid line/column
positions, unknown and reserved files, capacity classification, corrupt AST
metadata, diagnostic preservation, and decoded multi-file DWARF. These tests
inject internal-state failures; they do not establish guest exploit reachability
or whole-compiler panic freedom. Arena migrations remain stages 3 and 4.

## Stage 3: untyped arena and consumers

Migrate parser allocation/access, pattern lowering and inference reads to checked
APIs. Preserve ordinary recovery and accumulated diagnostics; arena failures stop
the phase. Propagate through script, package and direct phase APIs. Discard failed
transformations before another phase can consume them. Use ID iterators instead
of reconstructing IDs with narrowing casts. Remove legacy untyped accessors once
all callers have migrated.

## Stage 4: typed arena and remaining propagation

Migrate inference writes, checking, capture, desugaring, capability analysis,
codegen analysis and emitters, including indirect accessors such as `source_type`
and `is_effect_free`. Propagate errors explicitly, without placeholder nodes/types
or continued processing after failure. Remove legacy typed APIs after migration.
Failed compilation must not return Wasm or package artifacts.

## Verification and completion

Each stage gets focused tests and the repository's independent clean-code,
correctness and edge-case review loop. Cover invalid expression/statement IDs,
failed mutation/allocation without partial insertion, capacity boundaries using
small limits, and healthy operations after failure. Later stages add reversed and
cross-file spans, unknown files, Unicode boundaries, mismatched indexes, reserved
IDs and end-to-end script/package/direct-API propagation tests.

Compiler implementation changes require formatting, workspace clippy, full
workspace tests and package checks from `CLAUDE.md`. Set
`SUBMILLI_SKIP_HTTP_TESTS=1`; this work does not change HTTP transport. TypeScript
language semantics are unchanged by stage 1, so a TypeScript/Node differential
oracle is not applicable to that stage.

Keep item 06 open until all four stages and their propagation are verified.
