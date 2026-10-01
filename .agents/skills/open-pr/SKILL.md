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

1. Resolve the active checkout and read `AGENTS.md` and `CLAUDE.md`. Record the
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

Read each linked issue's title, URL, team, and current state, and discover the
team's actual review workflow state (often `In Review`). Do not invent a state
name or ID or substitute `Done`. Resolve a missing or ambiguous review state
before publishing when possible. If Linear access is unavailable, retain known
IDs in commits/PR text, report metadata as unverified, and report the status update
as blocked; do not claim the entire workflow completed. Do not block independent
code preparation or PR creation merely because Linear is unavailable.

## Review and verify

1. Select focused and final checks using the Verification section of `CLAUDE.md`
   and the entire proposed diff. Explain whether compiler/runtime implementation,
   behavior, inputs, dependencies, or conformance coverage changed. Run
   `SUBMILLI_FULL_TEST=1 cargo test --workspace` only when that impact requires it.
   Other Rust changes use `SUBMILLI_FULL_TEST=0 cargo test -p <affected-crate>`
   plus affected caller/integration checks. Run `cargo fmt --all --check` and
   `cargo clippy --workspace --all-targets -- -D warnings` for Rust source or
   Rust build/dependency/toolchain/lint changes. Use package, chart, and docs
   checks for their respective changes; combine checks for mixed changes.
   Fix formatting before review, without modifying unrelated work.
   For routine Submilli Cargo checks, set `SUBMILLI_SKIP_HTTP_TESTS=1`, including
   full compiler/runtime suites. For package/example checks, pass `--skip-network`
   to `build test`; that command ignores `SUBMILLI_SKIP_HTTP_TESTS`. Follow
   `CLAUDE.md`'s conditional HTTP policy: use `SUBMILLI_SKIP_HTTP_TESTS=0` for
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
2. Read and run `.agents/skills/launch-review-agents-loop/SKILL.md` for all three
   review roles, triage, repeat rounds, and final verification. The review skill
   leaves changes uncommitted; this outer workflow resumes after it passes.
3. Reuse completed check results for the same content, scope, base, and relevant
   environment. Do not repeat full suites just because the review skill returned.
   A rebase onto a changed base requires fresh verification as described below.
   If fixes or hooks alter the diff, rerun the review loop and affected checks.
   A blocked required check, unresolved in-scope finding, or non-converged review
   blocks PR creation, including drafts. Never weaken checks.

## Commit the reviewed changes

- Match recent commit style and keep the subject concise (aim under 70 chars).
  For one linked issue, include its ID and title or a short paraphrase, for example
  `SUB-123: Handle nullable return types`. Strip a leading `N. ` plan-number prefix
  from Linear titles. For multiple issues, use a covering subject and a short
  paragraph per issue in the body, including each ID and what changed for it.
- Every commit created by this workflow must name the linked issues it addresses
  in its subject or body. Ensure every linked issue appears in at least one PR
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
3. Reassess the whole PR diff and verification selection after rebasing. If the
   base or content changed, run the review loop again and all selected final
   checks on the integrated result. Keep full tests conditional on compiler/runtime
   impact, including relevant changes in the newly integrated base. Reassess the
   HTTP-test selection too; retain the skip setting unless the integrated diff
   requires the affected HTTP checks. A no-op rebase
   with identical base/content can reuse prior results. If fixes are necessary,
   commit them with the relevant IDs and repeat review/verification. Amend only
   this workflow's own unpublished commits, never base commits.
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

1. Follow any repository PR template. Describe the concrete problem and resulting
   behavior, link every associated Linear issue by ID and URL when available,
   and include scope, review rounds, finding dispositions, checks/results, why
   full tests ran or were skipped, and unrelated outstanding findings or disclosed
   independent-review fallback. Do not claim unavailable checks passed.
2. Create the PR with explicit base repository/branch and head, or update the
   existing matching open PR rather than creating a duplicate. Use structured
   arguments or a temporary file with `gh pr create/edit --body-file` to preserve
   literal text and newlines. Honor the requested ready/draft state.
3. Read back the PR and confirm it is open, targets the intended base, and has
   the verified head SHA. If any of those changed, reconcile and review/verify
   new content before proceeding. PR creation failure must not move Linear issues.
4. For each linked Linear issue, re-read its current state. If already in the
   discovered review state, leave it there. Otherwise update it to that state
   after confirming the PR, including for a draft unless the user requested a
   different state policy. Do not reopen completed/canceled issues automatically;
   report them for clarification. Never mark issues `Done` here.
5. Confirm each Linear update by reading the issue back. Report per-issue success
   or failure; if an update fails, keep the PR open and report the incomplete step
   and reason. A subsequent invocation should reuse that PR and retry only the
   missing updates when its verified content and base are unchanged.
6. Return the PR URL, verified head/base SHAs, validation summary, rebase attempts
   and conflicts, commit issue IDs, confirmed Linear review states, and remaining
   blockers. If publishing fails, say exactly what was committed or pushed.

This file is the canonical PR workflow. The Claude command at
`.claude/commands/open-pr.md` delegates here; maintain the workflow in this file.
