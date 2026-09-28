---
name: launch-review-agents-loop
description: Run iterative clean-code, correctness, and edge-case reviews of the proposed diff, triage findings, and verify the result. Required before creating any pull request in this repository.
---

# Launch review agents loop

Extracted from the review workflow in `fix-issue`. Review the proposed change
until a complete round produces no new confirmed findings. Issue IDs are optional;
use the user's requirements and intended behavior when there is no issue.

## Scope and prepare

- Resolve the active checkout with `git rev-parse --show-toplevel` and read its
  `AGENTS.md` and `CLAUDE.md`. Stay in that checkout, including linked worktrees.
- Record branch, status, staged and unstaged changes. Preserve unrelated work
  and existing staging. Identify the intended PR base and review the whole
  proposed diff: committed changes since its merge base, plus intended staged,
  unstaged, and untracked files. Do not assume `git diff` alone covers a PR.
  If the base cannot be determined, resolve it before claiming complete coverage.
- Run relevant focused checks before review. Reserve full required suites for
  final verification; scale checks to the changed files as `CLAUDE.md` requires.
- For TypeScript syntax, typing, or runtime changes, provide comparisons with
  the installed/project-pinned TypeScript compiler under strict options and its
  emitted JavaScript under Node. Record versions, options, commands, acceptance,
  values, side effects, evaluation order, and thrown error type/timing. Keep
  programs equivalent and document harness adapters. Distinguish existing gaps,
  introduced gaps, and previously authorized deviations. Mark unavailable oracle
  checks unverified; do not infer parity from compiler acceptance alone.

## Launch independent reviews

Explicitly delegate three read-only reviews. Run them in parallel when tools and
slots permit; otherwise stagger them. Use general reviewers with the role
instructions below rather than assuming a particular installed agent type.
Add fix-verification or merge-integration reviewers only for a concrete reason.
If delegation is unavailable, perform three separate passes yourself and disclose
that the review was not independent. Reviewers report to the parent; they must
not launch this loop recursively or create a PR.

1. **Clean code:** assess readability, naming, control flow, and organization
   against the rubric below and repository guidance. For non-Rust changes,
   apply the relevant language/documentation conventions. Do not hunt bugs,
   repeat lint findings, or manufacture findings.
2. **Correctness:** trace changes through callers and shared mechanisms;
   challenge assumptions, invariants, interactions, and TypeScript compatibility
   where applicable.
3. **Edge cases and coverage:** probe boundaries, error paths, sibling sites,
   and regression-test gaps with concrete counterexamples. Challenge the
   compatibility evidence where applicable.

Give every reviewer a self-contained packet containing:

- Exact checkout, requirements, intended behavior, root cause when applicable,
  changed files, full proposed diff, invariants, and useful sibling implementations.
- This skill's rubric, applicable repository guidance, and resolved paths to
  relevant design documents. Do not require private documents for this public
  repository or assume personal skills are installed.
- Concrete hypotheses, checks and results (including differential evidence),
  changes since the last round, and prior findings with their dispositions.
- Explicit read-only scope: no source edits, staging, stashing, checkout,
  restore, rebase, or Git-ref changes. Scratch reproductions belong outside
  tracked source. Specify allowed focused checks; prohibit repeated full
  workspace or full fixture sweeps. Check-generated build artifacts are allowed.
- Request ranked findings with file:line, evidence, expected versus actual
  behavior for bugs, and introduced versus pre-existing classification.
  Clean-code findings require a concrete rubric violation, not a reproduction.
  An empty findings list is valid.

### Clean-code rubric

Repository guidance takes precedence. Review changed/added code and surrounding
context; report untouched readability issues only if the change worsens them.

- Functions should do one coherent thing. Extract meaningful phases with small
  interfaces; do not split merely to meet a line count.
- Use descriptive domain names, question-like boolean names, and established
  abbreviations. Avoid cryptic names and redundant type encodings.
- Keep public flow before helpers and helpers below callers. Prefer guard
  clauses to nesting and data/iterators only when they clarify the operation.
- Comments explain invariants or reasons; follow repository error conventions.
- Ordered index/declaration tables encode layout: length is not a finding.
  Do not obscure or reorder their sequence through incidental cleanup.
- Keep exhaustive match dispatchers intact. Consider extracting substantial arm
  bodies, not splitting the dispatcher itself.
- Generated/encoding sequences, tests, and fixtures may be long and repetitive.
  Do not flag that alone or turn a review into an unrelated structural refactor.
- Exclude mechanical lint nits. Each finding must identify the specific reader
  cost and an actionable direction for improvement.

## Triage and repeat

The parent agent owns fixes and triage; reviewers remain read-only.

1. Reproduce behavioral failures or establish the specific rubric violation.
   Reject unsupported speculation with a short reason.
2. Check relevant code on local `main` and changes since the branch's merge
   base before implementing a finding. Report missing `main` as a limitation.
   Classify introduced versus pre-existing defects; neither is automatically
   excluded. If a fix already exists on `main`, report the evidence and resolve
   integration within existing authorization rather than duplicating it.
3. Fix confirmed findings within the requested scope, including sibling
   instances of the same cause. Run focused regression checks. Resolve routine
   implementation choices autonomously; ask about unresolved product semantics.
4. Track confirmed unrelated defects or separate design work outside the patch.
   If Linear access and authorization to file/comment already exist, search two
   or three distinctive queries and read plausible matches first. Reuse existing
   issues; comment only with materially new evidence. Verify the destination
   (Submilli team/interpreter project for runtime issues), group shared causes,
   and include reproduction, expected/actual behavior, mechanism with file:line,
   status on `main`, fix sketch if known, and originating issue/PR if available.
   Do not set labels, priority, or estimates unless requested. This review skill
   alone does not authorize external writes or closing issues. Without access
   or authorization, report the finding and filing limitation in the handoff.
5. Carry every disposition and the latest delta into another complete round
   after fixes. Stop successfully only when a complete round has no new
   confirmed findings and no unresolved in-scope findings. Filing an issue is
   not a substitute for fixing a defect within the PR's scope.

After about ten rounds without convergence, or when a required decision or check
is blocked, stop and report remaining findings and reasons. Do not claim a clean
review or open a PR while the gate is blocked. Unrelated findings may remain
explicitly tracked in the handoff without expanding the patch's scope.

## Verify and hand off

After convergence, run the owning repository's required checks from `CLAUDE.md`
once on the final proposed diff. For documentation-only changes, verify relevant
links, paths, and instructions; run documentation-site checks only when that site
is affected. Do not run Rust suites for documentation-only changes.

If final checks expose a regression, fix it, repeat the full review round, and
rerun affected checks. Subsequent changes to the proposed diff require a new
review round and affected verification before creating or updating the PR.

Report scope/base reviewed, rounds, independent reviews or fallback, findings
fixed/rejected/filed/deferred with evidence and links where available, introduced
versus pre-existing defects, checks and compatibility results, and outstanding
work. A clean review requires completed checks and no unresolved in-scope findings.

This skill does not stage, commit, push, create a PR, close issues, or invoke a
shipping skill. Leave changes unstaged and uncommitted, preserving existing user
staging. A separately authorized PR workflow can continue after this gate passes.

This file is the canonical review workflow. The Claude skill at
`.claude/skills/launch-review-agents-loop/SKILL.md` delegates here; maintain the
workflow in this file.
