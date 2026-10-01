# Submilli Agent Instructions

`CLAUDE.md` is the canonical agent guidance for this repository. Read and follow
[`CLAUDE.md`](CLAUDE.md) before making non-trivial changes.

If any instruction here conflicts with `CLAUDE.md`, follow `CLAUDE.md` unless
the current user request explicitly says otherwise.

## No-panic execution requirement

Follow [CLAUDE.md's no-panic policy](CLAUDE.md#no-panic-execution-paths) when
implementing or reviewing execution-path changes. Internal invariants must use
typed error propagation or structurally non-panicking operations, even when a
failure "should never happen." Review implicit panic sources and error cleanup
as well as explicit panic calls. Apply the policy to the affected code and track
unrelated existing violations separately.

## Conditional HTTP verification

Use `SUBMILLI_SKIP_HTTP_TESTS=1` for routine Cargo tests and `--skip-network` for
package tests. Enable affected Rust socket tests with `SUBMILLI_SKIP_HTTP_TESTS=0`;
for live package tests, omit `--skip-network` and explicitly supply credentials.
Run these only when the changed behavior requires them, following
[CLAUDE.md](CLAUDE.md#conditional-http-tests).
Local HTTP mocks also need network access outside the sandbox. Keep in-process
handler tests enabled and report skipped coverage separately from passing tests.

## Mandatory review before pull requests

Use `$open-pr`, following
[the Codex PR skill](.agents/skills/open-pr/SKILL.md), to prepare, verify, and open
a pull request. It runs the review gate below and selects checks using
`CLAUDE.md`, enabling `SUBMILLI_FULL_TEST` only for compiler/runtime-impacting
changes or relevant inputs.

Before creating any pull request (including a draft), run
`$launch-review-agents-loop` using
[the Codex skill](.agents/skills/launch-review-agents-loop/SKILL.md).
This applies to code, configuration, and documentation changes. Complete the
review loop and required verification on the final proposed diff before opening
the PR; rerun the loop after subsequent changes. A blocked or non-converged
review does not satisfy this requirement. See `CLAUDE.md` for the shared policy.

## graphify

This project has a knowledge graph at graphify-out/ with god nodes, community structure, and cross-file relationships.

When the user types `/graphify`, use the installed graphify skill or instructions before doing anything else.

Rules:
- For codebase questions, first run `graphify query "<question>"` when graphify-out/graph.json exists. Use `graphify path "<A>" "<B>"` for relationships and `graphify explain "<concept>"` for focused concepts. These return a scoped subgraph, usually much smaller than GRAPH_REPORT.md or raw grep output.
- Dirty graphify-out/ files are expected after hooks or incremental updates; dirty graph files are not a reason to skip graphify. Only skip graphify if the task is about stale or incorrect graph output, or the user explicitly says not to use it.
- If graphify-out/wiki/index.md exists, use it for broad navigation instead of raw source browsing.
- Read graphify-out/GRAPH_REPORT.md only for broad architecture review or when query/path/explain do not surface enough context.
- After modifying code, run `graphify update .` to keep the graph current (AST-only, no API cost).
