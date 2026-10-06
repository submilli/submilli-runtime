# SUB-633 — current panic inventory

Authoritative [Linear inventory](https://linear.app/submilli/document/sub-633-current-panic-inventory-2026-10-06-376d756ae854)
for [SUB-633](https://linear.app/submilli/issue/SUB-633/no-panic).

Reviewed 2026-10-06 at `8b7d567aad6b459e1bdb1f194d0e20527ca35a1f`
(PR #141), against integration base
`d14f4dbb3f0d29ee3735113691caf462ce1159ce` (`upstream/main` as fetched for
that PR). Dependency versions: `submilli-wasm 0.1.9` (the `wasmtime` alias),
Tokio 1.52.3, UUID 1.23.1, rmcp 1.7.0. This is a pinned audit, not a claim about
other worktrees or later main commits.

This replaces the old 01–42 checklist as the active inventory. The old source
ledger, research document, issue comments and
[sub-633-invariant-policy-inventory.md](sub-633-invariant-policy-inventory.md)
remain historical evidence. Completed fixes are not reopened just because their
files still contain accepted `expect`, indexing, or poisoned-lock accesses.

Historical references: [former issue checklist](https://linear.app/submilli/document/sub-633-historical-issue-checklist-before-2026-10-06-reset-df0d5a446b3d),
[old source ledger](https://linear.app/submilli/document/sub-633-complete-baseline-source-site-inventory-4b6b6b5a42c5),
and [original research](https://linear.app/submilli/document/sub-633-research-confirmed-reproducers-and-error-propagation-design-ef58e45c3c83).

Scope: production compilation, package and blueprint preparation, runtime and
host calls, HTTP/MCP/CLI entry points, direct library APIs, reporting and cleanup.
Track panics and process-abort mechanisms, including native stack exhaustion and
uncontrolled allocation. General cancellation, idempotency, timeouts, lints and
fuzz infrastructure are not independent panic findings. Genuine documented
construction/validation/API invariants and poisoned standard locks are permitted
by [AGENTS.md](../AGENTS.md#no-panic-execution-paths).

## Active work

Entries marked **fixed on branch** have completed focused verification and review;
other N entries remain open. Evidence means:

- **Reproduced:** the stated operation failed in a bounded scratch process.
  The entry says whether this was a public API, source input, or only a dependency
  operation. It does not imply an HTTP reproduction or a release-build failure.
- **Inspection:** the operation lacks a required bound or can panic on a real
  resource failure. A process abort has not been induced.
- **Question:** an investigation with a specific exit condition, not a confirmed
  failure or a justification for replacing an invariant with error plumbing.

| ID | Work | Evidence |
| --- | --- | --- |
| N01 | **Fixed on branch:** bound blueprint filter parsing, tree height and destruction | Debug/release 2 MiB stack regressions pass |
| N02 | Bound package graph DFS in build and GitHub resolution | Build DFS abort reproduced; resolver inspected |
| N03 | Check OAuth token expiry before adding it to `Instant` | Overflow operation reproduced; caller traced |
| N04 | Charge and bound variable-to-variable substitution chains | Budget bypass reproduced; stack risk inspected |
| N05 | Bound namespace metadata before recursive consumers | Inspection; direct metadata API |
| N06 | Validate sibling dependencies at public build entry | Public API panic reproduced |
| N07 | Make resolver error formatting safe for arbitrary UTF-8 | Public error formatting panic reproduced |
| N08 | Make watchdog thread creation fallible | Inspection; OS failure |
| N09 | Handle blocking-pool thread admission failures | Inspection; dependency OS failure |
| N10 | Propagate UUID entropy acquisition failures | Inspection; dependency OS failure |
| N11 | Bound lexer diagnostic collection before rendering | Source-controlled accumulation reproduced |
| N12 | Bound validation frontiers before enqueueing children | Allocation amplification measured |
| N13 | Bound expanded Wasm locals across a module | Inspection; engine compilation allocation |
| N14 | Bound aggregate MCP discovery pages/tools/schemas | Inspection; remote response accumulation |
| N15 | Bound artifact reads and retained package data before loading | Inspection; file-input allocation |
| N16 | Admit native string-builder output before allocation | Inspection; host allocation before store limit |
| N17 | Validate the public reaper's timer configuration | Inspection; zero interval panics |

| ID | Investigation | Exit condition |
| --- | --- | --- |
| Q01 | Direct runtime runner's native-stack contract | Prove/enforce stack requirements at the runner's actual polling thread |
| Q02 | Public typed-phase payload validation before clone/drop | Establish a documented validated-input contract or identify and fix missing checks |
| Q03 | Aggregate source/token/arena compilation memory | Establish practical aggregate bounds at the named materialization sites |
| Q04 | Engine stack/GC transition and callback invariants | Obtain a scoped preservation proof or concrete failing transition for the pinned engine |

Keep N/Q identifiers stable from this reset onward. Close an N entry only after
its mechanism and directly affected siblings are fixed or a genuine scoped
invariant is established, with focused evidence. Resolve a Q into an N finding
or an accepted guarantee; lack of a reproducer is not a proof. Record revision,
short disposition and verification in the entry, not repeated progress essays.

## Findings

Paths and line numbers below refer to the pinned review revision. Dependency
paths are relative to the stated crate's published source. All findings are
pre-existing on the integration base; relevant bodies and locked versions were
compared. PR #141 changes some surrounding source/lines, not these mechanisms.
The original audit included no fixes. Subsequent dispositions below record fixes
on `codex/panic-inventory-fixes`; they are not yet merged.

### N01 — Blueprint filter recursion and recursive destruction

**Sites:** `crates/submilli-blueprint/src/filter.rs:1031` (`parse_or`), `:1041`
(`parse_and`), `:1051` (`parse_not`), `:1059` (`parse_primary`); recursive
`FilterExpr` at `:68`, evaluation at `:179`, collection helpers at `:247`, and
formatting at `:531`. YAML entry: `crates/submilli-blueprint/src/lib.rs:1233`;
HTTP preparation: `crates/submilli-server/src/handlers/blueprint.rs:80`.

Parentheses and `not` recurse without a depth bound. Flat `and`/`or` chains are
parsed by loops but create arbitrarily deep boxed trees. Evaluation, formatting,
clone and derived drop remain recursive. Valid YAML and valid filter grammar do
not establish a safe tree height. The accepted serialization `expect` at
`lib.rs:1332` does not excuse stack exhaustion inside filter formatting.

**Evidence:** existing debug library, separate processes, 2 MiB worker stack:
10,000 `not` operators (40,006 filter bytes) and 3,000 parentheses (6,006 bytes)
aborted during parsing. A complete blueprint YAML with the latter filter
(6,090 bytes) also aborted through public `submilli_blueprint::parse`.
20,000 comparisons joined with `and` (219,995 bytes) parsed, then aborted during
explicit drop. Formatting 10,000 joined comparisons also aborted after parsing.
These are library/YAML reproductions, not live HTTP tests or release thresholds.

**Direction/done:** limit grammar recursion AND constructed tree height/node
count before building unsafe trees, or use bounded iterative representations and
walkers. Cover rejection cleanup, `not`, parentheses, flat `and`/`or`, evaluation,
serialization and destruction; verify ordinary filters and a healthy follow-up.
Programmatically constructible `FilterExpr` trees need an explicit ownership and
validation contract too. A parser-only counter does not close this item.

**Disposition (2026-10-06, fixed on branch):** private `FilterExpr` nodes preserve
parser-established bounds: at most 128 total operators/groups and 64 KiB of both
input and canonical spelling. Oversized filters return existing parse errors
before recursive construction; all tree consumers and rejection drop are bounded.
This removes public unchecked enum construction. Blueprint tests (224) and
isolated debug/release 2 MiB-stack regressions pass, including exact limits,
mixed nesting, malformed suffix cleanup, YAML rejection, canonical-size
round trips and healthy follow-up. Three independent reviewers completed two
rounds; the canonical-size finding was fixed. Formatting and workspace/all-target
Clippy pass. No full suites or live HTTP tests (transport unchanged).

### N02 — Package dependency DFS has no depth bound

**Sites:** `crates/submilli-build/src/driver.rs:447` (`topo_order`), `:469`
(`visit`, recursive call `:496`); `crates/submilli-build/src/resolve.rs:283`
(`Resolver::visit`, recursive call in its dependency loop).

`build_packages` accepts an arbitrarily long acyclic sibling graph. The visited
marks detect cycles but do not bound native recursion. GitHub resolution has the
same independent problem across fetched manifests. This is separate from the
already bounded installed-artifact closure in `package_store.rs:195` (128 levels).
CLI's larger compiler stack changes the failure threshold, not the missing bound;
server package preparation can also reach these build/resolution APIs.

**Evidence:** public `build_packages` on a constructed 3,000-package acyclic
manifest aborted on a 2 MiB stack before source files were opened. Resolver
recursion is confirmed by inspection; no network or resolver-abort test ran.

**Direction/done:** bounded iterative DFS or explicit graph depth/node budgets
for both walkers. Preserve dependency order, shared-node deduplication and cycle
diagnostics. Test long acyclic chains as well as cycles and rejection cleanup.

### N03 — Remote OAuth expiry overflows `Instant`

**Sites:** `crates/submilli-shared/src/mcp_token.rs:246–249` in
`OAuthTokenManager::refresh`; token JSON/form decoding at `:365–395`.

The endpoint's `expires_in` is an unrestricted `u64`. It becomes a `Duration`
and is added to `tokio::time::Instant::now()` using panicking addition. A 64 KiB
response cap does not bound this numeric value. Discovery and outbound MCP token
refresh reach this path. Refresh-token rotation may already have been persisted
before expiry calculation, so a fix must preserve that completed effect.

**Evidence:** the exact addition with `u64::MAX` seconds panicked in Tokio
1.52.3 (`src/time/instant.rs:169`); decoding and caller flow were inspected.
No token-endpoint integration reproduction is claimed.

**Direction/done:** checked deadline construction and an explicit policy for an
unrepresentable provider lifetime. Test maximal/normal/absent expiry and rotated
credentials, preserving the last valid durable credential and future refresh.

### N04 — Substitution binding hops bypass depth and work budgets

**Site:** `crates/interpreter/src/typechecker/type_param_substitution.rs:90–100`;
public entry `TypeParamSubstitution::apply` at `:64`.

Distinct `T0 -> T1 -> ... -> number` bindings recurse at unchanged depth before
`budget.charge`. Cycle detection stops repeated names, not long distinct chains.
Every input type may be a depth-one leaf while native frames, the active-binding
vector and quadratic membership scans grow without charging each hop. The fixed
compiler stack and resulting-Type depth limit do not establish this bound.

**Evidence:** a 2,000-hop chain succeeded with only one type-work unit remaining
in a bounded probe on a compiler-sized thread. No stack abort or source-language
trigger was reproduced. Direct API reachability is established; generic inference
also uses this mechanism, but its achievable chain length remains unproven.

**Direction/done:** iterate or explicitly charge/bound binding hops, including
cycle detection work. Cover long distinct chains, self/mutual cycles, exhausted
work and normal substitution results.

### N05 — Namespace metadata depth is not validated

**Sites:** `crates/interpreter/src/package_declaration.rs:167–237`
(`check_type_limits`/`for_each_type`); recursive consumers at `:240–250`,
`:514–525`, and `crates/interpreter/src/typechecker/infer/imports.rs:36–67`.

The declaration check visits namespace containers iteratively but only validates
their contained `Type` values. Arbitrarily deep empty namespaces pass. Registry
population then recursively descends those namespaces and constructs longer
qualified paths. These public metadata structures are not constrained by the
source parser's depth bound. Recursive clone/drop also requires a disposition.

**Evidence:** confirmed by tracing direct `PackageDeclaration` input through
checked compilation/inference. No abort reproduced. Ordinary source cannot
introduce namespace declarations; normal JSON decoding supplies a separate depth
limit, so this is specifically a direct metadata boundary finding.

**Direction/done:** validate namespace depth, nodes and accumulated path bytes
before recursive consumers, or make those consumers bounded and iterative.
Test deep empty namespaces, valid imports and ownership/drop on rejection.

### N06 — Public build entry assumes sibling validation it does not perform

**Sites:** `crates/submilli-build/src/driver.rs:51–75`;
public mutable `ProjectManifest`, `PackageManifest`, `ResolvedDependency` in
`crates/submilli-build/src/lib.rs:60–92`.

`topo_order` skips a sibling name absent from its name table; the later
`built[&dep.name]` access panics. The manifest loader validates normal file inputs,
but callers can construct or mutate the public model and pass it directly to
`build_packages`. That entry has no validated wrapper or documented precondition
establishing membership. Private topological indexes drawn from `enumerate` are
separately safe; they are not this finding.

**Evidence:** a one-package public manifest referring to a missing sibling
panicked at `driver.rs:72`. This is a malformed direct-API input reproduction,
not evidence that loader-validated TOML reaches the same panic.

**Direction/done:** validate public graph inputs or accept a structurally
validated manifest type. Return a useful dependency diagnostic for missing
siblings; test mutation/construction and preserve valid topological order.

### N07 — Public resolver error formatting assumes ASCII SHAs

**Site:** `crates/submilli-build/src/resolve.rs:467–468` (`short`), called by
`ResolveError::fmt` at `:147` onward.

`&sha[..sha.len().min(12)]` can split a UTF-8 character. Loader validation proves
ASCII for normal parsed GitHub pins, but public error variants and public
`DependencySource::Github { sha: String, .. }` do not carry that guarantee.
Formatting a library failure must not add another panic.

**Evidence:** formatting `ResolveError::Fetch` with eleven ASCII `a` characters
followed by `é` panicked at byte 12. Normal loader-validated CLI pins are excluded
from this reproduction.

**Direction/done:** shorten at a character boundary or enforce a validated SHA
type at the public boundary. Cover Unicode, short strings and normal 40-digit
pins across the affected error variants.

### N08 — Watchdog thread creation uses a panicking OS API

**Sites:** `crates/interpreter/src/runtime/watchdog.rs:24–31`;
`RuntimeConfig::arm_timeout` at `runtime/mod.rs:487`; callers in direct runner
`:526`, CLI `commands/run.rs:476`, and package-test runner `commands/build.rs:1031`.

`std::thread::spawn` panics when the OS cannot create a thread. Resource availability
is not an invariant. This concerns CLI/direct timeout setup; the server's separate
`execution_timeout::start_ticker` already uses fallible `Builder::spawn`.

**Evidence:** source/API-contract inspection; no process thread exhaustion induced.

**Direction/done:** use fallible creation and propagate setup failure before guest
execution. Preserve timer ownership/disarming. Inject a spawn failure and verify
a later successful run; do not silently disable a requested timeout.

### N09 — Tokio blocking-pool admission can panic before a join exists

**Sites:** `crates/submilli-server/src/idempotency_store.rs:369`,
`session_manager.rs:1164`, `volumes.rs:189`, `handlers/packages.rs:360`.
Dependency: Tokio 1.52.3 `src/runtime/blocking/pool.rs:320–325`.

These server preparation/storage paths call `tokio::task::spawn_blocking`.
Tokio panics on `SpawnError::NoThreads`; handling the returned `JoinError` does
not handle a panic during admission. A configured runtime and a pool ceiling do
not guarantee OS thread availability. This finding concerns those concrete direct
calls; implicit Tokio filesystem/DNS worker admission is a residual dependency
coverage limitation, not a claim it was fully audited.

**Evidence:** pinned dependency source and first-party callers inspected; no OS
failure injection. `runtime::BlockingWork` demonstrates existing fallible native
worker creation with bounded admission, but changes must respect caller ownership.

**Direction/done:** arrange fallible worker admission for these operations or a
narrow error boundary for this documented admission failure. Preserve completed
writes and drain ownership. Test setup failure, operation errors and a healthy
follow-up; do not broaden this into a cancellation redesign.

### N10 — UUID generation panics if OS entropy fails

**Sites:** `crates/interpreter/src/stdlib/uuid.rs:76`; Git `storage.rs:510`,
`stage.rs:148`, `transport.rs:425`; server `audit.rs:125`, `:383`, `:769`,
`session_manager.rs:592`, `handlers/execute.rs:113`, `mcp/server.rs:783`,
`record/events.rs:188`. All use `Uuid::new_v4`.

UUID 1.23.1 `src/rng.rs:54–60` calls `getrandom::fill` and panics on failure in
the locked default RNG configuration. Randomness acquisition is an operational
failure, not a fixed-width UUID invariant. Audit/recording and cleanup-adjacent
Git paths need the same disposition as the guest UUID function.

**Evidence:** pinned dependency source and call-site review; no entropy failure
was injected. The session cursor and shared secret encryption already use fallible
randomness and should retain those real error paths.

**Direction/done:** generate the random bytes fallibly and construct the UUID
with the correct version/variant bits. Preserve fatal setup/host classification,
no partial publication, and existing ID format. For observation-only IDs, define
an explicit safe failure policy without inventing an apparently valid random ID.
Cover entropy failure at representative setup, guest and Git/recording boundaries.

### N11 — Lexer diagnostics grow before later error/render limits

**Sites:** `crates/interpreter/src/lexer.rs:112–130`, `:211–240`;
`crates/interpreter/src/compile.rs:96–132`.

`next_token_inner` continues past each invalid byte and appends a diagnostic with
allocated message/context. The parser's 20-error cap and bounded diagnostic
rendering run after collection; neither bounds this vector. The source-offset
ceiling near 4 GiB is not a practical diagnostic allocation budget.

**Evidence:** 10,000 `@` bytes produced 10,000 diagnostics in the first lexer
`next_token` call. This establishes source-controlled accumulation, not an OOM
reproduction. The outer pipeline retains and sometimes clones collected diagnostics.

**Direction/done:** cap diagnostic count/bytes during lexing with a clear limit
outcome and early stop. Preserve useful first errors and fatal-vs-language-error
semantics. Test repeated invalid bytes, multibyte errors, boundaries and a later
healthy compilation.

### N12 — Validation allocates an entire child frontier before checking its budget

**Sites:** `crates/interpreter/src/type_size.rs:88–99` (`measure`);
`crates/interpreter/src/tree_height.rs:780–790` (`annotation_nodes`).

A root's children are all pushed into `pending` before the next node-limit check.
A very small measurement budget therefore does not constrain temporary memory.
These helpers are used to validate public metadata and charge type copies, so
prior validation cannot be assumed for the very input they are validating.

**Evidence:** measuring a 100,000-element tuple with node/depth allowances of one
returned extent `(2, 2)`, but a scratch allocator observer measured a 2,097,152-byte
frontier allocation. No allocator failure was induced. The annotation sibling was
inspected; it has the same enqueue-before-check mechanism.

**Direction/done:** bound prospective frontier growth or traverse children lazily
with bounded frames and fallible reservation. Verify wide roots with tiny budgets,
ordinary limits and early error propagation. The already separate bounded
diagnostic preflight is not reopened.

### N13 — Engine expands local declarations without an aggregate compilation budget

**Sites:** submilli-wasm 0.1.9 `src/module/compile/mod.rs:79–86`,
`src/module/parse.rs:42–52`; first-party `RuntimeConfig::run_with_type_info` calls
`Module::new` at `crates/interpreter/src/runtime/mod.rs:523`.

Per-function local-count validation is present. The engine then expands each
run-length local declaration into the module-wide `code.local_types` vector using
infallible pushes. The encoded-module byte cap (default 256 MiB) does not provide
a practical bound on the aggregate expanded locals of many functions. The store's
GC/memory limiter does not govern this compilation allocation.

**Evidence:** pinned dependency inspection, no OOM or amplification benchmark.
Direct binary/library input is reachable; a source-generated exhaustion threshold
was not established. Do not describe validator limits as absent.

**Direction/done:** bound and charge aggregate expanded locals/retained arenas
before growth, with fallible reservation. Track the engine fix and adoption of its
released version here. Test repeated compressed local groups under a deliberately
small compile budget and successful ordinary module loading.

### N14 — MCP discovery limits each response but retains unbounded pages

**Sites:** `crates/submilli-shared/src/mcp/discovery.rs:500–523`;
`mcp/bounded_client.rs:162–180`; rmcp 1.7.0 `src/service/client.rs:378–392`.

`list_all_tools` extends a retained vector until `next_cursor` disappears. The
adapter's byte limit is per body/event, while `received` records traffic rather
than refusing an aggregate total. Discovery's ten-second timeout is a time bound,
not a retained-byte/tool/page bound. A fast endpoint can supply many individually
valid pages, which are then cloned/mapped into schemas and declarations. Schema
mapping's depth-12 guard does not constrain width or pagination.

**Evidence:** caller/dependency inspection; no remote test or OOM induced.

**Direction/done:** own bounded pagination with total pages/tools/schema bytes
and bounded/fallible accumulation. Account for decoded and mapped retention,
reject repeated/nonterminating cursors appropriately, retain connection draining
and fatal allocation classification. Test multiple individually small pages and
normal catalogs using in-process transport where possible.

### N15 — Artifact loading reads whole files before resource validation

**Sites:** `crates/submilli-build/src/artifact.rs:304–332`, `:343–366`,
`:375–388`; `PackageStore::load_closure` in `package_store.rs:162`;
server package preparation in `crates/submilli-server/src/app.rs:673–742`.

`fs::read`/`read_to_string` loads Wasm, declaration/type-info JSON, YAML, docs and
source maps before a byte budget. JSON nesting and artifact-version validation
are real checks, but do not bound file width or the aggregate retained artifacts.
The installed-package depth bound also does not constrain total bytes across a
wide closure. Wasm's later module-size check cannot prevent the earlier host read.

**Evidence:** local artifact/CLI/server call paths inspected; no large file or OOM
reproduction. The separate GitHub extraction cap does not cover arbitrary local
package stores or aggregate loaded artifacts.

**Direction/done:** bounded readers and an aggregate package-load budget before
read/decode/cache growth. Preserve I/O/format errors and package context. Test
oversized individual files and a wide closure of individually acceptable files;
never claim passing depth tests establish byte bounds.

### N16 — Native string results are allocated before tenant admission

**Sites:** `crates/interpreter/src/runtime/prelude/string/mod.rs:68–87`
(`repeat`), `:324–360` (`pad_start`/`pad_end`), `:314–318` (`concat`);
registration in `string/install.rs:614–639`, `:784–805`, `:810–835`, with GC
allocation in `StringAbi::write` at `:1039` onward.

The host operation constructs a Rust `Vec<u16>` before `abi.write` asks the GC
limiter to allocate its result. Repeat/padding have a 32 Mi-code-unit ceiling,
which is useful but still allows a 64 MiB native allocation for a small input,
independent of a smaller configured tenant cap. These output vectors use
infallible allocation. Concat similarly builds a separate combined native buffer.
The dedicated case-conversion wrapper at `:935` already uses `HostBytes`; it does
not establish a guarantee for these other wrappers.

**Evidence:** inspected allocation/admission order; no allocator abort induced.
This is an output-admission and allocation-failure item, not an accusation that
repeat/padding have no length guard or that UTF-16 indexing is unsafe.

**Direction/done:** admit native output bytes against the relevant budget before
building, reserve fallibly, then preserve GC accounting without double charging.
Keep UTF-16 units and real user range errors. Verify small tenant budgets reject
large outputs before native growth, boundaries still work, and accounting refunds
on error. Include concat and the named shared wrappers in the same fix.

### N17 — Public reaper accepts a zero timer interval

**Site:** `crates/submilli-server/src/session_manager.rs:963–978`
(`SessionManager::spawn_reaper`).

The public method accepts any `Duration`, sets `reaper_started`, then creates
`tokio::time::interval(interval)` inside its spawned task. Tokio rejects zero by
panicking. The normal `AppState` callers pass fixed nonzero `REAP_INTERVAL`, so
those calls have a genuine local guarantee; arbitrary direct callers do not.
A failed task also leaves the started flag set. An entered runtime alone does not
prove its timer driver is enabled.

**Evidence:** source/dependency contract inspection; no direct API execution.

**Direction/done:** establish the interval/runtime preconditions at the public
boundary before publishing started state, preferably structurally or through a
fallible start API. Test zero and supported nonzero configuration; decide and
document the timer-enabled runtime contract. No general reaper/lifecycle redesign.

## Questions requiring a scoped follow-up

### Q01 — Native stack for direct convenience runners

`crates/interpreter/src/runtime/mod.rs:406–416` explains a native-stack requirement
scaled from `max_wasm_stack`. `run`/`run_compiled`/`run_with_type_info` at `:497–528`
execute on the caller's polling thread. Engine `src/exec/mod.rs:90–96`, `:169–175`
charges 4 KiB per callback crossing; that is not a measurement of native headroom.
CLI/server provisioning does not establish the requirement for arbitrary library
executors. **Not a demonstrated overflow.** Resolve whether this is an existing
sufficient API contract or needs runner provisioning/clearer enforced admission;
verify supported stacks and callback recursion in debug/release. Do not remove
the existing documented compiler-sized-stack requirement or reopen bounded parser
recursion merely because a caller can violate that requirement.

### Q02 — Typed payloads at public phase APIs

`codegen/mod.rs:732` checks typed arena height, then
`codegen/runtime_values.rs:70` clones the public `TypedAst`;
`runtime_values/declarations.rs:93` clones dependencies. `typechecker/capture.rs:41–47`
also clones parameter payloads. Publicly mutable embedded `Type` trees are not
necessarily bounded by the arena-edge check. Determine whether every supported
entry supplies validated payloads before clone, formatting and drop; if the
contract is compiler-produced-only, establish that exact contract and scope.
Otherwise add the missing preflight as a concrete finding. **No reproduction or
blanket assertion that ordinary compiled programs fail.**

### Q03 — Aggregate source/token/arena memory

`compile.rs:105` builds the token vector before parsing; `tree_height.rs`
materializes arena-sized tables and child vectors. Per-type/per-function/depth
limits do not automatically prove a total per-compilation bound, especially with
many small independent declarations and concurrent compilation. Quantify these
specific retained buffers and their entry-point admission limits, then record a
bounded fix or a justified limit. N11/N12 cover already confirmed narrower gaps.
Coordinate with SUB-1123's compiler admission and SUB-1108's compile time limit;
neither a thread pool nor a timeout alone proves allocation safety. No claim that
every `Vec`/allocation must become a new error path.

### Q04 — Pinned engine's transition invariants

For submilli-wasm 0.1.9, resolve operand-stack pops, branch/index tables, GC handle
ownership and host callback re-entry against validator guarantees AND every
execution transition that mutates them. Validation alone is not a proof that an
interpreter implementation preserves the invariant. Conversely, a private
validated stack pop is not a bug merely because it uses `expect`. Obtain the
separate engine review's precise accepted guarantees or a concrete violation and
version-adoption requirement. This audit found N13 but did not prove every engine
transition. `Module::from_file` reads before checking module bytes, but no current
repository production caller was established; do not promote that isolated API
observation to a first-party finding without tracing a caller.

## Accepted guarantees

These are accepted sites, not removed panics or open fixes. Acceptance concerns
the stated operation; it does not grant an allocation, recursion or arbitrary
caller exemption to a whole module.

| ID | Mechanism and exact scope | Guarantee |
| --- | --- | --- |
| A01 | Lexer private newline/operator/delimiter dispatch; parser matched literals/template heads | The immediate caller has matched the byte/variant; no intervening mutation/callback changes it, and `advance` clones the matched token before moving the cursor. |
| A02 | Emitter root scope; private namespace root/path accesses | Constructor seeds the root and `pop_scope` cannot remove it. Namespace callers establish membership/nonempty paths before immutable resolution; field dispatch appends its member. N05 is a different namespace-depth issue. |
| A03 | BigInt decimal pool and call-default metadata serialization | Digits are nonempty ASCII decimal before BigUint parsing. Metadata is a closed JSON-compatible graph with explicit nonfinite-float encoding. Actual pool/index/width failures remain fallible. |
| A04 | Same-builder GC type lookups | Successful `RecGroupBuilder::build` preserves the locally declared handles/kinds; no external mutation or callback intervenes. Actual builder failures still propagate. |
| A05 | Numeric formatting and cursor cryptography | Finite/range validation plus formatter/conversion contracts establish the numeric cases. HMAC accepts every key length; private digest truncations request only 8 or 16 of 32 bytes. Entropy and dynamic input failures remain real errors (N10). |
| A06 | Fixed embedded schemas and closed serializer inputs | Schema registry parses only the compiled-in tested asset. Blueprint's supported serialization graph and plain filter strings have no unsupported serializer shape; JSON Values/fixed generated maps serialize to memory. Recursive filter formatting is explicitly N01, not exempted. |
| A07 | Local dispatch/path construction | `str::split` yields an initial component. Scaffold callers validate UTF-8 path components. Sync skill handling returns before target dispatch. MCP client ID is pinned/provided or registration assigns it before access. |
| A08 | Poisoned `std::sync` locks | Potentially partly updated protected state may panic on poisoned access, including cleanup, under AGENTS.md. The initiating panic needs independent justification; double-panic abort remains possible. No poison-only fallible plumbing requested. |
| A09 | Fixed host ABI slots and local collection indexes | Host registration validates argument/result signatures before body dispatch. Indexes derived immediately from bounds/iteration and immutable tables are structural invariants. This does not cover arbitrary guest indexes or the public manifest membership gap N06. |
| A10 | Existing structural/resource guards | Parser recursion/array suffix bounds, checked arena edges/cycles, closure arity, type/function limits, bounded rendering, Git pack/copy traversal, session JSON bounds and MCP schema depth checks remain useful. Recursive compiler work additionally needs its documented compiler-sized stack. These guards do not imply global memory safety (N04/N05/N11–N16, Q01–Q04). |

### Current explicit-site ledger

The fresh AST scan found 97 actual first-party explicit `expect`/`unreachable!`
sites. Inspection of macro arguments added three serializer expectations that
the AST traversal did not expose. All **100 known explicit sites** are accounted
for below: **58 documented invariants, 42 poisoned-lock accesses**.
Counts exclude two fallible `call_arguments::unwrap` functions, six fallible
session-parser `expect` calls, and the package-test runner's post-execution
`RefCell::borrow`. That borrow occurs after dispatch has completed, with no active
label-writing callback; test-only helpers and assertions were excluded separately.
This is syntax coverage, not a claim that implicit failures have all been proved.

| Source (under `crates/`) | Lines | Disposition |
| --- | --- | --- |
| interpreter/src/lexer.rs | 354, 1006, 1150, 1158, 1197 | A01 |
| interpreter/src/parser.rs | 3103, 3115, 4967, 5006 | A01 |
| interpreter/src/codegen/function_emitter/mod.rs | 348, 365, 384 | A02 |
| interpreter/src/typechecker/infer/namespace_symbol.rs | 72, 114, 157, 181 | A02 |
| interpreter/src/codegen/bigint_pool.rs | 108 | A03 |
| interpreter/src/codegen/call_arguments.rs | 24 | A03 |
| interpreter/src/runtime/gc_singleton.rs | 31, 51, 78 | A04 |
| interpreter/src/runtime/intrinsic_types.rs | 262, 265, 268, 271, 274, 277, 280, 283, 286, 289, 292, 295, 296, 299, 381, 444, 447 | A04 |
| interpreter/src/runtime/prelude/error.rs | 227, 230 | A04 |
| interpreter/src/stdlib/git/class.rs | 138, 141 | A04 |
| interpreter/src/runtime/number.rs | 86, 89, 131, 150 | A05 |
| interpreter/src/stdlib/session/cursor.rs | 227 | A05 |
| submilli-shared/src/mcp/schema_registry.rs | 50, 51 | A06 |
| submilli-blueprint/src/lib.rs | 1332 | A06; recursive formatting N01 |
| submilli/src/commands/blueprint/package_secrets.rs | 48 | A06 |
| submilli-build/src/scaffold.rs | 430, 470 | A06 |
| submilli/src/commands/server/run_code.rs | 139 | A06 |
| interpreter/src/stdlib/git/storage.rs | 905 | A07 |
| submilli-build/src/scaffold.rs | 536 | A07 |
| submilli/src/commands/skill.rs | 225 | A07 |
| submilli/src/commands/mcp/authenticate.rs | 151 | A07 |
| interpreter/src/runtime/mod.rs | 532, 546 | A08 |
| interpreter/src/stdlib/git/mod.rs | 106, 533 | A08 |
| submilli-server/src/app.rs | 401, 416, 590, 602, 653, 673, 712, 777, 787, 803, 817 | A08 |
| submilli-server/src/blueprint.rs | 146, 155, 162, 169, 179, 188, 201, 379, 387, 392, 399, 410, 422, 430, 444 | A08 |
| submilli-server/src/idempotency.rs | 185, 437 | A08 |
| submilli-server/src/idempotency_store.rs | 196, 245, 274 | A08 |
| submilli-server/src/mcp/router.rs | 141 | A08 |
| submilli-server/src/runner.rs | 789, 801 | A08 |
| submilli-server/src/session.rs | 57, 65 | A08 |
| submilli-server/src/session_manager.rs | 1017 | A08 |
| submilli-server/src/session_store.rs | 108 | A08 |

## Coverage and limitations

The fresh tree-sitter Rust scan parsed **473 non-test-candidate source files**
under `crates/*/src` with no syntax errors. It recorded 730 selected operations,
including 402 index expressions and 119 `with_capacity` calls. These are triage
counts, not defects. Test exclusion uses file/module/function attributes and is
heuristic. Follow-up source inspection removed known false positives. Macro
expansion, platform cfg evaluation and complete call/type resolution were not
performed. First-party macro definitions and explicit panic patterns in macro
arguments were checked separately; dependency and generated macro expansion
remains a limitation. Non-source build scripts,
conformance harnesses and maintained TypeScript packages do not get a claim of
first-party Rust execution-path proof from this scan.

| Area | Fresh review | Remaining limitation |
| --- | --- | --- |
| Frontend and compiler | Dispatch proofs, parser/tree guards, type substitution, metadata traversal, codegen entry/cloning, diagnostic collection/rendering | N04/N05/N11/N12; typed payload and total-memory questions Q02/Q03 |
| Runtime and standard library | ABI validation, numeric/GC/crypto guarantees, watchdog, JSON/vtable depth limits, selected string/BigInt/Git allocations and worker ownership | N08/N10/N16; Q01; other host/dependency allocation internals not exhaustively proved |
| Blueprint and build | Filter parser/AST consumers, UTF-8 slices, graph traversal, manifest validation, artifact readers, installed closure depth bound | N01/N02/N06/N07/N15; YAML dependency internals not exhaustively reviewed |
| Shared MCP/LLM/HTTP | OAuth expiry, catalog conversion/depth, bounded bodies, pagination, dispatch error paths, shared policy paths | N03/N14; a bounded response is not a proof of all dependency allocation behavior |
| Server and CLI | Preparation, sized compiler threads, timeout setup, UUIDs, storage workers, accepted locks, recording/events and reaper | N08–N10/N15/N17; library configurations/callback implementations need explicit contracts |
| Pinned engine/dependencies | Actual locked source for Wasm local expansion, native callback charging, Tokio worker admission/Instant/interval and UUID entropy; rmcp pagination | N09/N10/N13/N14; Q04; no complete transitive-dependency or abort-freedom proof |

No source-triggered engine operand underflow was demonstrated. No OS entropy,
thread-exhaustion or allocator exhaustion was intentionally induced. A
`catch_unwind`, successful suite, bounded depth elsewhere or clean explicit panic
scan would not close the unresolved items. Existing broad linter/fuzzer work is
verification tooling, not an extra active panic defect. Future concrete findings
should add bounded mechanism entries rather than restore a repository-wide
"audit everything" checkbox.

## Verification and review record

Inventory-only change; no production implementation changes or full suites.
Focused scratch checks used existing debug rlibs, not a fresh build after the
last rebase. Source mechanisms were compared with current checkout and the pinned
integration base. Parent checks: blueprint parse/format/drop, build DFS/missing
sibling, public error UTF-8 formatting, and the exact Tokio deadline operation.
Compiler reviewer checks: substitution budget, lexer accumulation, frontier
allocation. Abort checks ran only in separate processes with 2 MiB thread stacks,
15–20 second timeouts and core dumps disabled. The substitution/lexer probe used
the documented compiler-sized stack; the iterative frontier-allocation probe ran
on its process's main thread. No live HTTP, external provider or network test.

Independent review of this inventory covers organization/clarity, compiler
correctness, and runtime/dependency edge cases. Final document review and Linear
publication/read-back are recorded in the working handoff, outside Git. Existing
PR #141 tests remain historical evidence for that implementation, not proof that
the newly identified mechanisms are fixed.
