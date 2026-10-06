# SUB-633: invariant policy and simplification inventory

Historical execution ledger. The 2026-10-06 reset replaces the active SUB-633
backlog with [the current panic inventory](sub-633-panic-inventory.md).
The completed decisions and dated evidence below remain preserved.

Completed inventory decisions 2026-10-05. R01–R26 and P01–P09 have final
dispositions; T01–T06 record tracking/workflow synchronization. R25/item 32 is
complete within the user-confirmed panic-only scope; the retained SUB-633
resource/dependency/gate backlog is still open. This ledger records selective simplification, not whole-commit reverts.
The policy is [AGENTS.md](../AGENTS.md#no-panic-execution-paths).

The useful rollback is selective: remove error propagation that exists only for
documented invariants. Keep input validation, resource bounds, operational errors,
and fixes to actual compiler behavior. Several changes already express invariants
more clearly without panicking; permission to panic is no reason to undo them.

## Sources and review boundary

- [SUB-633](https://linear.app/submilli/issue/SUB-633/no-panic), including its
  completion notes and comments, and the
  [original source-site inventory](https://linear.app/submilli/document/sub-633-complete-baseline-source-site-inventory-4b6b6b5a42c5).
- Original source baseline: `25717faee899650ebbf81c1e4310547594be42e2`;
  original engine baseline: submilli-wasm 0.1.4. Preserve these historical records.
- Initial checkout inspected: `16e10725a08aabd5937bb64abf6f5a3c9b49fff1` on
  `codex/bounded-diagnostics`; engine locked at 0.1.9. The policy and initial
  inventory were committed as `46893ac3`; individual execution commits follow.
- Cached local main: `f9c1608b0a39d4a22a3fccd4ae624812d573398b`;
  cached upstream/main at the end of inspection:
  `dc1e61f47d909d5cbfdc540c8bcd5373cf60599a`. No fetch or rebase was performed.
  Recheck current main before implementing a row; upstream moved during review.
- Reviewed the relevant hardening commits and current implementations, grouped
  below. This is a contract/family inventory, not a fresh proof of every one of
  the original 1,613 first-party indexes or every transitive dependency.

Historical counts (837 explicit first-party candidates in 108 files, 1,613
first-party indexes, 227 engine explicit candidates and 318 engine indexes) are
search inventories, not counts of present defects or work to undo.

## How to maintain this inventory

Stable IDs R01–R26 identify code decisions; P01–P09 identify current explicit
sites; T01–T06 identify tracking changes. Keep IDs when splitting work, using
suffixes such as R04a. Keep original SUB-633 item numbers too.

The original analysis used these dispositions:

- **Simplify:** a concrete local guarantee supports removing invariant-only
  checks. Verify all callers and document the guarantee in the implementing diff.
- **Prove first:** there is a plausible guarantee, but a public boundary,
  serialization contract, cross-phase dependency, or mutation needs proof.
- **Keep:** real failures or useful structural improvements remain.
- **Already simplified:** the historical reversal has already happened.
- **Deferred:** intentionally not reopened by this review.

The execution entries now supersede those original recommendations. For future
changes, append owner/commit or PR, exact symbols, proof, remaining error cases,
focused checks and final disposition. A row is
finished only when its code and tracking disposition agree. An accepted invariant
stays recorded; it does not count as a removed panic. Do not delete history or
reinterpret a checked historical item as a promise of zero panics.

## Execution progress

Owner: Codex in this checkout. Process: one item, focused verification, independent
clean-code/correctness/edge review, commit, then the next item. Full PR verification
remains deferred to the repository's post-rebase gate. Item 32/R25 is complete for
panic review by the user's subsequent instruction; this does not close SUB-633's
separate resource/dependency backlog.

| Task | Status | Decision and evidence |
| --- | --- | --- |
| Policy and inventory baseline | Committed `46893ac3` | Three independent reviews, no findings; local links, commit references, 42-item coverage and diff checks passed |
| R01 | Complete: retained | Keep compiler fatal-error architecture; evidence below; leaf simplifications remain assigned to their individual rows |
| R02 | Complete: selectively simplified | Nine accepted local dispatch invariants; real validation/failure contracts retained; evidence below |
| R03 | Complete: retained | Keep checked public arena/span/source contracts; no API redesign |
| R04 | Complete: simplified | Removed invariant-only namespace resolution/field Result interfaces |
| R05 | Complete: retained | Preserve capture/narrowing behavior and existing error propagation |
| R06 | Complete: retained | Keep ordered substitution and numeric filter dispatch structure |
| R07 | Complete: retained | Keep cross-phase registration checks and fallible emission |
| R08 | Complete: selectively simplified | Removed root-scope-only failure checks; kept slot/mark/limit contracts |
| R09 | Complete: selectively simplified | Removed duplicate decimal validation and private parse-error Result |
| R10 | Complete: simplified | Metadata and typed_metadata return Option directly |
| R11 | Complete: retained | Keep bounded rendering and DWARF writer/source errors |
| R12 | Complete: simplified | 24 same-builder type lookup expectations; build failures remain fallible |
| R13 | Complete: simplified | Removed four private numeric engine-error wrappers; range errors preserved |
| R14 | Complete: retained | Keep shared raw-slice ABI helpers and boundary validation |
| R15 | Complete: simplified | Removed HMAC-init and fixed-digest-only error layers |
| R16 | Complete: selectively simplified | Accepted first path component; retained pack and borrowing checks |
| R17 | Complete: retained | Keep worker/client construction errors and ownership |
| R18 | Complete: already simplified | Poison-only reversion already implemented; preserve real backend errors |
| R19 | Complete: retained | Keep direct wire maps and retry classification loop |
| R20 | Complete: retained | Keep bounded unordered collection and positional sort |
| R21 | Complete: simplified | Remove compiled schema asset error chain |
| R22 | Complete: retained | Keep iterative filesystem traversal; explicit sites handled individually |
| R23 | Complete: retained | Preserve parser, closure arity and compiler limits |
| R24 | Complete: retained | Keep real boundary errors; narrow item 31 |
| R25 | Complete: panic-only scope | No outstanding unaccepted panic identified; broader lifecycle work outside scope |
| R26 | Complete: retained as backlog | Keep resource/dependency backlog with proof-based scope |

### R01 execution evidence

`ArenaError::into_compiler_failure` maps capacity and allocation failures to Limit
and invalid public IDs to Internal. `SourceError::into_compiler_failure` similarly
preserves source/file limits and invalid metadata. These are used by the public
compile/typecheck APIs, not only private unreachable branches. `front_end_with_transitive`
also rejects a source different from the parsed script's source. Capture, desugaring and codegen
propagate failures before `CompiledScript` construction, retaining prior diagnostics.
`CompileError::into_diagnostics` is an explicit compatibility adapter, not redundant
internal-only plumbing. Removing these contracts would lose real failure handling.

Disposition: retain R01's shared architecture unchanged. No code/test changes or
new runtime claims; inspected conversion arms and public call sites above, with
existing compiler tests covering fatal limits and mismatched source. Individual leaf
decisions are recorded under R02–R16. Documentation diff/link checks are sufficient
for this retention decision; no runtime tests were rerun. Three independent review
roles reported no findings. The commit containing this entry records completion.

### R02 execution evidence

`lex_newline`, `lex_operator` and `lex_delimiter` now document immediate
`next_token_inner` dispatch and use invariant panics for mismatched/missing bytes.
Parser string/number literal type extraction, accepted atom conversion and
TemplateHead extraction likewise use the token just matched: `advance` clones
`peek` before moving the cursor; `parse_atom` is the sole template helper caller.
Nine explicit sites are accepted, in addition to the original baseline inventory.

Retained the lexer fatal latch, source/Unicode checks, template depth overflow,
public token validation and parser limits. Other private parser checks remain:
removing their errors does not eliminate the still-needed Option/fatal machinery,
and nonlocal dispatch/state assumptions are not proven by this local review.
Only the test fragment demanding recovery from a deliberately invalid private
operator dispatch was removed; Unicode and depth failure tests remain.

Three independent review roles reported no findings. `cargo fmt --all --check`,
offline workspace/all-target Clippy with `-D warnings`, and focused interpreter
library tests passed: `lexer::tests` 134 and `parser::tests` 391. Tests used
`SUBMILLI_SKIP_HTTP_TESTS=1 SUBMILLI_FULL_TEST=0`; no transport behavior changed.
Graphify AST update completed with existing unsupported/partial-parse warnings.
Accepted invariant evidence was appended to the linked SUB-633 source-site ledger;
historical checkboxes remain unchanged. The commit containing this entry records
the implementation; the documentation-only R01 commit is `c1af5940`.

### R03 execution evidence

`ExprId`/`StmtId` expose their u32 payload; the expression/statement vectors are
private, but callers can fabricate IDs or pass IDs from a different AST. Public
metadata and mutable node access can also introduce invalid references.
`try_expr`, `try_stmt` and mutable counterparts accept IDs without an owner token.
`arena::get`/`get_mut` therefore perform real public-boundary validation. Allocation
and ID-width limits remain independent errors. Source APIs accept supplied spans,
file IDs, offsets and positions and validate identity, bounds and UTF-8 boundaries.
The checked-arena and checked-source integration tests exercise these contracts.

Disposition: retain the existing checked APIs. Private accesses may be redundant
after local construction, but introducing separate trusted-access APIs would add
surface area without removing these public contracts. Leave their simple existing
Result paths in place, as the policy permits. This closes R03's simplification
decision without claiming a general implicit-index audit. Documentation-only;
existing source/tests inspected, no runtime tests rerun. Three independent reviews
reported no findings; diff checks passed. Completion is recorded by this commit.

### R04 execution evidence

Both namespace dispatchers in `expr.rs` check root membership immediately before
entry. Call dispatch checks a nonempty chain; field dispatch appends its member.
Resolution only borrows state; the NotFound root lookup precedes any mutation or
callback. Those four expectations now state their guarantees.
`resolve_namespace_chain` returns ChainResolution directly and namespace field
access returns its typed result directly. Call inference and chain extraction
remain fallible for real inference/allocation/arena failures. Unknown members
still produce the same diagnostics. Removed only the private-state recovery test.

Three independent reviewers reported no findings. Formatting, offline workspace
Clippy with all targets and `-D warnings`, 17 namespace library tests and focused
fixture runs (27 namespace, 6 math) passed, with full tests disabled and HTTP
skipped. Graphify AST update completed with the existing extraction limitations.
Recorded and verified accepted invariants in SUB-633's source-site ledger. R02's
implementation commit is `a50130c4`; this entry's commit records R04 completion.

### R05 execution evidence

Retain the capture/narrowing changes. `capture` accepts a public TypedAst, and its
walkers perform checked node access as well as state bookkeeping; they cannot
become infallible by replacing stack pops. Closure processing recursively walks
the body before re-reading its mutable node and saved frame, so this is not the
immediate immutable dispatch proof used in R02/R04. Narrowing tracks parallel
assignment/tombstone frames and suspended executable-body state. Its callers also
perform genuinely fallible inference/materialization. Existing propagation is
compact and already needed; introducing trusted variants would add complexity.

`infer_body_with_narrowing_boundary` and `suspend_narrow_scopes` isolate body exit
facts and pending materializations; retain that semantic behavior. History
`6086f295` explicitly confirms that `0dde88f3` fixed the top-level block closure
panic by walking module statements inside a frame. No whole or partial semantic
reversion is justified. This is a decision to keep the current simple fallible
paths, not a claim that every private pop is input-triggerable. Source/history
review only; no runtime changes/tests rerun. All three independent review roles
reported no findings; diff checks passed. This commit records the disposition.

### R06 execution evidence

The exact-object unifier checks equal lengths and ordered keys before zipping
BTreeMap values; corresponding values therefore share keys. Its real mismatch
and recursive unification errors remain necessary. `Comparison::eval` dispatches
each numeric operator directly to `eval_numeric` with its comparison; that helper
handles nonnumeric/missing values as the established non-match semantics. Neither
mechanism now adds an invariant-only error interface. Restoring the old lookup or
nested unreachable arm would make the code less direct. Retain both changes;
this is a resolved no-revert decision. Source inspection only, no code/tests changed.
Three independent reviewers reported no findings; diff checks passed.

### R07 execution evidence

Retain registration/layout propagation. Public codegen entry points consume
TypedAst and dependency declarations; tree-height validation does not certify all
symbol/layout registrations. SymbolTable is built incrementally and exposes
optional lookup results. Class collection can fail on unavailable parents and
layout ordering; closure emission also handles captured-field lowering and arity
limits. Recursive-validator discovery/expansion can fail on type-size limits.
These phases cannot drop Result merely because a builtin registration is normally
present. A producer's completeness must cover public/imported metadata as well.

The leaf `ok_or_else(internal_failure)` checks are compact and use an already
required error channel. Retain them rather than expanding this task into a
validated-context API redesign or selectively restoring assertions without a
clearer interface. Actual cast failures and compiler failures remain distinct.
This closes the reversion decision for this family; it does not certify every
registration as an invariant or close SUB-633's implicit audit. Reviewed current
symbol/class/closure/recursive-validator contracts and public codegen entry;
documentation-only, no tests rerun. Three independent reviews had no findings;
diff checks passed.

### R08 execution evidence

FunctionEmitter construction seeds a root; the only production scope removal,
`pop_scope`, rejects its removal. Documented that invariant at the private field,
removed `require_scope`, and replaced three repeated binding/shadow/source scope
error branches with descriptive expectations. Existing Result interfaces remain
for local-type/AST validation. Kept evaluation mark validation: callers pass a raw
usize, and nested registration lifetimes are distinct from root-scope existence.
Kept local limits, root-pop errors and parameter validation. Removed only the
test segment fabricating an empty private scope vector.

Three independent reviews had no findings. Formatting, offline workspace/all-target
Clippy and 27 focused emitter library tests passed with full tests disabled and
HTTP skipped. AST graph update retained existing extraction limitations. Accepted
sites recorded in the SUB-633 source ledger; implementation recorded by this commit.

### R09 execution evidence

`intern_digits` is the sole production caller of `decimal_to_limbs`, after
nonempty ASCII-decimal validation. Locked num-bigint 0.4.6's FromStr delegates to
radix-10 parsing, whose only returned errors are empty/invalid digits. The private
helper now returns Vec directly with a documented parse expectation. Removed its
duplicate scan; kept zero representation and public malformed digits/index/width
checks. String-pool public mutable tables retain their existing checked accesses.

Three independent reviews had no findings. Four focused BigInt-pool tests,
formatting, offline workspace/all-target Clippy and diff checks passed; full tests
disabled and HTTP skipped. Graph updated with existing extraction limitations;
accepted site recorded and verified in SUB-633's ledger. This commit records R09.

### R10 execution evidence

Audited every DefaultValue/EnumVariantValue variant, string-backed MangledName and
the artifact_f64 serializer: the complete graph is JSON-compatible, and non-finite
numbers become strings. No arbitrary serializer or map key is reachable. Metadata
helpers now return Option directly with a documented serialization expectation;
six caller sites drop only metadata error propagation. Actual wrapper emission,
registration, size and other compiler errors remain fallible.

Three independent reviews had no findings. Formatting, offline workspace/all-target
Clippy and filtered fixtures (39 default, 21 rest, including non-finite defaults)
passed with full tests disabled and HTTP skipped. AST graph updated with existing
limitations. Accepted site recorded/verified in SUB-633; this commit records R10.

### R11 execution evidence

`rendering::Writer` is not an unrestricted String writer: byte/step/depth budgets,
fallible reservations and a latched error govern rendering. Truncation is consumed
as presentation control flow; real allocation/source/metadata failures still return.
DWARF generation validates source files and addresses, then propagates gimli's
write/section errors. Changing these paths to expect would discard real contracts.
Keep the shared error channels and existing compact formatting propagation; no
separate invariant-only public interface was identified to remove. R11 is resolved
as retention, not a statement that formatting can never panic. Inspected rendering,
diagnostics and DWARF code; documentation-only, no tests rerun. Three independent
reviewers reported no findings; diff checks passed.

### R12 execution evidence

Checked every declaration/definition/lookup in singleton, intrinsic, error-subtype
and Git-class builders against locked engine 0.1.9's RecGroup contract. Successful
build preserves IDs/kinds; getters return None for a different kind. All 24 lookups
use the same builder's handle and matching kind with no intervening mutation.
Documented expectations replace only lookup error branches. Build, invalid layout,
supertype, prelude/setup and other ABI failures retain fatal error propagation.

Three independent reviews had no findings. Formatting, offline workspace/all-target
Clippy, singleton failure/recovery, intrinsic/codegen compatibility and error-field
tests passed. Required Git memory-limit suite passed two tests; optional calibration
remained ignored. Full tests stayed disabled; only the required focused Git check
enabled nightly bodies, using in-process transport. Graph updated with existing
limitations. Accepted sites recorded/verified in SUB-633; this commit records R12.

### R13 execution evidence

Removed four private `*_js_checked` wrappers and their engine-error layer. Public
formatters retain Result<String, String> for range rejections; legacy host adapters
map those to ordinary errors and prelude adapters retain RangeError classification.
Other host/string/ABI failures remain on their existing paths. Accepted finite
exponential formatting/exponent parsing and finite BigInt conversion invariants.
For integer radix 2–36, even `next_down(1)` times radix lies below the rounding
midpoint to radix; subtracting the truncated part preserves [0,1), proving digit
conversion cannot fail. Fuel formulas and timing are unchanged and catalogued.

Removed two obsolete private formatter-trap injection tests; retained the generic
fatal-host classification test. Added extreme-float/radix boundary coverage.
Three independent roles reviewed both implementation and test delta, no findings.
Initial compilation caught the obsolete injection signatures; after their removal,
20 existing numeric unit tests, the new boundary test, three formatting fixtures,
generic fatal-wrapper test, formatting and workspace/all-target Clippy passed.
Full tests disabled, HTTP skipped; AST graph updated with existing limitations.
Accepted sites recorded/verified in SUB-633; this commit records R13 completion.

### R14 execution evidence

Retain `check_host_abi`, `abi_arg` and `abi_result`. The wrappers validate buffer
shape against a registered FuncType, but a helper accepts an arbitrary slice and
index and does not carry that signature. Shape validation alone does not prove a
callback's chosen index belongs to that signature. The shared helpers span over a
thousand source occurrences; changing their contract would require proving every
registration/body pair, or adding a new validated-signature access API. Neither
removes host Results needed for guest values, memory, callbacks and operations.

Individual fixed accesses can qualify, but retaining these compact existing
helpers is explicitly allowed by policy and avoids a large low-value mechanical
change. Keep their fatal trap classification and current boundary regression tests.
This completes R14 as a no-revert decision, not a claim that fixed slots are all
fallible or that an ABI bug was found. Source review only; no code/tests changed.
Three independent reviews reported no findings; diff checks passed.

### R15 execution evidence

HMAC's documented any-key-length contract supports an infallible private
constructor; removed `from_initial` and its artificial error layer. SHA-256 output
is converted to a 32-byte array; only 8/16-byte truncation callers exist. `tag` and
truncation now return arrays directly. Prefix sizing, entropy initialization,
malformed cursor checks, authentication, UTF-16 and allocation failures remain.
Deterministic encoding vectors are unchanged.

Replaced impossible HMAC fault injection with real entropy and checked
size/reservation failures. Compilation found two stale test references, corrected
before a second complete three-role review; no findings remained. All 18 cursor
tests passed, including wire vectors, guest-catch classification and healthy
follow-up. Formatting and workspace/all-target Clippy passed; graph updated with
existing limitations. Full tests disabled and HTTP skipped. Accepted invariants
recorded/verified in SUB-633; this commit records R15.

### R16 execution evidence

`validate_metadata_path` now documents and uses str::split's guaranteed first
component; existing path validation and subsequent security checks are unchanged.
Pack widths are already structural fixed arrays with checked read lengths; retain
untrusted header/offset validation. Pending-worktree and reference-cache borrowing
spans filesystem/resource work and use compact existing errors; retain them rather
than asserting a broad no-reentry contract. No wholesale Git reversion.

Three independent reviews had no findings. Ten storage tests and two required Git
memory tests passed; optional calibration ignored. Formatting and workspace Clippy
passed, full tests disabled, HTTP skipped; focused Git nightly bodies enabled as
required. Graph updated with existing limitations. Accepted site recorded/verified
in SUB-633; this commit records R16.

### R17 execution evidence

BlockingWork acquires a Tokio runtime through `try_current`, reserves failure
storage, spawns an OS thread and handles worker/channel outcomes. Those operations
can fail independently of invariants. Its ownership and draining protect resources
used by work after caller cancellation. HTTP client builders likewise return real
configuration/TLS/runtime setup errors. Retain these contracts and containment;
allowing a worker invariant panic does not justify rethrowing it or abandoning its
resources. Existing tests explicitly cover absent runtime, thread-spawn failure,
worker panic and subsequent healthy work. Source/history reviewed; no code or test
changes, no runtime checks rerun. R17 is resolved as retention; three independent
reviews had no findings and diff checks passed.

### R18 execution evidence

Confirmed `91c2ec7c` already removes poison-only APIs (19 files, +199/-698).
Current blueprint/session/idempotency store lock access uses documented poison
expectations. Shared store traits still return StoreError because disk/backend
implementations perform actual reads, serialization and atomic writes; an in-memory
implementation returning Ok does not make the trait error redundant. Keep the
existing reversal and backend contracts. Original 40 poisoned-lock accesses remain
accepted under P01; initiating panics are assessed separately. No further poison
reversion identified in this review. Documentation-only, no tests rerun. Three
independent reviewers reported no findings; diff checks passed.

### R19 execution evidence

Anthropic, Google and OpenAI wire builders construct Map values directly, avoiding
both a Value downcast and any error layer. Retain this structure. Provider failure
classification consumes Retry wrappers in a loop and then classifies the actual
failure; restoring an unreachable Retry arm adds no value. Keep real client-build
and remote-provider errors. Existing retry-wrapper tests describe the intended
classification; no network call or behavior change is involved. Source reviewed,
documentation-only, no tests rerun. R19 is resolved as retention. Three
independent reviewers reported no findings; diff checks passed.

### R20 execution evidence

The batch dispatcher creates one indexed future per prompt, collects with
`buffer_unordered` under the configured concurrency bound, and sorts completed
outcomes by index. This is compact and has no invariant-only error API. Restoring
semaphores and optional result slots adds machinery without a measured benefit.
Retain the current implementation. Existing tests cover varied completion order,
concurrency bounds, empty/single batches and cancellation of active dispatches.
This decision changes no dispatch, accounting or cancellation behavior.
Documentation-only source review; no tests rerun. Three independent reviewers
reported no findings; diff checks passed.

### R21 execution evidence

The sole production pack constructor consumes `include_str!("schemas/github.json")`.
Maintained tests parse that exact compiled asset, check tool coverage, validate every
schema's representability and compare recorded server responses. Documented expects
now enforce its JSON/tools-object invariants; a broken binary asset may panic.
Removed SchemaPackError, cached Result, DiscoveryError::SchemaPack and CLI injection
wrappers. Pack lookup returns Option; eager initialization remains at startup and
discovery. Server failure injection is test-only, retaining downstream fatal-error,
cache, idempotency and healthy-follow-up coverage without production setup plumbing.
Real client construction, remote discovery, allocation and filesystem errors retain
their existing propagation. Removed only corrupt-asset recovery tests. Shared MCP
focused checks: 73 passed, four HTTP tests ignored. Four server setup tests passed.
CLI run checks: nine passed initially; two failed because the sandbox blocked the
default local secret store, then passed with an isolated SUBMILLI_HOME. Formatting,
workspace Clippy and diff checks passed. Three independent reviews found no issues.
Graph updated; accepted-invariant entry appended to the source ledger and verified.
HTTP tests were skipped because transport behavior is unchanged.

### R22 execution evidence

`collect_source_files` maintains an explicit directory stack and canonical ancestor
set. Filesystem enumeration, metadata and canonicalization can fail; symlinks can
form ancestor cycles. Retain these checks and the iterative traversal. No fallible
layer exists solely for a proven invariant here. Serialization/path expects remain
separate P02–P06 decisions below; R22 does not pre-approve them. Existing source
cycle/path tests were inspected, not rerun. Documentation-only retention. Three
independent reviewers found no issues; diff checks passed.

### R23 execution evidence

Retain parser recursion guards and closure-arity/type/work budgets. Source text can
supply arbitrarily nested syntax and signatures; these limits are part of accepting
external input, not guaranteed private construction. The historical parser and
closure-arity fixes include boundary/process tests, and current parser entry points
retain depth accounting. The relaxed invariant policy does not justify undoing
these fixes or their error propagation. This resolves the reversion decision only;
aggregate resource work remains in item 38. Documentation-only, no tests rerun.
Three independent reviews found no issues; diff checks passed.

### R24 execution evidence

Keep boundary failures for source/import preparation, discovery, stores, diagnostics
and worker cleanup. Current runner preparation converts source/metadata failures
into caller errors; recording and diagnostics also have actual allocation/I/O
failure paths. Their outer representation as strings or anyhow errors is not a
panic or sufficient reason for another typed-error redesign. R21 removed only the
asset-only chain encountered in this review. Item 31 remains an audit of concrete
boundary failures, not a mandate to turn every internal invariant into a Result.
This is a retention/scope decision, not a claim that all boundaries are panic-free.
Documentation-only; no tests rerun. Three independent reviews found no issues;
diff checks passed.

### R25 execution evidence

Current disposition (user-directed scope correction, 2026-10-05): item 32 is
checked in SUB-633. No outstanding unaccepted panic has been identified here.
General cancellation, shutdown cleanup and idempotency correctness are outside
this panic-only completion criterion; no comprehensive lifecycle audit is claimed.
This supersedes the earlier R25 deferral and references to it in T03/T06 below.
The following paragraph preserves the original deferral history.

Preserve the user's explicit deferral of item 32. `GracefulShutdownTracker::watch`
spawns owned work; dropping its waiter does not abort the spawned request. Its
separate stop token can cancel work during forced shutdown. This distinction is
visible in implementation and the waiter-cancellation test. No new unaccepted panic
has been demonstrated here, and this review neither redesigns ownership nor marks
item 32 complete. The inventory disposition is recorded; the underlying audit stays
deferred. Documentation-only, no tests rerun. Three independent reviewers found
no issues; diff checks passed.

### R26 execution evidence

Retain items 37–40 as focused follow-up audits. `type_size::measure` pushes children
onto its pending vector before their subsequent budget checks; an iterative walk
alone is not an aggregate allocation bound. Resource prerequisites SUB-1108/SUB-1123
remain relevant. Cargo currently resolves submilli-wasm 0.1.9; the ledger's 0.1.4
reference is a historical baseline. Engine expectations require validator/executor
proof from the separate engine review; this task makes no engine change or claim
of completion. The previously recorded Git SSH callback finding is an integration
check for the branch containing it, not a reproduced current-checkout defect.
This resolves what to retain, not the underlying budget/dependency audits.
Documentation-only; no tests rerun. Three independent reviews found no issues;
diff checks passed.

## Historical candidates and original rationale

The original candidate analysis below is retained for provenance. Its “prove first”
and “candidate” wording is not outstanding work: the execution entries above give
each R ID's final simplification, retention or deferred decision.

### R01 — Compiler fatal-error architecture: keep; prune leaves

Items 03, 06, 12, 22. History: `3ca8e039`, `0c170376`, `e5ef4670`,
`5b127296`, `6faec525`, `8d1eb415`, `b699da07`.
[Compiler errors](../crates/interpreter/src/compiler_error.rs),
[arena](../crates/interpreter/src/arena.rs),
[source](../crates/interpreter/src/source.rs).

`CompilerFailure::Limit`, allocation failures and malformed public metadata still
need propagation through compilation. Keep `CompileError`, fatal-vs-diagnostic
separation and fallible compiler entry points. Remove individual internal-only
branches and private `Result` signatures after proving their contracts; do not
remove `Internal` globally while remaining callers use it. No partial artifacts.

### R02 — Parser/lexer dispatch: simplify local cases

Item 05; `3ca8e039`.
[Lexer](../crates/interpreter/src/lexer.rs), [parser](../crates/interpreter/src/parser.rs).

`next_token_inner` dispatches newline bytes directly to `lex_newline` without an
intervening mutation. Its other-byte fatal branch can be a documented invariant.
Likewise inspect parser helpers entered immediately after matching a literal or
template token, and first/last accesses on vectors just populated locally.
Document cursor movement and rollback before accepting each parser site; this is
not blanket approval for all token access. Keep `parse_checked`, token-stream/EOF
and source-span validation, resource limits and the fatal latch needed by those
failures. Verify malformed tokens and existing syntax/depth regressions.

### R03 — Arenas and spans: prove first; retain public checked APIs

Item 06; `0c170376`, `e5ef4670`, `5b127296`, `6faec525`.
Public IDs and mutable AST metadata do not by themselves establish valid indexing.
`InvalidId`, invalid source/span/UTF-8 bounds, capacity and allocation errors have
different contracts. A private just-allocated ID can qualify locally; arbitrary
IDs supplied to public APIs cannot. Keep checked arena/source entry points unless
a separate, explicit validated-input API contract is established. Do not undertake
an arena redesign merely to remove `?`. Retain checked-arena/source boundary tests.

### R04 — Namespace dispatch: simplify redundant private checks

Item 08; `0dde88f3`.
[Namespace resolution](../crates/interpreter/src/typechecker/infer/namespace_symbol.rs).

The expression callers establish a nonempty chain and root membership before
dispatching namespace resolution. Inspect `resolve_namespace_chain` and the
nonempty-path checks against all callers: where no mutation intervenes, use that
contract instead of a second internal failure. A private helper whose only error
is the established root/path condition can lose `Result`; enclosing inference
still reports genuine language/setup failures. Verify namespace/import fixtures.

### R05 — Capture, narrowing and scope restoration: prove first

Items 07–11; `0dde88f3`, with later confirmation in `6086f295` (SUB-1070).
Saved flow state, pending joins, binding tables and restoration stacks can have
invariants, but these conversions were mixed with actual narrowing isolation and
top-level block closure fixes. Retain those semantic fixes. Review each push/pop,
early return and reentrant path before simplifying a state helper. A generic
“typechecking guarantees it” is insufficient. Preserve closure/narrowing fixtures.

### R06 — Generic substitution and exhaustiveness: keep structural improvements

Items 11, 36; `0dde88f3`, `5ab68b6b` (merge `9640d6c1`).
[Substitution](../crates/interpreter/src/typechecker/type_param_substitution.rs),
[filters](../crates/submilli-blueprint/src/filter.rs).

Matching BTreeMap key sets established the old lookup invariant; paired ordered
values avoid the lookup altogether. The old nested comparison match was also
narrowed to its numeric variants; the extracted numeric evaluator expresses this
cleanly. Retain both structural changes. Retire any requirement to make those
specific established cases recoverable, without reintroducing old code.

### R07 — Symbol, class, closure and validator registration: prove first

Items 09, 13, 15–17, 19–21; `982a7db5`, `b699da07`, `c2dce469`.
Candidates include builtin/intrinsic registration, preallocated functions,
class layouts/vtables, closure environments and recursive-validator tables.
For each lookup, identify the producer, full supported variant set, ordering,
all consumers, and any imported/public metadata that bypasses the producer.
Only then simplify internal-only propagation. Missing dependency metadata and
unsupported source constructs still need errors. Keep emitted cast/throw behavior
and the distinction between failed compilation and a valid guest trap.

### R08 — Emitter root scope and temporary state: simplify selectively

Items 18–20; `b699da07`.
[Function emitter](../crates/interpreter/src/codegen/function_emitter/mod.rs).

Construction seeds a root scope and `pop_scope` prevents removing it. This supports
removing repeated `require_scope` failures at private accesses. Inspect
`define_local`, `record_single_evaluation`, `end_single_evaluations` and their
callers; marks/slots need their own provenance proof. A public invalid-pop call
cannot silently acquire a new precondition. Keep local-count/parameter limits,
allocation failures and real emission errors. The emitter as a whole remains
fallible. Verify scope/finally/temporary evaluation and local-limit behavior.

### R09 — Literal pools: prove first, with one narrow simplification

Item 14; `982a7db5`.
String pool tables and BigInt literal storage are publicly mutable, so “interned
earlier” is not a universal guarantee. Keep invalid-index checks at those boundaries,
UTF-16 handling, Wasm-width and limb-count limits. After `check_decimal_digits`
establishes a nonempty decimal string, the BigUint parser's rejection branch is a
candidate invariant under its documented contract; remove duplicate validation
only within that proven call path. Do not make the entire pool API infallible.

### R10 — Call metadata serialization: prove first, promising small API reduction

Item 14; `982a7db5`.
[Call arguments](../crates/interpreter/src/codegen/call_arguments.rs),
[default values](../crates/interpreter/src/package_declaration.rs),
[floating-point serializer](../crates/interpreter/src/artifact_f64.rs).

`metadata` serializes a closed vector of optional `DefaultValue` plus flags;
`typed_metadata` propagates it. Audit every enum/custom serializer and the
in-memory JSON serializer contract, including non-finite numbers. The f64 bridge
explicitly serializes non-finite values as strings. If all shapes are supported,
these metadata-only `Result` returns can disappear. Keep wrapper emission fallible
for emitter limits/registration. Verify default/rest/enum/non-finite metadata.

### R11 — DWARF and diagnostics: keep limits and real writer failures

Items 14, 33; `982a7db5`, `16e10725` (upstream merge `0f66c9d6`).
Plain formatting into an unrestricted String using known scalar formatters can
qualify as an invariant. This does not apply to the bounded diagnostic writer:
truncation, allocation, invalid source/metadata and formatting failure are tracked.
Keep bounded recursion/output, useful source-less errors and actual gimli writer
errors. Do not revert item 33 to recover a handful of formatting `expect`s.

### R12 — Singleton GC type lookup: simplify after successful build

Item 23; `0f8395eb`.
[Singleton helpers](../crates/interpreter/src/runtime/gc_singleton.rs).

Each helper declares, defines and builds one type, then looks up that same ID and
kind. Document the builder's successful-build contract and use a descriptive
`expect` for that lookup. **Keep `build()` fallible** and retain its fatal error
classification. Apply the same reasoning individually to intrinsic/error/Git type
construction; this is not permission to unwrap arbitrary engine setup failures.

### R13 — Number formatting wrappers: prove first; remove invariant-only layer

Item 24; `8a1ff644`.
[Number operations](../crates/interpreter/src/runtime/number.rs).

Candidates: missing/unparseable exponent after Rust's own finite exponential
formatting, finite-f64 BigInt conversion, and `from_digit` after a proven radix
bound. Check edge cases individually. If these are the only failures of private
`to_*_checked` functions, remove their engine-error layer and retain the ordinary
string error interface for invalid precision/radix. Preserve numeric semantics,
non-finite behavior, UTF-16 fixes and resource accounting. Verify rounding,
subnormals, negative zero and radix/precision boundaries.

### R14 — Fixed host ABI slots: simplify after boundary validation

Items 04, 25; `3ca8e039`, `8a1ff644`.
[Host wrappers](../crates/interpreter/src/runtime/host.rs),
[fuel wrappers](../crates/interpreter/src/runtime/fuel.rs).

`check_host_abi` validates buffers before registered callbacks. Fixed `abi_arg` /
`abi_result` accesses can rely on the declared signature if every registration
route, including async/fuel wrappers, passes through that check and no mutation
invalidates it. Remove redundant private errors there. Keep boundary validation
initially; removing it is a separate engine-contract proof. Dynamic guest indexes,
GC casts and arbitrary host callbacks need independent checks. Keep fatal host
errors, guest exception separation, capabilities and fuel behavior.

### R15 — Cursor crypto and fixed-width conversions: prove first

Item 28; `8a1ff644`.
[Cursor implementation](../crates/interpreter/src/stdlib/session/cursor.rs).

HMAC construction with an accepted key length and truncation of a fixed digest to
8/16 bytes are promising invariant-only errors. Prove the crypto API contract and
every `truncate<const N>` instantiation. `CursorError::Internal` also represents
size/allocation failures, so do not delete it wholesale. Keep entropy failures,
untrusted cursor decoding, prefix/authentication checks, UTF-16 fidelity and size
bounds. Verify tampering, prefix mismatch, no entropy and maximum-size inputs.

### R16 — Git byte widths and borrowing: prove first

Item 28; `8a1ff644`.
A fixed-size conversion immediately after an exact-length check can be an
invariant. Remote pack headers, request paths and dynamically selected slices
remain untrusted. `try_borrow_mut` around pending worktree state needs proof about
callbacks/reentry and ownership before replacement with a panicking borrow.
Retain Git publication/cancellation and memory checks. Git changes require the
focused Git memory-limit suite specified in AGENTS.md.

### R17 — Blocking workers and client factories: keep

Items 26, 27; `878eb30d`, `c2dce469`.
Thread spawn, runtime acquisition, allocation, HTTP client construction and
join/channel failure are real operational or lifecycle cases. Keep
`BlockingWorkError`, fallible spawn/factories, owner draining and fatal handling.
An allowed invariant panic in a worker does not make worker ownership unnecessary.
Do not reintroduce `resume_unwind` just because the original panic is now allowed.

### R18 — Poison-only server/store plumbing: already simplified

Items 29, 30. Introductions include `bca59bfe`, `29ccc412`, `0013e022`
(merge `02cd1383`), `57a662c4` (merge `18619267`). Reversal:
`91c2ec7c` (corresponding local commit `e46b7903`).

The poison-only simplification already removed about 698 lines and added 199
across 19 files. Do not propose it as fresh work. Keep Result contracts that also
carry SQLite/backend/I/O/setup failures, and keep console writer failures distinct
from poisoned console locks. Record current locks under P01 and update obsolete
issue acceptance text. Recheck later edits for new poison-only wrappers.

### R19 — LLM request construction and retry dispatch: keep structural changes

Item 34; `51bdeecf` (merge `a40754bc`).
[Wire construction](../crates/submilli-shared/src/llm/wire.rs).

An object created by `json!` is known to be an object; old `as_object_mut().expect`
sites were genuine construction invariants. Direct Map construction is already
clear and needs no reversal. The retry dispatcher can rely on unwrapping Retry
variants, but the current loop is also clear. Keep real client-build errors and
provider failure/accounting behavior.

### R20 — LLM batch bookkeeping: optional simplification, prove first

Item 34; same history as R19. A locally owned semaphore never closed by any
participant can establish successful acquisition. An exhausted stream of exactly
one indexed future per input can establish complete result slots. Current bounded
stream collection plus ordering is valid too. Compare readability and ordering
cost before choosing positional slots again; no performance benefit was measured
here. Preserve concurrency bounds, order, per-result failures and accounting.
This is lower priority than removing whole error-only API layers.

### R21 — Embedded MCP schema packs: prove first, strongest plumbing candidate

Item 35; `041d980a` (corresponding `c4ee873a`).
[Schema registry](../crates/submilli-shared/src/mcp/schema_registry.rs),
[discovery](../crates/submilli-shared/src/mcp/discovery.rs).

Production packs come from `include_str!`, not a remote response. With every
compiled asset's JSON/tools shape validated by maintained tests/build checks,
initialization can use a documented invariant. Candidate removal chain:
`SchemaPackError` → cached `OnceLock<Result<...>>` → fallible `github_pack` /
`initialize_builtin_packs` / `pack_for_url` → `DiscoveryError::SchemaPack` and
asset-only caller propagation. `pack_for_url` can then return Option directly.

Inspect all callers before deletion. Keep `DiscoveryError::ClientBuild`, actual
allocation failures and remote schema/discovery errors. Keep asset validation
tests; replace only tests whose sole contract is recoverability from corrupt
compiled assets. Lazy initialization may panic on a broken binary asset under the
new policy; document that explicitly rather than claiming inputs can never fail.

### R22 — Blueprint/build traversal and serialization: mixed; keep traversal

Item 36; `5ab68b6b` (merge `9640d6c1`). Retain iterative, symlink-aware module
walking and filesystem errors: those address actual external state. Filter
exhaustiveness is R06. Existing YAML/JSON/path `expect`s are P02–P06; assess their
actual serializer/path contracts instead of treating every serialize call as
fallible or every generated value as infallible.

### R23 — Parser, arity and compiler limits: keep

Items 01, 02, 12. `9e7b7773`; `6e5e1728`, `a691e275`, `3789596d`;
`982a7db5`, `8d1eb415`, `adbeed14`.
Parser recursion produced a process abort; excessive closure arity produced a
panic. Keep validation across methods/adapters/imported signatures, compiler
type/work limits and reduced recursion frame sizes. Those are input-reachable
limits, not internal invariants. Aggregate budgets remain open under item 38.

### R24 — Request boundaries and recording: keep real errors; narrow audit

Item 31. Keep preparation/import/discovery/store/recording failures through CLI,
HTTP, MCP and direct library entry points. A string conversion at an outer boundary
is not itself a panic defect. No additional typed propagation should be demanded
solely for an invariant that qualifies under this policy. Inventory any remaining
concrete failure and its caller before commissioning another propagation cascade.

### R25 — Cancellation/settlement: deferred, no new panic finding

Item 32. The user explicitly deferred this item.
[Request ownership](../crates/submilli-server/src/graceful_shutdown.rs) spawns
the watched request; disconnect does not cancel the request fiber. Normal shutdown
drains requests; forced shutdown/serving failure/dropped serving futures are
separate cases. Existing workers have ownership/draining mechanisms. No new
unaccepted panic was demonstrated here. Do not demand an ownership redesign or
mark this complete from that observation; leave its historical status deferred.

### R26 — Remaining budgets and dependencies: keep a focused audit

Items 37–40. Resource exhaustion is not justified by an invariant comment. Keep
request/aggregate limits, including checks that allocate traversal frontiers
before enforcing a limit (for example type-size measurement), and relate work to
SUB-1108/SUB-1123 where appropriate. Review actual dependency preconditions and
OS failures. Engine stack expectations require validation/execution proof, not
automatic conversion to Result. Details and tracking changes appear below.

## Final fallible-contract decisions

Remove a leaf first, then walk callers upward until encountering a real remaining
failure. Do not infer that a whole API becomes infallible from one accepted check.

| Contract | Final outcome | What prevents broader deletion |
| --- | --- | --- |
| Embedded schema-pack APIs and error type | R21: removed the asset-only chain | Remote discovery/client construction remain fallible |
| Numeric `to_*_checked` wrappers | R13: removed private internal-only engine-error wrappers | User precision/radix errors remain |
| Call `metadata` / `typed_metadata` | R10: metadata returns directly under documented serializer proof | Call-wrapper emission still has limits/errors |
| Private namespace chain helpers | R04: removed redundant root/path error returns | Other inference failures remain |
| Private lexer/parser dispatch helpers | R02: simplified locally established dispatch branches | Token/source validation and limits still use fatal state |
| Emitter scope/temporary access helpers | R08: removed root-scope error checks; retained other emitter Results | Local counts, registration, emission remain fallible |
| GC singleton helpers | R12: accepted post-build lookup invariants | `RecGroupBuilder::build` still returns real errors |
| Fixed host slot accessors | R14: retained compact existing raw-slice helpers | Boundary ABI errors and dynamic values remain |
| Cursor crypto helpers | R15: removed fixed-key/tag error-only helpers | Entropy, malformed input, size/allocation remain |
| Compiler/typechecker/codegen entry points | R01/R03/R07: retained entry-point and public metadata errors | Limits, diagnostics, public metadata, allocation remain |
| Server caches/stores/console | R18: poison-only work already removed | Backend/I/O/writer failures remain |
| Workers/client factories | R17: retain contracts | OS/runtime/join/ownership failures remain |
| Diagnostics/DWARF | R11: keep bounded writer error propagation | Truncation/source/allocation/emitter failures remain |

## Every SUB-633 numbered item

The historical state below is the fetched issue state, not a new assessment of
completion, except the user-directed panic-only completion of item 32 recorded
above. Checked: 01–30 and 32–36. Open: 31, 37–42.

| Item | Historical subject | State | Final review disposition / inventory |
| --- | --- | --- | --- |
| 01 | Parser recursion | Checked | Keep reproduced abort fix; R23 |
| 02 | Closure arity | Checked | Keep reproduced panic fix; R23 |
| 03 | Compiler fatal contracts | Checked | Keep architecture, prune leaves; R01 |
| 04 | Fatal host vs guest errors | Checked | Keep semantic separation; R14 |
| 05 | Lexer/parser assumptions | Checked | Simplify proven local dispatch; R02 |
| 06 | Arenas/spans/source | Checked | Retain public arena/source checks; R03 |
| 07 | Patterns/capture/desugaring | Checked | Retain real fixes and public/recursive-state checks; R05 |
| 08 | Inference setup/namespaces | Checked | Simplified local namespace checks; retain registration errors; R04/R07 |
| 09 | Class inference | Checked | Retain registration/metadata checks; R07 |
| 10 | Expressions/flow state | Checked | Preserve narrowing fixes; R05 |
| 11 | Generics/schema/substitution | Checked | Retain current structure and checks; R05/R06 |
| 12 | Compiler walks/work | Checked | Keep bounds; aggregate scope stays in 38; R23 |
| 13 | Symbol/type lowering | Checked | Retain cross-phase registration errors; R07 |
| 14 | Pools/metadata/DWARF | Checked | Simplified decimal/metadata leaves; retain public pools/writers; R09–R11 |
| 15 | Closures/adapters | Checked | Retain arity and registration checks; R07/R23 |
| 16 | Class/imported-class emission | Checked | Retain producer/imported metadata checks; R07 |
| 17 | Casts/guards/validators | Checked | Retain registration and guest type checks; R07 |
| 18 | Emitter state/parameters | Checked | Simplify root-scope invariants; retain limits; R08 |
| 19 | Expression emission | Checked | Prune proven leaves, keep fallible emitter; R07/R08 |
| 20 | Statement/finally emission | Checked | Retain stack/label checks; scope-root simplification only; R08 |
| 21 | JSON/MCP/throw emission | Checked | Preserve compile-error vs guest-trap distinction; R07 |
| 22 | Top-level propagation | Checked | Keep fallible entry points and artifact integrity; R01 |
| 23 | GC/intrinsic/error types | Checked | Simplify post-build lookups only; R12 |
| 24 | Prelude operations | Checked | Removed private numeric wrappers; retain input/UTF-16 fixes; R13 |
| 25 | Host ABI/value access | Checked | Retain compact raw-slice ABI helpers; R14 |
| 26 | Workers | Checked | Keep operational errors and ownership; R17 |
| 27 | Client/runtime setup | Checked | Keep real construction/context failures; R17 |
| 28 | Other stdlib | Checked | Simplified fixed crypto/split leaves; retain input/borrow checks; R15/R16 |
| 29 | Console | Checked | Poison requirement superseded; writer errors remain; R18 |
| 30 | Poisoned caches/stores | Checked | Poison conversion requirement superseded and reversed; R18 |
| 31 | Outer preparation/recording | Open | Narrow to concrete real failures; R24 |
| 32 | Cleanup/idempotency | Checked | Panic-only review complete; broader lifecycle correctness outside scope; R25 |
| 33 | Diagnostics/backtraces | Checked | Keep bounded rendering; R11 |
| 34 | LLM | Checked | Retain wire/retry/batch structure; R19/R20 |
| 35 | MCP discovery/packs | Checked | Removed compiled-asset-only error chain; R21 |
| 36 | Blueprint/build | Checked | Retain structure/traversal; P02–P06 accepted; R06/R22 |
| 37 | Implicit panics | Open | Accept proven accesses; retain unresolved/input-controlled audit; R26 |
| 38 | Allocation/work bounds | Open | Keep real budget work; R26 |
| 39 | Engine | Open | Update current version and accept proven engine invariants; R26/T04 |
| 40 | Other dependencies | Open | Keep input/OS precondition audit; R26/T04 |
| 41 | Regression gate | Open | Gate unreviewed violations, not all panic syntax; T05 |
| 42 | Adversarial verification | Open | Test actual contracts; revise invalid-state expectations; T06 |

No entire open item is justified as completed solely by changing policy. What can
be removed now is the blanket requirement to convert proven invariants, the old
poison-only acceptance requirements, and fault-injection expectations that
contradict accepted invariant contracts. Individual sites can leave the unresolved
queue once their proof is recorded; retain them in the accepted ledger.

## Explicit-site baseline and completed dispositions

At the initial `16e10725` baseline, a Rust syntax-tree scan excluded test files and test-only nodes and inspected
panic/assert/unreachable macros and `unwrap`/`expect` calls. It found 51 candidate
nodes: 40 poisoned-lock accesses, six non-panicking `self.expect` parser calls in
session value decoding, and five other sites. A separate textual inspection found
three additional calls inside macro token trees. Thus the inspected production
set has **40 lock sites and eight other explicit candidates**, not 48 confirmed
defects. This is a scoped scan, not proof about macro expansion, implicit panics,
dependencies or all conditional compilations. Line numbers below are baseline
locators; follow symbols after edits.

| ID | Baseline site | Final disposition and reason |
| --- | --- | --- |
| P01 | 40 std poisoned-lock accesses, grouped below | Accepted: documented poison policy; initiating panics assessed separately |
| P02 | `submilli-blueprint/src/lib.rs:1302`, `to_yaml` | Accepted: closed YAML-supported serialization graph; proof below |
| P03 | `submilli-build/src/scaffold.rs:428`, generated tsconfig JSON | Accepted: fixed JSON Value construction and in-memory serializer contract |
| P04 | `submilli-build/src/scaffold.rs:466`, generated task JSON | Accepted: fixed JSON Value and in-memory writer; documented at macro call |
| P05 | `submilli-build/src/scaffold.rs:530`, package path `to_str` | Accepted: both callers pass validated UTF-8 components; malformed paths already return an error |
| P06 | `submilli/src/commands/blueprint/package_secrets.rs:46`, filter YAML | Accepted: the input is a Rust str serialized as a YAML scalar; filter parse failure remains None |
| P07 | `submilli/src/commands/skill.rs:225`, Sync unreachable | Accepted: preceding dispatch returns for Sync and does not mutate the command |
| P08 | `submilli/src/commands/mcp/authenticate.rs:151`, client ID | Accepted: if absent, successful registration assigns Some; failure returns before access |
| P09 | `submilli/src/commands/server/run_code.rs:135`, JSON Value serialization | Accepted: JSON Value serializer contract; descriptive expect and proof added; resource bounds remain separate |

P02 execution: inspected every Blueprint field and custom serializer. VFS/mounts
and SecretSource serialize maps; idle duration and FilterExpr serialize strings;
packages serialize a string sequence. Variables, auth proxy, Git and LLM contain
scalars/options/sequences/string-keyed maps. Permission/access actions are unit
enums; MCP OAuth is internally tagged (a map), not a nested YAML-tagged enum.
serde_yml 0.0.12 rejects byte serialization and nested YAML enum tags; neither can
be emitted by this closed graph, and none of its custom serializers rejects values.
The in-memory writer produces UTF-8. Documented this guarantee at `to_yaml`, with
an explicit reminder to revisit it when fields change. This accepts the Result
expectation; it does not bound allocation or recursive FilterExpr formatting.
Three independent reviews: one P3 stale remaining-work sentence corrected by the
parent, no higher-priority findings. Formatting, workspace Clippy and diff checks
passed; graph updated. Comment-only source change; runtime tests not rerun.

P03 execution: generated tsconfig is a locally constructed JSON Value tree of
strings, booleans, arrays and string-keyed objects. Package paths are converted to
strings before insertion; no custom serializer or fallible writer participates.
Documented the serialization invariant beside its expect. This does not classify
filesystem writes or allocation exhaustion as invariants. Three independent reviews
found no issues. Formatting, workspace Clippy and diff checks passed; graph updated.
Comment-only source change; runtime tests not rerun.

P04 execution: tasks.json is a fixed JSON Value literal, containing supported
scalars and containers with string keys. Its serializer has no user-supplied
Serialize implementation or external writer. Documented that guarantee at the
expect inside the formatting macro. Allocation limits remain separate. No behavior
change. Three independent reviews found no issues; formatting, workspace Clippy
and diff checks passed; graph updated. Runtime tests not rerun.

P05 execution: both `package_block` callers (`init_project`, `add_package`) pass
`validated_package_path` output. That helper rejects non-UTF-8 paths with
InvalidPackagePath before normalization. Normalization copies/removes components
of that validated string, or returns the literal dot path; no filesystem operation
mutates the owned PathBuf before use. The expect is therefore accepted without
removing the real public-path error. Added the caller proof at the expect.
Comment-only source change. Three independent reviews found no issues; formatting,
workspace Clippy and diff checks passed; graph updated. Runtime tests not rerun.

P06 execution: corrected the initial inventory description: this call serializes
`&str`, not FilterExpr. serde_yml supports strings (including escaping controls)
and writes to memory; no custom serializer or unsupported YAML shape participates.
The subsequent FilterExpr deserialization remains fallible and maps invalid syntax
to None. Documented the scalar guarantee at the expect. No recursive FilterExpr
formatting occurs at this site. Three independent reviews found no issues;
formatting, workspace Clippy and diff checks passed; graph updated. Comment-only
source change; runtime tests not rerun.

P07 execution: `skill::execute` first matches `&cmd` and returns immediately for
Sync. Remaining operations use the target/path and do not mutate the owned command;
the later match therefore cannot see Sync. The existing unreachable message names
this earlier dispatch. Retain it; operational path/install errors remain Results.
Documentation-only, no tests rerun. Three independent reviews found no issues;
diff checks passed.

P08 execution: immediately before the client-ID expect, the absent-ID branch either
returns an error for a missing registration endpoint/failed registration or assigns
Some(registration result). Existing configured/provider IDs bypass that branch.
No mutation occurs between the branch and access. The existing expect names these
three sources. Keep it and all actual configuration/network errors; no new fallible
layer is needed. Documentation-only, no tests rerun. Three independent reviews
found no issues; diff checks passed.

P09 execution: the response result is an owned serde_json::Value, whose variants
and string object keys are supported by the in-memory JSON serializer. It contains
no external Serialize implementation or fallible writer. Replaced the bare unwrap
with a descriptive expect and documented the contract. Output and remote-response
handling are unchanged. Recursive/large-value resource limits remain in 37–40;
this acceptance does not establish those bounds. Three independent reviews found
no issues. Formatting, workspace Clippy and diff checks passed; graph updated.
No runtime tests rerun for this comment/message-only change.

P01 lock groups (paths below `crates/`):

| File | Count | Baseline locators |
| --- | --- | --- |
| interpreter/src/runtime/mod.rs | 2 | 520, 534 |
| interpreter/src/stdlib/git/mod.rs | 2 | 112, 539 |
| submilli-server/src/session_manager.rs | 1 | 1017–1019 |
| submilli-server/src/session.rs | 2 | 57, 65 |
| submilli-server/src/idempotency.rs | 2 | 185–187, 437–439 |
| submilli-server/src/blueprint.rs | 15 | 146, 155, 162, 169, 179, 188, 201, 379, 387, 392, 399, 410, 422, 430, 444 |
| submilli-server/src/runner.rs | 2 | 716, 728 |
| submilli-server/src/session_store.rs | 1 | 108 |
| submilli-server/src/idempotency_store.rs | 3 | 196, 245, 274 |
| submilli-server/src/app.rs | 9 | 525, 537, 586, 606, 645, 710, 720, 736, 750 |
| submilli-server/src/mcp/router.rs | 1 | 141 |

P01 execution: reviewed the lock access/shared-field comments in all listed groups.
They identify potentially partial protected state and distinguish poisoned access
from the initiating panic. Keep all 40 baseline accesses as accepted sites; R18
already confirmed poison-only fallible plumbing was reverted. No recovery API or
source change is needed. Documentation-only, no tests rerun. Three independent
reviews found no issues; diff checks passed.

These groups support accepting poison access, not declaring the surrounding
functions panic-free. No new input-triggered panic was reproduced by this review.
P02 and P05 now have their serialization and caller-validation proofs recorded
above. All P01–P09 dispositions are now recorded individually. New accepted sites added
by R simplifications are recorded in their execution entries and the Linear ledger.

## Completed SUB-633 tracking changes

### T01 — Replace the obsolete overarching requirement

Execution complete: exact issue/ledger patches replace blanket invariant recovery
with AGENTS.md's documented construction/validation/API and poison dispositions.
Real failures, resource bounds, fatal-vs-guest semantics and historical evidence
remain. Accepted sites stay in the ledger rather than counting as removed panics.
Three independent reviews found no issues. Narrow patches applied to SUB-633 and
its source ledger; all changed anchors read back and verified. Parent status and
checkboxes unchanged. Documentation-only; diff checks passed, no tests run.

Align the issue's blanket “even internal invariants” requirement with AGENTS.md.
Define outcomes as fixed input/operational failure, accepted documented invariant,
accepted poisoned lock, out of scope with reachability evidence, or unresolved.
Keep baseline source links and completion history. Do not rewrite a past fix as
if it never happened. Link simplification decisions to R IDs and implementing PRs.

### T02 — Reclassify historical invariant-only work

Execution complete: applied issue/source-ledger crosswalk for every R01–R26 decision and
P01–P09 acceptance, with implementation commits and retained-error boundaries.
Items 29/30 current requirements explicitly supersede poison-only conversions;
dated history and existing checkboxes remain. Three independent reviews found one
P3 directional wording error, corrected by the parent; no higher-priority findings.
Both external updates were read back and every changed anchor verified. Linear
automatically linked the SUB-633 substring inside the file path; quoting the path
as code fixed it, and exact read-back then passed. Diff
checks passed; no tests run for documentation-only tracking.

Attach R01–R23 to their completed numbered items. Mark the poison-conversion
requirements in 29/30 superseded, citing the existing reversal. For 05–25 and
34–36, permit selective simplification rather than reopening all completed items.
Record structural improvements as retained, even when the previous panic would
now be acceptable. The implemented and retained outcomes are recorded in the execution entries above.

### T03 — Narrow 31/37 and preserve 32/38

Execution complete: applied narrow item 31/37 requirements and an explicit item 32 deferral
note. Item 38's frontier/aggregate budget findings stay unchanged. Source ledger
contains the same scope distinction. Three independent reviews found no issues.
Both writes read back and verified (normalizing Linear issue-link markup only);
checkboxes and parent status unchanged. Diff checks passed; no tests run.

Item 31 needs concrete boundary failures, not another universal Result cascade.
Item 37 needs proof/disposition of potentially panicking operations, not mandatory
replacement of all indexes. Keep unresolved arithmetic, narrowing, borrow,
recursive traversal/drop and unchecked-size questions. Do not treat serde_json
indexing or fallible `unwrap_*` as panics by name. Leave 32 deferred. Keep 38's
actual per-request/shared resource requirements and related prerequisite issues.

### T04 — Refresh dependency scope without erasing its baseline

Execution complete: applied current 0.1.9 engine scope while preserving the 0.1.4 baseline;
stack proof and separate-engine adoption remain unresolved. The absent SSH path
is explicitly an integration check. Three independent reviews found no issues;
issue and ledger writes read back and verified. No checkbox/status changes.
Diff checks passed; documentation-only, no tests run.

Item 39 originally named engine 0.1.4; its current scope and the inspected lockfile
now use 0.1.9. The original ledger remains historical evidence. The operand-stack
`pop().expect("operand stack underflow")` in engine `exec/stack.rs:137`, and tagged
stack operations, are proof candidates: show that validation and every execution
transition preserve height, including host calls and cleanup. They are not defects
merely because `expect` remains. The separate submilli-wasm agent is handling that
area; incorporate its proof/fix and required version adoption before closing work.

Keep real engine memory/GC/ABI and untrusted-module failure questions open. The
engine's own guidance already permits documented post-validation invariants while
requiring resource bounds and state restoration around host panics.

For item 40, distinguish documented dependency preconditions from operational
failure. The recorded git2/libgit2 initialization risk on the SUB-1229 SSH branch
is an OS-failure question, not an invariant; its `shared/src/github/ssh.rs` path
is absent from this checkout. Recheck when that branch is integrated rather than
claiming it is currently reachable here. Crypto, serializers, encoders and context
APIs each need their actual contract, not a blanket dependency exception.

### T05 — Replace a zero-panic target with a review gate

Execution complete: synchronized the canonical review skill with AGENTS.md and applied
item 41/source-ledger scope updates. Reviewers assess guarantees before reporting
violations and record accepted sites without reopening completed work solely for
panic syntax. The separate CI/lint implementation stays open. Three independent
reviews found no issues. Frontmatter, local links, Claude delegation, policy
consistency and diff checks passed. Both external writes read back and verified;
no checkbox/status changes. No runtime tests for this instruction-only change.

Item 41 should detect unreviewed explicit panic sites and require a documented
invariant, poison exception or ordinary error path. Its target is zero unreviewed
violations, not zero `expect`/assert/index syntax. Keep macro-aware inspection and
reject unimplemented input behavior or success defaults that hide failure.
An allowlist entry needs a symbol, guarantee and review evidence, not a blanket
module allow. This inventory does not implement a lint/CI gate.

### T06 — Test supported failure contracts

Execution complete: applied item 42/source-ledger contract updates. Preserved real failure,
resource, classification and follow-up tests; accepted private-invariant corruption
may panic. Item 32 stays deferred and the adversarial/fuzz gate stays open. Final
inventory reconciliation replaces stale proposal-only status and the original
no-implementation disclaimer. Three independent final document reviews found two
P3 stale-status sentences, corrected and parent-checked; no higher-priority findings.
All 26 R, nine P, six T and 42 issue mappings, local links and 42 referenced commits
validated; diff checks passed. Issue/ledger writes read back and verified. Confirmed
all 42 issue checkboxes are preserved, with 31/32/37–42 open and parent In Progress.
No runtime tests for this final documentation/tracking change.

Item 42 should retain source/arity regressions, malformed public metadata,
resource boundaries, operational fault injection, fatal-vs-guest classification
and healthy-request-after-failure coverage. A test that corrupts a private,
documented invariant may legitimately assert a panic; it need not force production
recovery plumbing. Poisoned-lock tests must reflect the accepted poison policy.
Keep cleanup tests for actual supported lifecycle paths without reopening deferred
32 here. Parent closure still requires dispositions for unresolved in-scope sites.

## Completion and remaining work

All R and P decisions were handled individually, independently reviewed, and
committed before continuing. Invariant-only plumbing was simplified in R02, R04,
R08–R10, R12–R13, R15–R16 and R21; other code decisions retained useful structure or
real failure contracts. R25 is complete within panic-only scope. P01–P09 are
documented accepted sites, not removed panics. T01–T06 synchronize current policy and tracking without
erasing historical fixes or changing the parent's In Progress state.

The remaining SUB-633 work is explicitly retained: concrete boundary/implicit
failure audits (31/37), resource budgets (38), engine/dependency proof and adoption
(39/40), a future CI/lint gate (41), and adversarial/fuzz verification (42). These
are separate backlog outcomes, not unfinished reversion candidates. No new
input-triggered panic was reproduced by this selective invariant review.

## Verification and review handoff

Each item records its focused verification and independent clean-code, correctness,
and edge-case review. R13 and R15 required follow-up review of their final changes;
P02, T02 and the final T06 reconciliation had only P3 wording findings, corrected
and parent-checked. All final
in-scope findings are resolved. Retention-only entries did not rerun runtime tests.
Rust changes passed formatting and workspace/all-target Clippy; implementation
entries retain the focused test counts, including failures and corrected reruns.
HTTP coverage was skipped where transport behavior was unchanged; in-process
server failure/recovery tests ran. Graph updates retained existing partial-parse
warnings; generated graph artifacts were not committed.

Linear updates were made through the
[canonical review skill](../.agents/skills/launch-review-agents-loop/SKILL.md)
and read back, with accepted sites distinguished from removed panics. Final
tracking reconciliation and document checks are recorded under T06.

Reviews covered each incremental task against its preceding commit. The starting
commit `16e10725` includes earlier diagnostic work; this ledger does not claim a
whole-PR review against main. No fetch, rebase, push, PR or full-suite run occurred
in this execution. A future PR must review its full proposed diff and perform the
repository's single post-rebase full-test gate. Engine source was not modified.
