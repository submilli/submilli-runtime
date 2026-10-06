---
name: launch-review-agents-loop
description: Run iterative clean-code, correctness, and edge-case reviews of the proposed diff, triage findings, and verify the result. Required before creating any pull request in this repository.
---

# Launch review agents loop

Review the proposed change until a complete round reports no confirmed findings
above low priority. Fix confirmed in-scope findings, but a round with only low
priority findings does not require another round after those fixes. Issue IDs
are optional; use the user's requirements and intended behavior when absent.

## Scope and prepare

- Resolve the active checkout with `git rev-parse --show-toplevel` and read its
  `AGENTS.md`. Stay in that checkout, including linked worktrees.
- Record branch, status, staged and unstaged changes. Preserve unrelated work
  and existing staging. Identify the intended PR base and review the whole
  proposed diff: committed changes since its merge base, plus intended staged,
  unstaged, and untracked files. Do not assume `git diff` alone covers a PR.
  If the base cannot be determined, resolve it before claiming complete coverage.
- Reuse existing focused verification evidence; do not run tests merely to start
  a review round. Review by reading code and existing results by default. A
  narrowly scoped test or scratch reproduction is allowed to resolve a concrete
  correctness question or edge case. Never run full workspace, fixture, package,
  or conformance sweeps in this workflow; full verification belongs to `open-pr`
  after rebasing onto main (or the selected PR base).
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
   where applicable. Apply the no-panic review below to execution-path changes.
3. **Edge cases and coverage:** probe boundaries, error paths, sibling sites,
   and regression-test gaps with concrete counterexamples. Challenge the
   compatibility evidence where applicable; verify no-panic failure handling
   and cleanup where execution is affected.

Give every reviewer a self-contained packet containing:

- Exact checkout, requirements, intended behavior, root cause when applicable,
  changed files, full proposed diff, invariants, and useful sibling implementations.
- This skill's rubric, applicable repository guidance, and resolved paths to
  relevant design documents. Do not require private documents for this public
  repository or assume personal skills are installed.
- Concrete hypotheses, checks and results (including differential evidence),
  changes since the last round, and prior findings with their dispositions.
- For execution-path changes, the affected no-panic boundaries, error categories,
  resource/cleanup invariants, and evidence for failure paths. Distinguish
  source-triggered reproducers from injected internal-state failures.
- Explicit read-only scope: no source edits, staging, stashing, checkout,
  restore, rebase, or Git-ref changes. Scratch reproductions belong outside
  tracked source. Instruct reviewers not to run tests by default. Allow only
  narrow checks for a specific hypothesis or edge case, with the reason and
  result reported. Prohibit all full-suite sweeps, including the first run.
  Reuse supplied results instead of repeating checks. Check-generated build
  artifacts are allowed.
- Request ranked findings with priority (P0 critical, P1 high, P2 medium, P3 low),
  file:line, evidence, expected versus actual
  behavior for bugs, and introduced versus pre-existing classification.
  Clean-code findings require a concrete rubric violation, not a reproduction.
  An empty findings list is valid.

### No-panic review

For execution-path changes, apply the canonical
[no-panic policy](../../../AGENTS.md#no-panic-execution-paths) to new/changed code
and directly affected callers and mechanisms. This includes compilation, setup,
Rust host operations, diagnostics and cleanup, not only guest execution.

- Identify explicit panic macros, panicking `unwrap`/`expect`, assertions and
  rethrown panics. Assess construction, explicit validation or API guarantees
  before calling a site a violation; document why callers, mutation or callbacks
  cannot invalidate an accepted invariant. Poisoned std-lock access is permitted
  by AGENTS.md and needs its own documented disposition. "Impossible" or lack of a
  reproducer is not proof. Keep real input/operational/resource failures fallible;
  do not require error plumbing solely for proven invariants. Test assertions are
  allowed; inspect contracts rather than flagging fallible `unwrap_*` by name.
- Inspect implicit failures: indexing/slicing, arithmetic/conversions, borrowing,
  runtime-context requirements, recursion/drop, allocation sizes and dependency
  preconditions. For each finding, identify the operation and violated assumption;
  a text match alone does not prove a panic or input reachability.
- Trace returned errors through every affected entry point. Reject ignored
  errors, fabricated defaults, partial compiler output and accidental conversion
  of internal host/setup/ABI failures into guest-catchable exceptions. Preserve
  ordinary guest error semantics and diagnostic/source context.
- Verify cancellation and cleanup still run before resources are released,
  including blocking workers, store/VFS ownership, budgets and idempotency.
  Treat panic containment as defense in depth, not a replacement for this policy;
  native stack overflow and allocator aborts need prevention/resource bounds.
- Require focused boundary/failure evidence appropriate to the change, including
  a healthy subsequent request when shared state is affected. Run potential
  process-abort reproducers in bounded child processes. Request debug/release and
  production-sized-stack coverage when changing recursion or stack limits.

An explicit panic is not automatically a violation. Once a violation is confirmed,
a guest-input reproducer is not required: report the execution path, missing or
invalid guarantee, and evidence; label exploit reachability unproven when needed.
Track unrelated pre-existing violations
in SUB-633 as required below; do not expand every review into the full no-panic
backlog or clear an in-scope violation merely by filing it elsewhere. Do not
claim panic freedom from passing tests, `catch_unwind`, or a clean search alone.

### Record existing panic sites in SUB-633

When a review encounters a confirmed existing production execution-path policy
violation, the parent agent must ensure it is recorded
in [SUB-633](https://linear.app/submilli/issue/SUB-633/no-panic), even if fixing it
is outside the current patch. Reviewers remain read-only. Invoking this review
skill authorizes the parent to update SUB-633 and its attached inventory and to
reopen it when unresolved findings require further work. This is a narrow
exception to the external-write restrictions below, not permission to create
other issues, change unrelated metadata, or close SUB-633.

Record newly accepted invariant/poison sites and their guarantees in the matching
ledger entry too; they are accepted sites, not removed panics. Do not reopen a
completed item solely because an accepted operation still uses panic syntax.
Reuse accurate existing dispositions rather than duplicating them.

1. Confirm the operation and execution path, distinguish a policy violation from
   proven input reachability, and check the current integration base as well as
   the working branch. Test-only assertions and fallible APIs are not findings
   merely because their names match a panic search.
2. Read the current issue, relevant comments and attached inventory before
   writing. Reuse the matching item/site and add only materially new evidence.
   If already recorded accurately, report the existing entry rather than adding
   a duplicate. For an uncovered site, add it to the relevant item/inventory;
   append a new unchecked item only when no existing item covers the work.
3. Record file:line and revision, the panicking operation or implicit failure,
   affected entry point, invariant, reproduction or injected-state evidence when
   available, status on the integration base, and a fix direction if known.
   Include the originating review/PR when available. Preserve stable numbering
   and baseline references; use narrow patches and re-read changed anchors so
   concurrent edits are not overwritten.
4. If a matching item/file was marked complete but still has unresolved work,
   uncheck the affected entry and explain the evidence. If SUB-633 is closed or
   completed and the finding remains unresolved on the current integration base,
   discover the team's actual open/backlog state and move SUB-633 there. Keep an
   already-open issue's state. Do not reopen solely because a stale working branch
   lacks a fix already present on the base; record the integration need instead.
   If the base cannot be verified, record that uncertainty before claiming the
   completed work has regressed or changing completion state.
5. Read back each update and state change. Report the linked entry, additions or
   deduplication, and any reopening in the handoff. If Linear access, a required
   state transition, or write verification is unavailable, report the pending
   update explicitly rather than claiming it was recorded or reopened.

Recording an unrelated finding does not require implementing it in this patch.
An in-scope violation must still be fixed before the review can pass. A failed
tracking update leaves the review workflow incomplete; do not silently discard
the finding or claim all required review steps completed.

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

1. Reproduce behavioral failures, establish the specific rubric violation, or
   demonstrate a concrete no-panic policy violation as described above. Reject
   unsupported speculation with a short reason.
2. Check relevant code on local `main` and changes since the branch's merge
   base before implementing a finding. Report missing `main` as a limitation.
   Classify introduced versus pre-existing defects; neither is automatically
   excluded. If a fix already exists on `main`, report the evidence and resolve
   integration within existing authorization rather than duplicating it.
3. Fix confirmed findings within the requested scope, including sibling
   instances of the same cause. Run only regression checks for the affected
   areas, including directly affected callers; do not rerun unrelated tests or
   full suites after fixes. Reuse still-valid results. Resolve routine
   implementation choices autonomously; ask about unresolved product semantics.
4. Track confirmed unrelated defects or separate design work outside the patch.
   For existing no-panic violations, follow the mandatory SUB-633 workflow above.
   For other findings, if Linear access and authorization to file/comment already
   exist, search two or three distinctive queries and read plausible matches
   first. Reuse existing
   issues; comment only with materially new evidence. Verify the destination
   (Submilli team/interpreter project for runtime issues), group shared causes,
   and include reproduction, expected/actual behavior, mechanism with file:line,
   status on `main`, fix sketch if known, and originating issue/PR if available.
   Do not set labels, priority, or estimates unless requested. This review skill
   authorizes only the SUB-633 updates and reopening specified above; other
   external writes require separate authorization. It never authorizes closing
   issues. Without access or authorization, report the finding and filing
   limitation in the handoff.
5. Evaluate the complete round across all three roles after triage. If it had
   any confirmed P0/P1/P2 findings in scope, fix them and run another complete
   round, carrying forward dispositions and the latest delta. If it had no
   confirmed findings or only P3 (low priority) findings, stop after fixing the
   in-scope findings and checking those fixes directly; do not launch another
   round merely to get an empty findings list. Run only affected checks as needed.
   Earlier unresolved in-scope findings still block completion. Classify priority
   by impact, not the desire to stop; required behavior and no-panic violations
   must not be downgraded to cosmetic nits. Filing an issue is not a substitute
   for fixing a defect within the PR's scope.

After about ten rounds without convergence, or when a required decision or check
is blocked, stop and report remaining findings and reasons. Do not claim a clean
review or open a PR while the gate is blocked. Unrelated findings may remain
explicitly tracked in the handoff without expanding the patch's scope.

## Verify and hand off

After convergence, confirm that affected areas have focused verification for the
final changes, running only missing or invalidated checks. Do not run a full suite
as a review exit gate. Defer full verification to `open-pr` after its rebase; a
clean review does not claim that the later PR verification has passed.
For documentation-only changes, verify relevant
links, paths, and instructions; run documentation-site checks only when that site
is affected. Do not run Rust suites for documentation-only changes.

If final checks expose a regression, fix it, repeat the review round, and rerun
affected checks. New implementation changes or conflict resolutions after review
require renewed review. Fixes from the final low-priority-only round need only
parent inspection and affected verification. Committing unchanged reviewed
content and a rebase without conflicts do not require another review cycle.

Preserve a review completion record in the handoff: reviewed scope and base,
commit or working-tree diff identity, roles/rounds completed, findings and their
priorities/dispositions, final low-priority fixes checked by the parent, and
focused verification results. `open-pr` must reuse this completed review when
it covers the proposed changes, including after a conflict-free rebase; changed
commit SHAs alone do not invalidate it. Report whether completion followed an
empty round or a low-priority-only round, rather than claiming zero findings.

Report scope/base reviewed, rounds, independent reviews or fallback, findings
fixed/rejected/filed/deferred with evidence and links where available, introduced
versus pre-existing defects, checks and compatibility results, and outstanding
work. Report full verification as deferred to `open-pr`. A clean review requires
completed applicable focused checks and no unresolved in-scope findings.

This skill does not stage, commit, push, create a PR, close issues, or invoke a
shipping skill. Leave changes unstaged and uncommitted, preserving existing user
staging. A separately authorized PR workflow can continue after this gate passes.

This file is the canonical review workflow. The Claude skill at
`.claude/skills/launch-review-agents-loop/SKILL.md` delegates here; maintain the
workflow in this file.
