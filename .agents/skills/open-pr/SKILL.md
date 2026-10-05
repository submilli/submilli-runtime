---
name: open-pr
description: Review, commit with Linear issue IDs, rebase onto the PR base, verify and push the feature branch, open or update its PR, and move linked Linear issues to review.
---

# Open PR

Use this workflow when the user asks to open a pull request. Invocation authorizes
committing the intended changes, rebasing the feature work onto the selected PR
base, pushing a feature branch, creating or updating its PR, and moving its linked
Linear issues to their team's review state after the PR is confirmed open.
It does not authorize merging, pushing to the base branch, force-pushing, or
closing issues. User-supplied scope, base, destination, and ready/draft preference
take precedence; create a ready PR unless draft is requested.

## Establish the proposed change

1. Resolve the active checkout and read `AGENTS.md`. Record the
   branch, status, staged/unstaged changes, untracked files, and remotes. Preserve
   unrelated work and staging; do not stash, reset, or discard it.
2. Determine the intended base repository/branch and writable head remote from
   the request, branch tracking, repository configuration, and existing PR.
   Fetch the selected base branch explicitly into its remote-tracking ref before
   determining the merge base. Do not assume `origin` is the target or push
   destination. Resolve ambiguity before publishing. Use available GitHub tools
   or `gh`; report unavailable credentials or network access without claiming
   success. Inspect any existing remote head before planning a rebase.
3. Identify all commits and intended working-tree changes that would enter the
   PR, including new files. Read the complete diff against the base's merge base;
   do not review only the last commit or current unstaged diff. Do not publish
   unrelated commits or files. Resolve scope ambiguity before committing/pushing.
   If on the base branch or detached HEAD, create a descriptive feature branch
   without disturbing other work. Check for an existing PR for this head/base.
4. With pending intended changes, use the full flow. With only existing commits,
   skip creating a new commit unless metadata needs correction below. If there
   is no proposed diff, report that there is nothing to open; do not manufacture
   an empty commit or PR, even when the branch is behind the base.

## Resolve Linear issues

Use explicit issue IDs first. Otherwise infer candidates from the conversation,
proposed diff, branch commits relative to the fetched base, and, if needed, Linear
search for relevant in-progress work. Confirm that each issue is addressed by
this PR; incidental mentions are not links. Ask when ambiguous. With no candidates,
continue without Linear updates and say no issue was identified.

### Partial no-panic backlog work

For work implementing selected items from SUB-633, treat the parent as tracking
context rather than an issue completed by the PR. Omit its ID and URL from the
PR title and description, new commit messages, and new feature-branch names so
the Linear integration does not link the PR as completing the entire backlog.
Use the concrete change and selected item numbers instead, for example
`Return worker failures and preserve cleanup ownership (item 26)`. Do not rewrite
published history or rename an existing published branch solely for this rule.

Keep the parent issue's workflow state unchanged. Record progress only for the
selected checklist items and inventory sites; a merged subset does not complete
the parent. The commit-ID, PR issue-link, and review-state requirements below
apply to other linked issues, but exclude this parent for partial work. Include
the parent as a linked issue only if the user explicitly requests that and all
of its completion criteria are satisfied.

Read each linked issue's title, URL, team, and current state, and discover the
team's actual review workflow state (often `In Review`). Do not invent a state
name or ID or substitute `Done`. Resolve a missing or ambiguous review state
before publishing when possible. If Linear access is unavailable, retain known
IDs in commits/PR text, report metadata as unverified, and report the status update
as blocked; do not claim the entire workflow completed. Do not block independent
code preparation or PR creation merely because Linear is unavailable.

## Review and verify

1. Plan focused and final checks using the Verification section of `AGENTS.md`
   and the entire proposed diff. Before rebasing, run only affected-area checks
   needed for implementation or review fixes, reusing valid results. Do not run
   full suites until the single post-rebase, pre-PR run below. This timing applies
   to every development task. During development, use
   `SUBMILLI_FULL_TEST=0 cargo test -p <affected-crate> <test-filter>`
   plus affected caller/integration checks. Run `cargo fmt --all --check` and
   `cargo clippy --workspace --all-targets -- -D warnings` for Rust source or
   Rust build/dependency/toolchain/lint changes. Use package, chart, and docs
   checks for their respective changes; combine checks for mixed changes.
   Fix formatting before review, without modifying unrelated work.
   For routine Submilli Cargo checks, set `SUBMILLI_SKIP_HTTP_TESTS=1`, including
   full compiler/runtime suites. Set `SUBMILLI_TEST_NIGHTLY_ONLY=0` for development
   and post-rebase PR checks: conformance, compiler determinism, and expensive
   CLI fuel-accounting tests do not belong in this workflow. The flag is enabled
   by nightly CI and, separately, the release skill. For package/example checks, pass `--skip-network`
   to `build test`; that command ignores `SUBMILLI_SKIP_HTTP_TESTS`. Follow
   `AGENTS.md`'s conditional HTTP policy: use `SUBMILLI_SKIP_HTTP_TESTS=0` for
   affected Rust socket tests; for live package tests, omit `--skip-network` and
   explicitly supply credentials. Enable these only when changed HTTP transport, routing,
   wire formats, authentication, proxy/SSRF policy, external API behavior, or
   relevant dependencies require them. Parser/compiler changes alone do not.
   Test-selection/build-script changes need both modes checked with synthetic
   package tests and a focused localhost test; live APIs are needed only when
   their integration behavior changes. Scope enabled runs to affected tests or
   packages. Socket tests, including localhost mocks, may need execution outside
   the sandbox; request that access only for selected required checks. Report
   skipped/ignored HTTP coverage separately from passing tests and explain the
   selection. In-process handler tests remain enabled.
2. Read `.agents/skills/launch-review-agents-loop/SKILL.md` and check for a
   completed review in the current conversation or handoff, including one run by
   `fix-issue`. Reuse it when its scope covers the proposed changes and its
   completion criteria were met, including the low-priority-only exit. Do not
   launch review cycles again just because `open-pr` was invoked or reviewed
   content was committed. If no completed review covers the change, run that
   skill. New implementation changes or conflict resolutions require renewed
   review; parent-checked final low-priority fixes do not. Record the review
   evidence reused. Reviewers default to reading code, with narrow tests only
   for concrete hypotheses or edge cases. This outer workflow owns full
   verification after rebasing.
3. Reuse completed check results for the same content, scope, base, and relevant
   environment for focused checks. Review completion does not trigger full suites.
   Run full tests once after the final rebase and before opening the PR; reuse
   that run on subsequent invocations unless another rebase integrates new base
   changes. Pre-rebase focused checks do not replace this full run.
   If fixes or hooks introduce unreviewed implementation changes, rerun the
   review loop and affected checks under its completion and reuse rules.
   A blocked required check, unresolved in-scope finding, or non-converged review
   blocks PR creation, including drafts. Never weaken checks.

## Commit the reviewed changes

- Match recent commit style and keep the subject concise (aim under 70 chars).
  For one linked issue, include its ID and title or a short paraphrase, for example
  `SUB-123: Handle nullable return types`. Strip a leading `N. ` plan-number prefix
  from Linear titles. For multiple issues, use one covering subject and list
  the IDs together in the body.
- Keep the message matter of fact: what problem is solved and how. A subject
  alone is usually enough; add at most one short paragraph when context is
  needed. Omit review history, test logs, workflow narration, and file inventories.
  Required issue IDs and attribution trailers are separate from this prose.
- Every commit created by this workflow must name the linked issues it addresses
  in its subject or body, subject to the partial-backlog exception above.
  Ensure every linked issue appears in at least one PR
  commit message. For existing commits missing IDs, amend/reword only unpublished
  feature commits owned by this task; do not rewrite base commits or others' work.
  If that cannot be done safely, report the metadata blocker before publishing.
- Include the active agent's appropriate co-author attribution. For Codex use
  `Co-Authored-By: OpenAI Codex <noreply@openai.com>`; Claude follows its own
  attribution convention rather than claiming Codex authored its work.
- Stage explicit intended paths/hunks only. Never use `git add -A` or `git add .`.
  Preserve unrelated staging, including unrelated hunks in shared files. Resolve
  an unclear untracked file's purpose before staging. Inspect the actual commit
  contents to ensure no unrelated staged work was included.
- Honor hooks; never use `--no-verify`. If a pre-commit hook fails, fix the cause,
  review/check any changed content, re-stage explicit paths, and create the commit
  anew. Do not amend an older commit to paper over a failed commit attempt.

## Rebase, verify, and push

After committing (or selecting existing commits), repeat this sequence up to five
attempts if the base advances. Use the selected PR base, not a hard-coded remote.

1. Rebase only with a clean index/worktree. If unrelated edits or staging remain,
   create an isolated worktree and temporary feature branch at the intended
   commit and perform integration there. Read that checkout's guidance; preserve
   the original checkout/index and report where rebased work lives. Do not move
   a branch checked out elsewhere, stash user work, or switch to the primary
   checkout. The selected remote PR head remains the publishing destination.
2. Fetch the base explicitly, record its SHA, and rebase the feature work onto
   that exact base commit. If conflicts occur, inspect both histories and combine
   their intent; stage resolutions explicitly and continue. Ask about non-obvious
   semantic choices. If progress is impossible, abort this workflow's rebase and
   report the blocker; never abort a pre-existing user operation.
3. Reassess the whole PR diff and verification selection after rebasing. A
   rebase without conflicts reuses the completed review; do not run review cycles
   again solely because the base, commit SHAs, or diff context changed. If there
   were conflicts, review the resolved result using the review-loop skill. New
   implementation fixes also require renewed review under that skill's rules.
   Resolve findings with only affected-area checks before the full-test gate.
   Run the full-test commands in `AGENTS.md` once on the integrated result, after
   the final rebase and before opening the PR. If the rebase is a no-op and no
   post-rebase full run exists yet, run it now. Otherwise, only another rebase
   integrating new base changes permits a new full run. New fixes, commits,
   reviews, or repeated invocations do not. Reassess HTTP-test selection; retain
   the skip setting unless the integrated diff requires affected HTTP checks.
   If the full run exposes a failure, fix it, run only affected checks, and
   complete any required review. Report the original full-run result and focused
   follow-ups accurately; do not rerun the full suite for those fixes.
   Commit fixes with relevant IDs; amend only this workflow's own unpublished
   commits, never base commits.
4. Inspect final status, commit contents, and the full PR diff. Record the verified
   head with `git rev-parse HEAD`. Refresh the base again; if it advanced, restart
   from step 2 and re-verify. Stop after five attempts with an explicit blocker.
5. Push that exact verified SHA to the selected remote feature branch, for example
   `git push <head-remote> <verified-sha>:refs/heads/<head-branch>`. Configure
   tracking for the local feature branch when appropriate. Never push to the base
   branch, use force (including `--force-with-lease` or a `+` refspec), or reset hard.
   Rebasing an already-published branch may need a non-fast-forward update: detect
   this before pushing and stop to resolve the publishing strategy with the user.
   Do not overwrite others' commits or silently create a replacement PR. For any
   push rejection, inspect and report the cause; do not blindly retry or force it.
6. Read back the remote feature ref and require its tip to equal the verified SHA.
   If it differs, inspect the extra changes and review/verify them before proceeding;
   ancestry alone does not prove the PR contains only verified changes. Report
   confirmation failures before creating the PR or changing Linear states.

## Open the PR and move issues to review

1. Keep the title and description short and matter of fact. Explain the problem
   and the solution in two short paragraphs or a few bullets, then link the
   associated Linear issues (subject to the partial-backlog exception above).
   Aim for under 150 words. Follow required repository template fields concisely.
   Include a compatibility or rollout note only when readers need it to use or
   deploy the change. Omit review cycles, agent names, finding dispositions,
   commit SHAs, rebase history, test transcripts, and exhaustive file lists.
   Keep review and verification evidence in the working handoff for reuse.
   If a template requests testing, use one brief summary and disclose material
   skipped or blocked coverage accurately; never claim unavailable checks passed.
2. Create the PR with explicit base repository/branch and head, or update the
   existing matching open PR rather than creating a duplicate. Use structured
   arguments or a temporary file with `gh pr create/edit --body-file` to preserve
   literal text and newlines. Honor the requested ready/draft state.
3. Read back the PR and confirm it is open, targets the intended base, and has
   the verified head SHA. If any of those changed, reconcile and review/verify
   new content before proceeding. PR creation failure must not move Linear issues.
4. For each linked Linear issue, excluding the partial-backlog parent above,
   re-read its current state. If already in the
   discovered review state, leave it there. Otherwise update it to that state
   after confirming the PR, including for a draft unless the user requested a
   different state policy. Do not reopen completed/canceled issues automatically;
   report them for clarification. Never mark issues `Done` here.
5. Confirm each Linear update by reading the issue back. Report per-issue success
   or failure; if an update fails, keep the PR open and report the incomplete step
   and reason. A subsequent invocation should reuse that PR and retry only the
   missing updates when its verified content and base are unchanged.
6. Return the PR URL and any remaining blockers or incomplete Linear updates.
   Keep the detailed verification and publishing record in the working handoff.
   If publishing fails, say exactly what was committed or pushed.

This file is the canonical PR workflow. The Claude command at
`.claude/commands/open-pr.md` delegates here; maintain the workflow in this file.
