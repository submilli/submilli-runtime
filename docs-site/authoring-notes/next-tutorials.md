# Tutorials: chapter briefs

Drafts under `docs/next/tutorials/`, in two folders (`with-your-coding-agent/`,
`connect-a-harness/` with its `index.md`) and four loose pages, Part 5 of the target structure in
`~/.claude/plans/diataxis-docs-plan.md`. Every page is a Diátaxis
tutorial (https://diataxis.fr/tutorials/): one paragraph on why, then
"In this tutorial we will…", concrete steps in order, the output each
step should produce, "Notice that…" where the lesson is, no options or
alternatives, explanation kept to a sentence with a link, and a closing
line that says what the reader has built. Pages are read in order within
their sub-tree. The content is moved from the live `harness`,
`blueprints`, `permissions`, `mcp-servers`, `server`, `server-mcp`,
`package-anatomy`, `testing-packages`, and `cli` chapters and from the
quickstart example; new sentences are the openings, the step framing,
and the "notice" lines.

## What was run

Scratch under `scratchpad/tutorials/`, with the release binaries built
from main d199f40 plus the uncommitted SUB-1271/SUB-1274 changes (which
don't touch these paths), each run under its own `SUBMILLI_HOME`:

- `quickstart/` (home `home/`): the quickstart example copied from
  `examples/quickstart`, for Diagnose a denial. Every output on that page
  is from these runs, including the two filter variants.
- `acme-repo/` (home `ci-home/`): the Part 3 Acme project trimmed to
  `@acme/billing`, plus `blueprints/support.yaml` (the Publish page's
  blueprint), `policy/credit.ts`, and `policy/test.sh`, for Verify in CI.
  Every command in the workflow was run locally with the Stripe test key
  from the repo `.env`; the credits are real, in test mode. The workflow
  itself was not run on GitHub: the installer URL is not public yet
  (pre-launch), so a job can't install the CLI. Needs a real run once it
  is, like SUB-1273 for the Linux page.
- `ops/` (home `ops-home/`, port 8139, token file, store key): the
  server for Manage blueprints in Git. `secret put`, the first and
  second `apply`, `list`, and `show` are real. `submilli server packages
  install … --sha` and `submilli install org/repo@sha` were not run (no
  such repository); the `installed @acme/billing @ 3f9c2a1b7e40` line is
  the one Register a blueprint uses, from the live chapter.
- `harness/` then `harness2/` (home `harness-home2/`, port 8138): the
  server for the five harness tutorials. The first run used main d199f40's
  volume form; after main 0924ccc (SUB-1222 merged) the server was
  restarted from the new build with the page's own `server.yaml`
  (`secret_store.key_file`, `volumes.notes` as `managed-local`) and the
  blueprint's `mode: named`, and the five checks (`npm run check`,
  `python check.py`) printed their `ok` lines again, the note landing
  under `server/volumes/notes/u_ada/notes/`. Three agents ran with real models: Vercel (Gemini 3.8
  Flash) and the Claude Agent SDK (Claude) answered and their answers are
  on their pages, trimmed with `…`; deepagents (Gemini) crashed twice,
  see the candidate issue below. OpenAI Agents was not run (no key). The
  Mastra conversation is the live chapter's run, unchanged.
- `github-mcp/` (home `gh-home/`): `blueprint init`, `add-mcp` against
  GitHub's server (the OAuth probe is real), `auth-status`, `secret put`,
  `provider add` with the OAuth app from the repo `.env`, and the
  `authenticate` prompt up to "Waiting for the redirect" are real. The
  login needs a browser; everything after it on the page is unwritten.

Candidate issues found, filed on 2026-10-02 as SUB-1282 and SUB-1283:

- SUB-1282 (MCP file tools answered a denial as a JSON-RPC error,
  crashing deepagents): fixed on main 10da5c4 (de531f9). The deepagents
  page's check description gained the fifth assertion, the harness
  index says the file tools answer as results, and the deepagents
  conversation was run again on 2026-10-03.
- SUB-1283 (lint accepted a filter on a field the operation doesn't
  report): fixed on main 10da5c4 (a331fef); lint now errors. Diagnose a
  denial's missing-field section was rewritten around the real error,
  with `submilli run` showing what the rule does since `run` doesn't
  lint.

## Craft a blueprint

- **Purpose:** What a good result looks like when the assistant writes
  a blueprint: reads first, tests both directions, proves each half of a
  filter, hands credentials back to you, has a verifier look past the
  tool names, checks the server before changing it, and ends with
  questions.
- **Starting point:** Install (the skill), Parts 2 and 4 by title; the
  billing package in the store.
- **Understanding:** the order the assistant should work in; why a
  report that says the request isn't fully met is the right report; that
  credentials never pass through the conversation.
- **Action:** four prompts, each with what to look for.
- **Boundaries:** no commands of its own; every command is on a Part 2
  or Part 4 page. The server step's account is the research blueprint's,
  said so.
- **Evidence:** the four "With a coding agent" accounts from the live
  `permissions` (the Jina pair: "what does the package offer" and
  "docs.python.org and nothing else"), `blueprints` (status API),
  `mcp-servers` (Playwright), and `server` (two users) chapters, moved.
  Rewritten on 2026-10-03 at Doron's request so the prerequisites stop
  sending the reader away: the first step now uses the curated
  `@submilli/jina` package, installed with one command, instead of
  `@acme/billing` from Part 3, so the billing account (session-scoped
  grant, both halves of the filter) is no longer on this page. A public
  Acme repository was the other option Doron raised; `submilli install`
  enforces that a package's scope equals the repository's GitHub owner
  (`InstallError::ScopeMismatch`), so `@acme/billing` can only come from
  an `acme` org, which isn't ours. The "Before you start" outputs
  (`blueprint init`, `install`, `secret put`, `capability list`) are
  real, on main 10da5c4, the install made against the private runtime
  repository with a GitHub token.

## Build a package

- **Purpose:** The Attio run as a lesson: what to look for in the
  package the assistant writes (normalized id, single reads, ownership
  filtering, own types), in its tests (live, under the blueprint, with
  controls), in the verifier's review, and in its report; then the
  no-key tests prompt and the self-correction.
- **Evidence:** `package-anatomy` "With a coding agent" and
  `testing-packages` "With a coding agent", moved; the `listNotes`
  fragment kept because the four checks are visible in it.

## Connect a harness (index)

- **Purpose:** The research agent, the shared server setup, and the
  three things a harness decides, once, so that each harness page can be
  short. Carries the eight tools and the execute result, and the harness
  secrets paragraph (`submilli-secrets`, 409, `rebind`).
- **Deviation from the plan:** the plan had one paragraph and links; the
  shared setup (clone, server.yaml with the volume in SUB-1222's form,
  `publish-local -p @submilli/jina`, `secret put`, `apply`) lives here
  rather than five times.
- **Evidence:** `harness` chapter's first three sections and
  "Credentials that belong to the session", moved; the setup commands
  real, re-run under main 0924ccc.

## Connect Mastra / deepagents / OpenAI Agents / Claude Agent SDK / Use the HTTP API

- **Purpose:** The same agent on one harness: get the project, read the
  agent file with its harness-specific notes, run the no-model check
  (the verifiable result), run one real conversation and know what to
  notice.
- **Evidence:** the harness chapter's section for each, moved; the check
  outputs real; conversations as listed above. Mastra keeps the "Connect
  an existing agent" account, deepagents the "Find a session bug"
  account.

## Diagnose a denial

- **Purpose:** One denial read all the way down: the three fields of the
  message, the reasons table, the rule found by hand, the binding flipped
  to move the denial, the missing-field denial and what `not` does, and
  the rule-or-program decision.
- **Evidence:** all outputs from the quickstart example under `submilli
  run --var`, re-run on main 10da5c4; `permissions` chapter's reasons
  and evaluation tables, moved.

## Verify in CI

- **Purpose:** A job that runs the package tests with the key, lints
  the blueprints against the published package, and runs the policy as
  two sessions through `policy/test.sh`, with two failures shown.
  Doron's ask (2026-10-02): both lint for blueprints and tests for
  packages.
- **Evidence:** every step run locally; workflow not run on GitHub.

## Manage blueprints in Git

- **Purpose:** GitOps for blueprints, Doron's ask (2026-10-02): lint on
  every pull request, apply on every merge to main or on every release,
  packages pinned in `packages.txt` in the same commit, rollback by
  revert, and the one thing the job doesn't do (remove).
- **Evidence:** server outputs real; GitHub-side installs not run.

## Add the GitHub MCP server

- **Purpose:** The GitHub server as the OAuth-provider case: declare,
  register an app, provider, log in, typed tools, read-only set then
  `create_issue`, a program, the server side, per-user token.
- **Status (2026-10-02, late):** Doron fixed the OAuth app's callback
  and approved both logins. Real: the local login, `docs @mcp/github`
  (46 tools, 17 typed by Submilli's GitHub pack), the read-only rule,
  `issues.ts` against rust-lang/rust, the `issue_write` denial, the
  server side under `scratchpad/tutorials/gh-server` (port 8143,
  provider under `mcp_oauth`, PENDING, the unavailable error, the
  server login, ACTIVE, `run-code`), and the per-user header blueprint
  under `scratchpad/tutorials/github-peruser`. "Allow one that writes"
  (2026-10-03) widened the rule to `issue_write` and filed real issues in
  `submilli/test-github-package`, which Doron offered: #5 from a first
  program whose cast declared `html_url` (GitHub's server answers
  `{id, url}`, learned with an `update` call), and #6 from the program
  on the page. The page is complete.

## Folders (2026-10-03)

Doron asked for grouping inside the part: `with-your-coding-agent/` holds
Craft a blueprint and Build a package; `connect-a-harness/` holds the
sub-tree's index as `index.md` and the five harness pages. The four
remaining tutorials stay at the part's root. File numbers keep the
part-wide reading order (01–02, 03–07, 08–11), as do `sidebar.order`
values. Slugs are unchanged, so no link moved. Starlight labels an
autogenerated group with the directory name verbatim, so the cutover's
`astro.config.mjs` must name the two groups ("With your coding agent",
"Connect a harness") with nested `autogenerate` entries rather than rely
on the folder names.

## No clone, no download (2026-10-03)

Doron: readers must not clone the runtime repository, nor download files
from it; every tutorial is built from scratch on the page. The harness
index prints `blueprint.yaml`, `note.ts`, `server.yaml`, and
`summarize.ts` for the reader to save under `harnesses/`, and installs
the package with `submilli install submilli/submilli-runtime
@submilli/jina` (real output). Each harness page starts its project
(`npm init -y`, `npm pkg set type=module`, `npm install …`, or a venv and
`pip install …`), then prints `agent.ts`/`agent.py` and the check file
in full; the checks read `../note.ts`, hence the `harnesses/<dir>`
layout. Verified end to end for Mastra from a clean directory with the
files taken from the page (the `type=module` line came out of that run:
`npm init -y` makes a CommonJS package and the files use top-level
`await`). No URL is pinned to a tag any more, so the release skill's
Compose obligation does not extend to these pages.

## The agent's brief (2026-10-03)

Doron asked for a better system prompt, from his experience with the
Slack assistant's. `examples/harnesses/prompt.txt` holds a cut of it for
the research agent (purpose, work in programs, resources, how to
answer, ~30 lines); the five example agents now read it from the
directory above with `{userId}` replaced, and the index prints it as
step 3. The five checks pass with it. The Mastra conversation on its
page predates the brief; re-run when convenient.

## Anthropic by default, and the brief tightened (2026-10-03)

Doron: Anthropic by default across the tutorials. The Mastra, deepagents,
and Vercel examples now name `claude-haiku-4-5` (`anthropic/…`,
`anthropic:…`, `@ai-sdk/anthropic`); dependency lines, run lines, and
reprinted agent files follow. Doron also asked that the overstep denial
(`fs.list` on `/`) never appear in the tutorials: the brief now says the
notebook directory is the only path the agent may touch and to use no
other file tool for it, and asks for the newest source with its date
checked. Three Claude runs after the change (Mastra traced, Vercel,
deepagents) produced no denial in the server log. The Mastra page's
conversation is the traced Claude run (ten calls, three compile errors
repaired, one model-name error, the work in one program, Rust 1.99.0);
the Vercel page quotes its run's closing lines. Haiku's answers were not
always current (one Vercel run named 1.98.0, one deepagents run 1.85.0,
both from stale search results), so the deepagents page keeps no
excerpt. A trial with `claude-sonnet-5` could not be judged: Jina
started answering `HTTP 402 Payment Required` (quota) during it, which
also blocks any further conversation runs until the key is topped up.

## No check sections (2026-10-03)

Doron: the reader's attention goes to the conversation, which is the
proof; the no-model checks are ours. The "Check it without a model"
sections and the printed `check.ts`/`check.py` files were removed from
the five harness pages; each is now start the project, the agent file,
one conversation. The index keeps `note.ts` as step 2, "Save a test
program", and the two `run-code` runs in "Prove it", which is the
deterministic proof of the binding before any model is involved. The
checks stay in `examples/harnesses/` for CI and for us.

## Tutorials 8–10 start from scratch (2026-10-03)

Doron: every tutorial stands alone. Diagnose a denial and Verify in CI
now open by building the quickstart's offline package and blueprint
(`build init`, the printed `lib.ts`, `publish-local`, the printed
`blueprint.yaml`, the two programs), so neither needs a key or an
earlier page; both were re-run in fresh directories (`scratchpad/tutorials/diag`,
`ci2`) and the outputs match. Verify in CI lost the Stripe secret and
`--env-var`; the workflow has no secrets, and the live-test case is one
sentence pointing at Write tests. Manage blueprints in Git now uses a
`reader` blueprint over `@submilli/jina` with a `site` variable, because
its `packages.txt` must name a real repository: the deploy job's
`submilli server packages install submilli/submilli-runtime @submilli/jina
--sha 6d68ef78a52f` and the lint job's `submilli install …@6d68ef78a52f`
ran for real (`scratchpad/tutorials/ops2`, server on 8139 with a
`github_token_file` since the repository is private until launch), as did
`secret put`, both applies, `list`, and `show`. Add the GitHub MCP server
already started from scratch.

## The Acme example repository (2026-10-03)

Doron's decision: readers get `@acme/billing` with one key-free line,
`submilli install submilli/acme @acme/billing`, from a repository in our
org. That needs an exception to `install`'s scope rule (scope must equal
the GitHub owner), filed as SUB-1306 with the repository. Diagnose a
denial's setup now shows that line without an output and carries a
frontmatter note; everything after it is real (re-run in
`scratchpad/tutorials/diag`). A Jina-based version of the page was
captured in `scratchpad/tutorials/diag2` (denials and lint real, the
allowed reads blocked by Jina's exhausted quota) and set aside in favour
of the Acme route. Verify in CI keeps printing the package, since the
repository under test is where it lives.

## The Acme example repository, final (2026-10-03)

Doron decided against changing the CLI: the package in
https://github.com/submilli/acme is named `@submilli/acme-billing`, which
satisfies install's rule that a scope equal the GitHub owner. The
capability stays `acme.com/charges.list`. Diagnose a denial installs it
with `submilli install submilli/acme @submilli/acme-billing` (released CLI
0.1.6, no token, real output) and every later output was re-run against
it in `scratchpad/tutorials/diag4`; only the package name in the frames
and the capability list changed. The scope-exception branch was deleted
unmerged. Verify in CI still prints its own `@acme/billing`, since its
repository is the one under test and `publish-local` has no scope rule.

## CI split by repository (2026-10-03)

Doron: a reader rarely wants package and blueprint CI together, and the
blueprint half belongs with Manage blueprints in Git. Page 9 is now
Verify a package in CI (`09-verify-a-package-in-ci.md`, slug
`next/tutorials/verify-a-package-in-ci`): the offline `@acme/billing`
built from scratch in the reader's repository, `submilli build test`, the
workflow, a real failure (the customer comparison dropped: the `check`
still passes and only the test catches it), and a short section on
`network.test.ts` with `--env-var` from secrets and `--skip-network` for
forks. Page 10 now carries the lint and the two-session policy test in
its pull-request job, uses `@submilli/acme-billing` from `submilli/acme`
pinned at 88656b81c537 (no key, unlike the earlier Jina reader), and
folds the release trigger, `show`, and rollback into shorter sections.
All outputs real (`scratchpad/tutorials/pkgci`, `ops3`, server on 8139).
Not run on GitHub.

