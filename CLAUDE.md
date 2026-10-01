# Submilli — contributor and agent guidance

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
- `any` and `undefined` are unsupported. Use concrete types, `unknown`, or `null`.
  Casts with `as` and non-null assertions are runtime-checked.
- Language feature changes need a fixture demonstrating their behavior.
- Runtime strings are UTF-16 code units. Operate on those units rather than
  round-tripping through Rust UTF-8 strings, which loses lone surrogates.

## Errors and capabilities

Compile errors should include source context, a caret, relevant type or function
signatures, and an actionable fix. Thread source spans through every compiler phase.

When adding or removing a gated capability, update
[the capability catalog](crates/interpreter/src/stdlib/capabilities.rs) in the same
change, including its summary, filter fields, and example filter. Blueprint
scaffolding reads this catalog.

## No-panic execution paths

Production script execution must not panic, including when an internal invariant
is violated. This covers parsing, typechecking, compiler transformations, codegen,
module loading/setup, runtime and host functions, request preparation, diagnostics,
and cleanup. It applies through CLI, HTTP, MCP, and direct library entry points.

- Use typed `Result` errors and propagate failures to the caller, or express the
  invariant structurally so the operation cannot panic. "Should never happen"
  and "the previous phase guarantees this" do not justify a panicking operation.
- Do not use `panic!`, `unreachable!`, `todo!`, `unimplemented!`, panicking
  `unwrap`/`expect`, or assertions (including debug assertions) on these paths.
  Tests may assert or panic to report test failures. Fallible APIs whose names
  contain `unwrap` are not violations merely because of their names.
- Review implicit panic and abort sources too: indexing/slicing, arithmetic and
  narrowing, borrowing, runtime-context APIs, recursive traversal/drop, unchecked
  allocation sizes, and dependency calls. Use checked or structurally safe
  operations and enforce resource/depth limits before exhaustion. A broad
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
  failure; an exploit reproducer is not required to remove an explicit panic.

## Code style

- Rust 2024; typed errors in library APIs. `anyhow` is appropriate at the CLI
  boundary. Production execution paths follow the no-panic requirement above,
  including internal operations believed to be infallible.
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
clean-code, correctness, and edge-case reviews, triage every finding, and repeat
until a complete round has no new confirmed findings. Complete required checks
on the final proposed diff; rerun the loop after subsequent changes. A blocked
or non-converged review does not satisfy this requirement. If delegation is
unavailable, use the skill's separate-pass fallback and disclose that limitation
in the PR. Report review rounds, finding dispositions, checks, and outstanding
items in the PR description. Filing an issue does not clear an unresolved defect
within the PR's scope.

## Verification

Choose checks from the entire proposed PR diff, including committed changes,
based on behavior affected rather than file extensions alone. Combine the checks
for mixed changes. Record why full tests are required or skipped.

For Rust source changes or changes to Rust build/dependency/toolchain/lint
configuration, run from the repository root:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
```

Run the full tests only when the change affects the compiler/runtime or inputs
that can change their behavior or conformance coverage. This includes interpreter
implementation, standard library host functions, language fixtures and snapshots,
conformance tests/harness/data, and relevant dependency, feature, build, or
toolchain changes. Changes in CLI/build/shared/server code require full tests
when they alter compilation, generated artifacts, execution, or runtime integration;
an isolated CLI message or server routing change does not by itself require them.
Inspect dependency and build-input relationships for configuration-only changes.
When impact is uncertain, investigate those relationships and explain the decision.

For compiler/runtime-impacting changes, run:

```sh
SUBMILLI_SKIP_HTTP_TESTS=1 SUBMILLI_FULL_TEST=1 cargo test --workspace
cargo run -p submilli -- build test --skip-network
```

For other Rust changes, run affected crates' tests with full tests explicitly
disabled, for example
`SUBMILLI_SKIP_HTTP_TESTS=1 SUBMILLI_FULL_TEST=0 cargo test -p submilli-server`.
Include affected callers/integration tests when shared code changes. Do not use
`SUBMILLI_FULL_TEST=1` merely because a `.rs` file changed.

For TypeScript package-only changes, run the affected packages' tests and
documentation examples instead:

```sh
cargo run -p submilli -- build test --skip-network -p @submilli/<package>
```

Run any additional package-specific checks documented by those packages, such as
blueprint policy tests. Ordinary TypeScript package or documentation edits do not
require Rust formatting, clippy, or the workspace test suite. Embedded/build-input
documentation (such as `llm-prompt.md` and package `docs/readme.md`) also needs its
owning build/example checks; use the full suite only if compiler/runtime behavior
or conformance coverage is affected. For public book changes, use the
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
charts/submilli`. Its `checksum/blueprints` tests assert literal digests of the
rendered `configmap-blueprints.yaml`, so editing that template or
`submilli.labels` moves them — re-run and copy the reported `Actual:` values.
Chart `version` and `appVersion` are pinned there so release bumps don't;
anything else that varies per release needs pinning too.

## Documentation site

The public user book lives in `docs/`. Build it independently with:

```sh
cd docs-site
npm ci
npm run check
npm run build
```

See [docs-site/README.md](docs-site/README.md) for preview and hosting instructions.

## graphify

This project has a knowledge graph at graphify-out/ with god nodes, community structure, and cross-file relationships.

Rules:
- For codebase questions, first run `graphify query "<question>"` when graphify-out/graph.json exists. Use `graphify path "<A>" "<B>"` for relationships and `graphify explain "<concept>"` for focused concepts. These return a scoped subgraph, usually much smaller than GRAPH_REPORT.md or raw grep output.
- If graphify-out/wiki/index.md exists, use it for broad navigation instead of raw source browsing.
- Read graphify-out/GRAPH_REPORT.md only for broad architecture review or when query/path/explain do not surface enough context.
- After modifying code, run `graphify update .` to keep the graph current (AST-only, no API cost).
