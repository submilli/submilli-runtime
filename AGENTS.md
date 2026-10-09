# Submilli — contributor and agent guidance

`AGENTS.md` is the canonical guidance for all agents working in this repository.
Read and follow it before making non-trivial changes.

## Repository

This workspace contains the compiler/runtime (`interpreter`), CLI (`submilli`),
HTTP/MCP server (`submilli-server`), supporting build/blueprint/shared crates,
and the conformance suite. Maintained TypeScript packages live in `packages/`.

The runtime embeds [llm-prompt.md](llm-prompt.md) at compile time.
Package `docs/readme.md` files are build inputs and their examples are tested.
Changes must build and test from this repository without private documentation.

## Design constraints

- Keep the lexer, parser, typechecker, and code generator hand-written in Rust.
- Compile to WasmGC. `submilli-wasm` is the interpreter engine; the workspace
  dependency named `wasmtime` is an alias for it.
- Runtime and standard-library operations are Rust host functions. Preserve
  capability checks, caller attribution, and resource limits when changing them.
- Use `.ts` for new source and fixtures; `.subm` remains supported.
- Accept equivalent TypeScript syntax under the forgiveness principle. Do not
  introduce different semantics merely to accept another spelling.
- `any` is unsupported. Use concrete types or `unknown`. Missing values use
  `undefined`; explicit `null` remains distinct. Optional fields read as
  `T | undefined`. Casts with `as` and non-null assertions are runtime-checked.
- Language feature changes need a fixture demonstrating their behavior.
- Runtime strings are UTF-16 code units. Operate on those units rather than
  round-tripping through Rust UTF-8 strings, which loses lone surrogates.

## Server domain and application layers

These rules govern `crates/submilli-server/src/domain/` and
`crates/submilli-server/src/application/`, with implementations of ports in
`crates/submilli-server/src/adapters/`.

- **Ubiquitous language comes first.** Domain types, use-cases, and port contracts
  must express business concepts and actual behavior. Agree on what a concept
  means before naming or abstracting it. `SessionCache` and
  `SessionCleanupQueue` describe infrastructure; wrapping them in traits does
  not make them domain concepts. Existing exceptions are not precedents.
- **Domain is pure.** It owns business rules and state transitions, with no I/O,
  clock reads, or dependencies on server modules outside `domain`. It may depend
  on the shared kernel, currently `submilli-blueprint`. Reuse its `Blueprint`
  type rather than introducing a wrapper solely for storage concerns.
- Aggregates protect their invariants through meaningful operations such as
  `close`, `expire`, `recover`, and `replace_credentials`. Application code calls
  those operations rather than changing fields directly. Names must describe
  what happens: recording execution timestamps is not acquiring or releasing
  execution ownership.
- **Application knows domain; all other collaborators come through ports.**
  Organize use-cases under `application/blueprints/` and `application/sessions/`.
  Inject focused ports through use-case constructors. Do not depend directly on
  managers, storage implementations, transport types, caches, or other
  infrastructure, and do not collect unrelated operations in an environment
  trait. Application orchestrates work and may read the current time; it does
  not need to be pure.
- Define ports in application and implement them in adapters. Keep persistence
  records, serialization formats, and infrastructure details out of domain and
  application contracts. Session repositories work with the domain `Session`.
  Cross-aggregate use-case orchestration belongs in application, not storage.
- Unit of work and repositories are separate patterns here. `UnitOfWork` has
  the direct operations its callers need, such as `get_session` and
  `save_session`; it must not extend repositories or expose repository
  accessors. Independent repositories use names such as `get` and `save`.
  Add only methods needed by current use-cases. A unit of work accumulates
  changes, reads its own changes, and commits them without a separate changes
  argument. Dropping it without committing must discard uncommitted changes.

Domain events are a future direction, not a requirement to add an event system
to each change. The domain can produce business facts such as `SessionClosed`,
with orchestration outside application dispatching them to handlers for effects
such as scheduling cleanup. This can reduce application ports while keeping
infrastructure out of its vocabulary. When introduced, event delivery must
preserve transaction guarantees so a committed transition cannot lose its
required follow-up work.

## Errors and capabilities

Compile errors should include source context, a caret, relevant type or function
signatures, and an actionable fix. Thread source spans through every compiler phase.

When adding or removing a gated capability, update
[the capability catalog](crates/interpreter/src/stdlib/capabilities.rs) in the same
change, including its summary, filter fields, and example filter. Blueprint
scaffolding reads this catalog.

## No-panic execution paths

Production script execution must handle malformed input, unsupported language
constructs, resource limits, and operational failures through diagnostics, typed
errors, or appropriate runtime traps. This covers parsing, typechecking, compiler
transformations, codegen, module loading/setup, runtime and host functions,
request preparation, diagnostics, and cleanup through CLI, HTTP, MCP, and direct
library entry points. Documented internal invariants and poisoned-lock access
may panic under the rules below.

- Propagate expected failures through typed `Result` errors. Do not use `todo!`
  or `unimplemented!` for unsupported input on production execution paths.
- `expect`, assertions (including debug assertions), `unreachable!`, and explicit
  invariant panics are permitted when construction, explicit validation, or a
  documented API contract establishes the invariant. Explain what establishes it
  and why intervening mutation, callbacks, or other callers cannot invalidate it.
  Prefer `expect` with a descriptive message to a bare panicking `unwrap`.
- Absence of a reproducer, "should never happen," or a general claim that an
  earlier phase guarantees correctness is insufficient. Identify the actual
  guarantee and its scope. A private helper dispatched immediately after matching
  a variant or a fixed host argument slot after ABI validation can qualify;
  arbitrary external metadata, dynamic guest indexes, and allocation or I/O
  failures do not become invariants merely because they usually succeed.
- Prefer expressing an invariant structurally when that makes the code clearer.
  Do not introduce error variants, fallible APIs, or repeated checks solely for
  proven invariants. Keep existing fallible paths that remain simple or also
  report real failures; allowing invariant panics does not require reverting them.
- Review implicit panic and abort sources too: indexing/slicing, arithmetic and
  narrowing, borrowing, runtime-context APIs, recursive traversal/drop, unchecked
  allocation sizes, and dependency calls. Establish bounds by validation or a
  documented invariant, and enforce resource/depth limits before exhaustion. A broad
  `catch_unwind` wrapper or a clean text search does not satisfy this requirement.
- Preserve source context and distinguish ordinary language errors from internal
  failures. Never replace a failure with a successful default, partial Wasm, or
  an emitted guest trap that hides a compiler error. Internal host/setup/ABI
  failures must terminate execution rather than become guest-catchable exceptions.
- Error propagation must preserve capability checks, caller attribution, resource
  limits, cancellation, and cleanup. Drain owned work before releasing resources
  it still uses; preserve session/idempotency and uncertain-side-effect semantics.
- Fix violations in new/changed code and directly affected mechanisms. Track
  unrelated pre-existing sites separately without expanding every change into a
  repository-wide rewrite. Existing violations do not excuse new ones. Review
  must distinguish a confirmed policy violation from a demonstrated input-triggered
  failure. An explicit panic is not automatically a violation: assess its
  invariant first. A demonstrated violation needs no exploit reproducer.
- Tests may assert or panic to report test failures. Fallible APIs named
  `expect` or `unwrap_*` are not panicking operations merely because of their names.

Poisoned `std::sync::Mutex` and `std::sync::RwLock` access is also permitted to
panic, including during cleanup: the protected state may have been partly
updated by an earlier panic. Do not add recovery, error variants, or fallible APIs
solely for poisoning. Document the reason at the access, shared helper, or
protected field when changing poison handling. Do not claim poisoning is
impossible. The initiating panic must independently satisfy this policy;
poisoning does not excuse it. A poisoned-lock panic during unwinding can still
cause a second panic and abort the process.

Record justified invariant and poisoned-lock panics in SUB-633 as accepted
exceptions, with their reasoning, rather than as removed panics or unresolved
violations. Apply these exceptions when following review and no-panic workflows.

## Fuel for host functions

Fuel bounds the CPU a program spends, wherever it spends it. Wasm instructions
burn fuel by themselves. A host function (a standard-library or prelude function
implemented in Rust) must charge for its own work from the same budget, through
`crates/interpreter/src/runtime/fuel.rs`. A host function that charges nothing
is a free loop for any program.

**Host work is discounted.** One fuel is about 2.5 ns, the cost of one
interpreted Wasm instruction. Native Rust does the same work far faster, so host
work is priced by the native time it takes, not by counting Rust operations.
Copying a byte costs 1/8 fuel (`COPY`), not the dozens of fuel the same copy
would cost as a Wasm loop. Don't inflate a charge to match what the work would
cost in Wasm.

**Build every charge from the cost classes in `fuel.rs`.** A formula is `CALL`
plus `rate × n` for each class that describes what the function does with sizes
it knows: `COPY`, `SCAN`, `PARSE`, `ELEM`, `HASH`, `REGEX`, `IO`, `SYSCALL`, the
flat `TZ` and `GATE`, and the helpers `sort_cost` and `bigint_product_cost`.
Never write a raw number in a host function, and never tune a rate for one
function. The rates are placeholders that SUB-1270 calibrates in one place.
`CALL` is charged for you by `register_host_fn` and `fuel::host_func`. An O(1)
function costs `CALL` alone. Waiting costs nothing: time blocked on the network,
a model, or a child process is free.

**Charge only your own overhead.** Callbacks, comparators, getters, and the
hooks of user classes run as Wasm and pay their own fuel. The host function
charges its per-element overhead, not the callback's work. Charge each term at
one level: most charges already sit in shared helpers (`read_string_arg`,
`write_code_units`, `ArrayStorage`, the Map and Set probes, `check_security`),
listed in `plans/sub-1269-host-fuel-costs.md`. Don't charge again in the
function that calls them.

**Charge before the work, when the size is known.** `fuel::charge` refuses
when the budget is short, before anything happens. Charge the input before the
work and the output once its size is known, before building the result. For
work whose size appears only as it runs (walks, iterators, searches with early
exit), charge per item or per chunk inside the loop. Cap and charge unbounded
results before computing them, such as a BigInt power or a `replaceAll` output.

**Never lose an effect.** Durable execution will let a user continue a run that
stopped for fuel, so a stop must not discard work that already had an effect.
Refuse only before any effect: the request, the write, the commit. Once the
effect has happened, charge the rest with `fuel::settle` (or `settle_result`
while marshalling the result). It never refuses: it takes the budget to zero,
the call returns its result, and the run stops at the next fuel check in Wasm.
Never clamp a response to the remaining fuel or abort halfway through a write.
When a cost is uncertain, charge the lower estimate.

**Don't copy what you don't read.** An accessor such as `length`, `at`,
`charCodeAt`, or `pop` reads the GC value in place and costs `CALL`, not a
copy of the whole receiver. The nightly tests
`accessors_do_not_pay_for_the_whole_receiver` and
`operations_charge_for_the_input_they_process` (`crates/submilli/tests/run.rs`)
guard this.

`submilli:test` charges nothing. It runs only under `submilli build test`.
When you add or change a host function, add or update its row in
`plans/sub-1269-host-fuel-costs.md` with its formula and when it charges.

## Code style

- Rust 2024; typed errors in library APIs. `anyhow` is appropriate at the CLI
  boundary. Production execution paths follow the failure-handling and documented
  invariant rules above.
- Keep functions focused, names descriptive, and control flow easy to follow.
  Prefer early returns to nesting. Keep helpers below their callers.
- Preserve ordered tables and exhaustive dispatchers: they encode layout or
  completeness constraints. Extract named operations without obscuring those rules.
- Comments explain non-obvious invariants or reasons, not change history or
  what already-readable code does.
- TypeScript classes use `private` / `private readonly`, not `#private` fields.

## Mandatory review before pull requests

Use `/open-pr` in Claude, following
[.claude/commands/open-pr.md](.claude/commands/open-pr.md), or `$open-pr` in Codex,
following [.agents/skills/open-pr/SKILL.md](.agents/skills/open-pr/SKILL.md),
to prepare, verify, and open a pull request. Both workflows enforce this gate.

Before creating any pull request (including a draft), run the
`launch-review-agents-loop` skill:

- Claude: `/launch-review-agents-loop`, following
  [.claude/skills/launch-review-agents-loop/SKILL.md](.claude/skills/launch-review-agents-loop/SKILL.md).
- Codex: `$launch-review-agents-loop`, following
  [.agents/skills/launch-review-agents-loop/SKILL.md](.agents/skills/launch-review-agents-loop/SKILL.md).

This is mandatory for code, configuration, and documentation changes. Run the
clean-code, correctness, and edge-case reviews and triage every finding. Follow
the skill's priority-based completion rule: a round with only low-priority
findings can finish after those fixes and affected checks, without another round.
Reuse completed reviews in `open-pr`, including after a conflict-free rebase.
New implementation changes or conflict resolutions require renewed review;
committing reviewed content or parent-checked final low-priority fixes does not.
Complete required checks on the final proposed diff. A blocked
or non-converged review does not satisfy this requirement. If delegation is
unavailable, use the skill's separate-pass fallback and disclose that limitation
in the working handoff. Keep review rounds, finding dispositions, checks, and
outstanding items in that handoff for reuse. PR descriptions should briefly state
the problem and solution, with only material compatibility or rollout notes.
Filing an issue does not clear an unresolved defect
within the PR's scope.

## Verification

Choose checks from the entire proposed PR diff, including committed changes,
based on behavior affected rather than file extensions alone. Combine the checks
for mixed changes. The timing rule below applies to every development task.

During development and after review fixes, run only checks for affected areas
and directly affected callers; reuse still-valid results. Review cycles default
to reading code and existing evidence without running tests. Reviewers may run a
narrow test or reproduction to resolve a concrete hypothesis or edge case, but
must not run full suites. Neither `fix-issue` nor `launch-review-agents-loop`
runs full suites at handoff.

Run full tests exactly once, after the final rebase onto main (or the selected
PR base) and before opening the pull request. Finish implementation, review,
and review fixes before that run. Only a subsequent rebase that integrates new
base changes permits another full run. A no-op rebase, review round, fix,
commit/amend, or repeated `open-pr` invocation does not. After a full run exposes
a failure, fix it and rerun only the affected tests; retain the full-run result
and the focused follow-up results in the handoff. Do not rerun the whole suite
just to obtain a new all-green summary.

For Rust source changes or changes to Rust build/dependency/toolchain/lint
configuration, run from the repository root:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
```

Nightly-only tests are excluded from routine development and post-rebase PR
verification, including `SUBMILLI_FULL_TEST=1`. `SUBMILLI_TEST_NIGHTLY_ONLY=1`
enables the ECMA-262 and TypeScript conformance suite bodies, both compiler
determinism sweeps, compiler type limits (`type_limits`), Git memory-limit tests
(`git_memory`), host memory bounds
(`host_memory`), server memory caps (`memory_cap`), and the CLI fuel-accounting tests
`accessors_do_not_pay_for_the_whole_receiver` and
`operations_charge_for_the_input_they_process`. Conformance filters and
baseline-update settings do not opt in. Nightly CI enables the flag; the
[release skill](.agents/skills/release/SKILL.md) requires all these checks on the
final release candidate before publication. Enable the flag during development
only for focused verification of changes to these tests or their selection,
and for the Git standard-library checks below.

Whenever modifying the Git standard library (`crates/interpreter/src/stdlib/git/`),
run the Git memory-limit tests in addition to other affected checks:

```sh
SUBMILLI_SKIP_HTTP_TESTS=1 SUBMILLI_FULL_TEST=0 SUBMILLI_TEST_NIGHTLY_ONLY=1 cargo test --locked -p interpreter --test git_memory -- --nocapture
```

These tests use an in-process HTTP transport backed by local `git upload-pack`,
so they need no network access. The optional calibration report remains ignored;
it is not required by this command.

At the single post-rebase, pre-PR full-test gate, run:

```sh
SUBMILLI_SKIP_HTTP_TESTS=1 SUBMILLI_TEST_NIGHTLY_ONLY=0 SUBMILLI_FULL_TEST=1 cargo test --workspace
cargo run -p submilli -- build test --skip-network
```

During development, run affected Rust tests with full tests explicitly
disabled, for example
`SUBMILLI_SKIP_HTTP_TESTS=1 SUBMILLI_FULL_TEST=0 cargo test -p submilli-server`.
Include affected callers/integration tests when shared code changes. Do not use
`SUBMILLI_FULL_TEST=1` merely because a `.rs` file changed.

During TypeScript package development, run the affected packages' tests and
documentation examples:

```sh
cargo run -p submilli -- build test --skip-network -p @submilli/<package>
```

Run any additional package-specific checks documented by those packages, such as
blueprint policy tests. Ordinary TypeScript package or documentation edits do not
require Rust formatting or clippy. During development, documentation changes need
only affected documentation checks. Embedded/build-input documentation (such as
`llm-prompt.md` and package `docs/readme.md`) also needs its owning build/example
checks. These focused checks do not trigger an early full run or replace the
single post-rebase, pre-PR full-test gate. For public book changes, use the
documentation-site checks below. For agent instructions, commands, and skills,
validate their frontmatter, referenced paths, and workflow consistency.

When a compiler change alters what the TypeScript conformance suite finds, update
its committed divergences, and edit their `.triage` explanations by hand, including
those citing a bug it fixes; see [After a change to the compiler](crates/conformance/typescript/README.md#after-a-change-to-the-compiler).

Use focused tests while iterating. Interpreter fixtures use assertions to verify
runtime behavior; compile-error fixtures use `// expect-error: <substring>`.
Keep snapshots when the rendered diagnostic or declaration is the contract under test.

### Conditional HTTP tests

Use `SUBMILLI_SKIP_HTTP_TESTS=1` for routine Cargo verification. Cargo marks
annotated Rust tests that open HTTP sockets (including localhost mocks) as ignored.
For package verification, use `submilli build test --skip-network`: it skips
`network.test.ts`, `network_*.test.ts`, and their `.subm` equivalents before
compilation and reports the skipped files. Matching uses the filename anywhere
under `tests/`, not detection of network calls. The package runner ignores
`SUBMILLI_SKIP_HTTP_TESTS`. Keep new socket tests
annotated with `#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]` and
new live package tests under that naming convention. Ordinary tests, in-process
HTTP/MCP handler tests, and documentation examples still run. This selects tests;
it is not a network firewall. The Rust setting is read by Cargo build scripts, so
change it through `cargo test`, not by invoking an old test binary directly.

Run affected Rust HTTP tests with `SUBMILLI_SKIP_HTTP_TESTS=0`; for affected live
package tests, omit `--skip-network` and explicitly supply credentials with
`--env-var NAME`, `--env-file PATH`, or `--all-env`. Enable these tests when the diff changes
HTTP transport, server routing, request/response encoding, authentication,
proxy/SSRF policy, or a package's external API behavior. Include relevant
dependency and configuration changes. A parser/compiler change alone does not
require live HTTP calls. For test-selection or build-script changes, verify both
modes with synthetic package tests and a focused localhost test; contact real
APIs only when their integration behavior is affected.

Select the affected crate/test or package rather than all network integrations,
for example `SUBMILLI_SKIP_HTTP_TESTS=0 cargo test -p interpreter --test http_live`
or `cargo run -p submilli -- build test -p @submilli/jina --env-var JINA_API_KEY`.
These tests may require execution outside the sandbox, even for local listeners.
Request network access only for selected checks that require it. Record why HTTP
tests ran or were skipped and report skipped/ignored coverage accurately. If a
required HTTP check cannot run, report it as blocked rather than passed.

### Other checks

The chart has its own suite, not covered by `cargo test`: `helm unittest
charts/submilli`. Its `checksum/config` tests assert literal digests of the
rendered server configuration, so changing the default configuration moves them —
re-run and copy the reported `Actual:` values. Encryption-key tests also cover
key generation, retention, and reuse through mocked Kubernetes lookups.

## Documentation site

The public user book lives in `docs/`, in six parts that follow
[Diátaxis](https://diataxis.fr/): Start here (explanation, with a quickstart
tutorial), Blueprints, Packages, and Server (how-to guides), Tutorials, and
Reference.

Before writing or reviewing any page in `docs/`, read
[docs/WRITING.md](docs/WRITING.md). It says which type each part is,
how each type is written, and the checklist a page passes before review. Every
command output in the book comes from a real run.

Build it independently with:

```sh
cd docs-site
npm ci
npm run check
npm run build
```

See [docs-site/README.md](docs-site/README.md) for preview and hosting instructions.

## graphify

This project has a knowledge graph at graphify-out/ with god nodes, community structure, and cross-file relationships.

When the user types `/graphify`, use the installed graphify skill or instructions before doing anything else.

Rules:
- For codebase questions, first run `graphify query "<question>"` when graphify-out/graph.json exists. Use `graphify path "<A>" "<B>"` for relationships and `graphify explain "<concept>"` for focused concepts. These return a scoped subgraph, usually much smaller than GRAPH_REPORT.md or raw grep output.
- Dirty graphify-out/ files are expected after hooks or incremental updates; dirty graph files are not a reason to skip graphify. Only skip graphify if the task is about stale or incorrect graph output, or the user explicitly says not to use it.
- If graphify-out/wiki/index.md exists, use it for broad navigation instead of raw source browsing.
- Read graphify-out/GRAPH_REPORT.md only for broad architecture review or when query/path/explain do not surface enough context.
- After modifying code, run `graphify update .` to keep the graph current (AST-only, no API cost).

## Review artifacts

Do not commit generated review screenshots, before/after captures, recordings, or
test-output dumps. Show them in the conversation or attach them to the PR outside
Git. Product assets and required test fixtures are separate; commit review evidence
only when the user explicitly requests it.
