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
