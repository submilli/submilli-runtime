# Packages: chapter briefs

Drafts under `docs/next/packages/`, Part 3 of the target structure in
`~/.claude/plans/diataxis-docs-plan.md`. Every page is a Diátaxis how-to
guide: one paragraph on why the reader needs it, then "This guide shows
you how to…" with the steps, the example named as substitutable, the
commands in the order they are run with what each prints. Explanation
stays beside the output it explains; options go to the reference by a
"Refer to…" link. One thread runs through the part: `@acme/billing`,
the package Parts 1 and 2 granted, built from the scaffold up, with the
support package as its dependent.

## The example service

Doron chose Stripe's test mode (2026-10-01) so that every run is real
while the package keeps the name, operation, and fields Parts 1 and 2
use. `applyCredit` reads the customer's `metadata.class` from Stripe
(`GET /v1/customers/{id}`), checks `{ customerId, customerClass,
amount }`, and posts a negative customer balance transaction (`POST
/v1/customers/{id}/balance_transactions`), which Stripe records as a
credit. Two test customers were created for the book: Northwind
(`cus_VMQR3azuTWVAWs`, class premium) and Initech (`cus_VMQRgpQKYgN52y`,
class standard). The key is `STRIPE_SECRET_KEY` in the repository's
`.env`, loaded into the scratch store and the scratch project's `.env`
without being printed. Part 2 page 1 still shows the fictional
`billing.internal.example.com` host in its `add-package` output; update
it to `api.stripe.com` with GET, POST, and the secret when Part 2 is
revisited.

## Start a project

- **Purpose:** The scaffold, its layout, the manifest written for the
  model, `build check` on the scaffold, a second package, and the editor
  files, with where the editor's view stops.
- **Evidence:** `package-anatomy` ("Start a project", "The manifest",
  "Build it"), `editor-setup` (all sections, condensed to one). Every
  output is from one scratch run with CLI 0.1.6: the doc-comment
  warning, the manifest. The `build init` and `build new` outputs are
  from the development build after the change made on 2026-10-01 at
  Doron's request: both commands now scaffold `README.md` beside
  `docs/readme.md` and report every file they create (the released
  0.1.6 prints only the manifest, the entry point, and the test). Paths
  are shortened to `…/acme/`. The
  compile error is real: a `broken()` function with a wrong return type
  appended to the support package's source for one run.

## Export a function

- **Purpose:** Tag and `check`, the four field forms, the two warnings,
  the payload (scope, looked-up facts, normalize before the check, read
  once), the service call (host in a constant, the secret by name, never
  return it, own types), `build check` clean, the derived file, the
  whole file. Merged from the plan's Export a function, Design the
  payload, and the service-call half of Use the standard library
  (Doron, 2026-10-01).
- **Evidence:** `package-anatomy` ("The source", "Capabilities and
  check", "The payload says what the operation means", "Calling a
  service", "Build it"). The page builds the function in three stages
  that each compile (Doron, 2026-10-01: "we want to end up with
  something that compiles"): the tag and `check` with a placeholder
  return; the trimmed id and a stand-in `lookUpClass` returning
  "standard"; the real request. Each stage was built for real, and the
  two warnings were captured on stage one's file: `amount` left out of
  the tag (line 19, in the tip box Doron asked for) and the nested
  `check` (line 19). The host-from-parameter warning is from a variant of
  the final file (`base` passed as a parameter). `capabilities.yaml` is
  the real derived file: the host was derived through the
  `customerPath` and `requestHeaders` helpers. The "Add it to a
  blueprint" section that briefly lived here (`publish-local`, `init`,
  `add-package`, `capability list`) moved to Publish on 2026-10-01 at
  Doron's request, so this page does one thing and Publish owns the
  blueprint flow; Build it ends with a pointer there.

## Document the package

- **Purpose:** Doc comments as the printed API, `docs/readme.md` for the
  model, the compiled examples with a real failure, the people's readme,
  keeping them in step.
- **Evidence:** `package-anatomy` ("The readme"), `testing-packages`
  ("Readme examples are compiled"), `curated-packages` ("Choose a
  package", "Supply credentials", for what the people's readme
  carries). `submilli docs` output, the `example 1 (compile)` line, and
  the `refund` failure are real (the failure's second error, "unresolved
  identifier", was trimmed). Both readmes are the scratch project's.
  `submilli docs` prints declarations only; the readme reaches the model
  through its documentation tool, per the Packages explanation.

## Add a dependency

- **Purpose:** Import, the three forms, the lockfile and the SSH form,
  what the dependency adds to `requires` and to the blueprint.
- **Evidence:** `package-anatomy` ("Dependencies"). Real: the
  not-found error (0.1.6 says "package … not found", not the old
  chapter's "declare dependency…"), the sibling build, support's
  `capabilities.yaml`, `add-package @acme/support`, the denial inside
  the billing package, `add-package @acme/billing`, "credited $15", the
  whole file. Not run: the local-store and GitHub forms (the runtime
  repository is private, so an unauthenticated fetch of `@submilli/jina`
  at the v0.1.6 commit answers 404; `submilli.lock` was never written).
  The private-repository passages follow the SUB-1229 implementation in
  the `submilli-wt1` worktree (uncommitted on 2026-10-01): the SSH URL
  `git@github.com:org/repo.git` in `[dependencies]` and for `install`,
  your ssh-agent or `~/.ssh` keys locally, the server's deploy key
  (`package_ssh_key_file`, printed by `server packages ssh-key`) on a
  server, one deploy key per repository. Wording moved from that
  worktree's package-anatomy, CLI, and server chapters; not run here.
- **Findings:** (1) `add-package` for a package with a dependency adds
  only that package's rules; the dependency's own `requires` are not
  added and lint passes, so the first run is refused inside the
  dependency (`caller=@acme/billing capability=secrets.get`). Doron
  called it a bug (one `add-package` should add the tree); filed as
  SUB-1235 in the launch project on 2026-10-01. At Doron's request the
  page is written for the fixed behavior: one `add-package @acme/support`
  lists both packages and writes both caller lists. That output and the
  resulting `blueprint.yaml` are expected, assembled from the two real
  outputs of 0.1.6 (`add-package @acme/support`, then `add-package
  @acme/billing`); the secret commands and `credited $15` are real.
  The frontmatter says so; re-run and re-capture when SUB-1235 lands. (2) The
  denial's stack prints `source context unavailable: invalid source
  position 7:44` for the `apologize` frame in the dependent package;
  noted in SUB-1235. The page trims the frame list to one line each.

## Write tests

- **Purpose:** A test file and the three helpers, labels and the first
  failure, the live test with `--env-file .env` and the skip, what tests
  don't prove. "Tests that call the service" follows SUB-1237 and
  SUB-1238 (Doron, 2026-10-01: `build test` must not read the environment
  or `.env` by default, and skipping network tests must be an option, not
  a variable), merged the same day as PR #43. Every output in that
  section was re-captured with the merged build from `main`
  (`target/debug/submilli`): the plain run fails naming the secret
  although `.env` is present, `--env-file .env` runs the live test
  against Stripe (one duplicated `secrets.get` line dropped),
  `--skip-network` leaves it out (path shortened). The live test has no
  `secrets.get` guard (Doron: a test that passes without its key hides
  configuration mistakes). The merged build prints stack frames as
  `@acme/billing/lib:62:25`; the page's frames follow it, context lines
  trimmed.
  The "What to test without the service" section (export request
  builders so tests can reach them) was dropped on 2026-10-01: Doron
  objected to exporting internals for tests and filed the fix as
  SUB-1236 (tests may import any file in the package); the issue lists
  the docs to update when it lands, this page first.
- **Evidence:** `testing-packages` (all sections but the coding-agent
  one). Every output is real and was re-captured after `customerPath`
  stopped being exported: the one-label file, the failing file (`lib.test.ts`
  temporarily replaced by a three-label version with a wrong expectation
  about the negative-amount message, run with `network.test.ts` moved
  aside, then both restored; the output is verbatim but for one empty
  context line), the
  live run against Stripe with the `[security]` lines (two duplicated
  `secrets.get` lines were dropped), the skip line (path shortened). The
  live test credits 100 cents to Northwind in test mode on every run.

## Publish a package

- **Purpose:** `publish-local`, search and docs, add it to a blueprint
  (`init`, `add-package` with the secret warning, `capability list`, and
  the reading of `requires` as the package's rules and `provides` as what
  `main` can be granted, moved here from Export a function), finish the
  blueprint and run the allowed case (the denial was dropped on
  2026-10-01, Doron: "we have showed it enough"; one sentence points to
  Start a blueprint for it), installing from the repository (public and
  SSH), the server.
- **Evidence:** `package-anatomy` ("Publish it"), `cli` ("Install a
  package"), `server` ("Install packages"). Real: `publish-local` (the
  store path shown as `~/.submilli/packages/…`), `search`, the blueprint
  commands and their outputs (the `secret put` prompt line follows the
  old chapter; the value was piped), both runs (balance −1600 after the
  live test's 100 and this 1500), and the server section on the local
  `submilli-server` 0.1.6 (`remove` then `apply` so the output is
  "Added"; the balance there is after the dependency page's run and the
  earlier server run). Not run: `submilli install` of the book's
  repository (fictional). The local SSH install (`git@github.com:…`, your
  ssh-agent or `~/.ssh` keys) stays on this page, from the worktree's
  CLI chapter, not run; the server's deploy key was cut on 2026-10-01
  in favor of one pointer to the new Part 4 page, Install private
  packages. The
  server's `packages install` was tried against the runtime repository
  and answered 404 because it is private; the page describes it and
  links to Register a blueprint instead of quoting output.

## Verification

- CLI 0.1.6 in a scratch project `acme/` under an isolated
  `SUBMILLI_HOME`, with the Stripe key in the scratch store and the
  project's `.env`. Every command output on the six pages is from those
  runs unless the page's note above says otherwise.
- `submilli docs submilli:security` and `submilli docs submilli:test`
  answer "unknown package" in 0.1.6 (the package-only and test-only
  modules are not listed); the pages name them without quoting docs.
- The scratch server's earlier secret-store key file was overwritten
  during this session; the store was moved aside and recreated, so the
  Part 2 blueprints registered on that server no longer have their
  secrets. Nothing on the committed pages depends on that server state.
