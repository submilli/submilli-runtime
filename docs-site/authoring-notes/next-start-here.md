# Start here: chapter briefs

Drafts under `docs/next/start-here/`, assembled from the existing chapters
per the target structure in `~/.claude/plans/diataxis-docs-plan.md`. Read in
order; each chapter assumes only the ones before it.

## Why Submilli

- **Type:** explanation.
- **Purpose:** Make the case that a program an agent writes needs rules
  about what it may do, and that isolation alone can't enforce them.
- **Starting point:** Tool calling, prompt injection, sandboxes. No Submilli
  vocabulary.
- **Understanding:** Why agents now write programs; why that program is
  untrusted; why a sandbox can't judge what an allowed call means; what a
  rule has to be about (operation, arguments, who the session is for).
- **Action:** None. Decide whether to read on.
- **Boundaries:** One blueprint example, defined in one sentence. What a
  blueprint, a package, and the server are is left to chapters 4–6.
- **Evidence:** Moved from `introduction` unchanged, minus "What you're
  installing" and "Try it", which chapters 4–7 now cover.

## Install

- **Type:** how-to.
- **Purpose:** Get the CLI, the server, and the skill onto the machine, and
  show they work.
- **Starting point:** A terminal.
- **Action:** Install; run four commands that prove each piece works;
  install the skill for one assistant and check its status.
- **Boundaries:** No explanation of what the server or the skill does beyond
  one sentence each. Skill update mechanics and team workflows belong in
  reference.
- **Evidence:** Commands from `quickstart` "Install" and `skill` "Install"
  and "Updates". See verification below.

## Quickstart

- **Type:** tutorial.
- **Purpose:** The reader builds the scenario by hand and sees one rule
  refuse one call.
- **Starting point:** Chapter 1 and an installed CLI.
- **Action:** Write a blueprint, then the package it governs, publish it,
  lint the blueprint, start the server, register the blueprint, run two
  programs through a forty-line application, see the second refused.
- **Boundaries:** Blueprint before package. One sentence per new word; no
  explanation of why. The coding-assistant setup moved to Install. The
  real-agent run is a pointer to `examples/quickstart`.
- **Evidence:** Every code block and output is moved from `quickstart`
  unchanged. Sections reordered: blueprint, package, lint, application. The
  blueprint's intro sentence changed from "references schema generated from
  the package we just compiled" to "names an operation the package in the
  next step will provide". Lint stays after `publish-local`.

## Blueprints

- **Type:** explanation.
- **Purpose:** The file the reader just wrote, in full.
- **Starting point:** The quickstart's blueprint and the words it
  introduced: blueprint, package, server, rule, customer id.
- **Understanding:** Default deny and how a call is decided; rules are
  stated in the package's vocabulary; a variable makes one file serve every
  customer and the program can't read or change it; each caller has its own
  list; secrets are declared by name and never readable by the program;
  files, state, and models are granted here; what no rule can grant.
- **Action:** None yet; the how-tos in Part 2 follow.
- **Boundaries:** No CLI commands, no filter grammar beyond one sentence, no
  vfs mode table. Where the vocabulary comes from is chapter 5.
- **Evidence:** Assembled from `blueprints` (intro, "Start from nothing
  allowed", the rule paragraphs, session and vfs sentences),
  `how-submilli-works` ("Semantic security", "Blueprint variables", "Trusted
  code and untrusted code", "Secrets and variables stay on the trusted
  side"), and `permissions` ("Refusals no rule can change"). New writing:
  the opening two paragraphs (a blueprint is the plan the server builds each
  run's environment from, as a blueprint is to a house or an image to a
  container; Doron's framing), "What the plan says" (one sentence per
  block), and the secret-sources sentence.

## Packages

- **Type:** explanation.
- **Purpose:** Where operations come from and why there is no other way out.
- **Starting point:** Chapter 4.
- **Understanding:** A package is an npm-style library built for agents;
  why not npm (OS access, no semantic security); packages versus MCP
  servers; where packages come from; what an operation looks like from the
  inside and how it hands typed facts to the runtime; what the agent's
  program sees; the tools for building one.
- **Boundaries:** No project layout, manifest, build commands, or tests
  (Part 3). No MCP declaration details (Part 2).
- **Parked** (dropped from this page at Doron's request, still in the old
  chapters until cutover): "Generated code reaches the outside only through
  operations" (WebAssembly host functions, the `fetch` error, the
  `exfiltrate.ts` denial, the security aside) fits The server or a Part 3
  page; "The runtime knows who is asking" and "Credentials live in the
  package" fit the Part 3 how-tos "Write an operation" and "Call a service".
- **Evidence:** Assembled from `how-submilli-works` ("Generated code reaches
  the outside only through operations", "Context can include facts outside
  the request", the caller paragraph of "Trusted code and untrusted code",
  "Its own packages in place of npm"), `package-anatomy` ("Capabilities and
  check", first three paragraphs), and the intros of `mcp-servers` and
  `curated-packages`. New writing: the opening (a package is a library
  around one of your systems, like an npm package except each function
  asks the blueprint first), the four-part list, and "Credentials live in
  the package" (condensed from `package-anatomy` "Calling a service").
  "Why not npm" moved up front as the contrast. The caller paragraph is
  reduced to the guarantee; the compiler-stamp mechanism is left out.

## The server

- **Type:** explanation.
- **Purpose:** What the process the reader started does with a program.
- **Starting point:** Chapters 4 and 5.
- **Understanding:** Why a WebAssembly instance per run beats a microVM
  per agent on memory, idle cost, and start time; the four steps a program
  goes through; what an operator can bound an instance with, including
  the default refusal of private addresses; that the server exposes MCP
  per blueprint and an HTTP API; that it is the production piece and where
  it runs.
- **Boundaries:** No configuration, tokens, state directories, or limit
  figures (Part 4 and reference). No language or compiler-error material
  (parked on the Language reference entry).
- **Evidence:** Assembled from `server` (intro), `how-submilli-works` ("What
  happens to a program", "A language the model already writes" condensed,
  "An in-process sandbox instead of a virtual machine"). New writing: the
  opening's contrast with a microVM per agent (Doron's story, 2026-09-30:
  half a gigabyte to a gigabyte per microVM, a few dozen per 64 GB machine,
  kept warm and billed while idle, versus a few megabytes per instance, no
  CPU while waiting, hundreds of runs and thousands of sessions per server,
  fuel as the compute meter). Those figures are his sizing reasoning, not a
  published measurement; "10x cheaper" was left out for that reason. The
  sandbox section was first trimmed and then removed on 2026-10-01; its one
  claim worth keeping somewhere, that an agent needing an OS (installing
  dependencies, running binaries) is not what Submilli is for, has no home
  yet. Left out: the
  "TypeScript because models write it" reasoning and the default memory
  figure. "Errors written to be fixed in one turn" (the misspelled-field
  example and the denial's closing instruction) was removed at Doron's
  request on 2026-10-01 and parked on the Language reference entry in the
  plan. Added on 2026-10-01 at Doron's request: "Limits" (from
  `resource-limits` intro and `server` "limits", no figures) and "In
  production" (from `server` intro, `harness` "what an MCP connection
  fixes", `deploying` intro): the server is the production piece, with an
  MCP endpoint per blueprint and an HTTP API.

## Your application

- **Type:** explanation.
- **Purpose:** Generalize the forty-line application to a harness.
- **Starting point:** Chapters 3 and 6.
- **Understanding:** The harness keeps the loop; MCP or HTTP; the three
  decisions the model has no part in; what tools the agent gets and what a
  result and a denial look like to it.
- **Boundaries:** No framework code (tutorials), no endpoint or tool tables
  (reference).
- **Evidence:** Assembled from `harness` (intro, "What an MCP connection
  fixes", "The eight MCP tools and the execute result"). The tool table is
  condensed to one sentence. The examples are changed from the research
  agent (`userId`, `u_ada`, a notebook result) to the quickstart's blueprint,
  customer, and denial, so the reader meets nothing new; those values are
  illustrative and not re-run. New sentences: the opening and "Where to go
  from here".

## Verification

- `submilli --version` on this machine prints `submilli 0.1.0`; `submilli
  builtins Map` prints the declaration shown. The installed 0.1.0 has no
  `skill` or `upgrade` subcommand and `submilli-server` is not on the path,
  so the skill commands, `submilli upgrade`, and the server start, status,
  and stop output are as the current `skill`, `quickstart`, and `server`
  chapters document them, not re-run here.
- Packages: built `@acme/billing` (the `applyCredit` fragment plus a
  `lookUpClass` stub and a short readme) with CLI 0.1.6 in a scratch project
  under an isolated `SUBMILLI_HOME`; `submilli search billing` and
  `submilli docs @acme/billing` output is copied from that run. Ran
  `credit.ts` under a blueprint granting `acme.com/credits.apply` filtered on
  `${vars.customerId}` and `customerClass == "premium"`: `credited 1500
  cents` bound to `cus_northwind`, `PermissionDeniedError` bound to
  `cus_initech`. Those two runs used the stub version of `applyCredit`
  that returned a literal; the page's fragment now makes the real call
  (`secrets.get`, `http.post` to `billing.internal.example.com`), which
  `build check` compiles and derives into `requires: http.post` on that host
  and `secrets.get` for `BILLING_API_KEY`, matching chapter 4's package
  rules. With the real call the run can't be reproduced against a fictional
  host, so the `credited 1500 cents` line stands as the stub's output. The
  tip's warning was produced by removing `customerClass`
  from the tag in that project and running `build check`; only the path is
  shortened from `packages/billing/src/lib.ts` to the chapter's
  `package/src/lib.ts`. Note: the readme is in the store but `submilli docs` does
  not print it; the page attributes the readme to the agent's docs tool,
  per the package chapter. The current CLI chapter's "what they print is
  what the model sees" is therefore not exact for the readme.
- Quickstart code and outputs are unchanged from the verified chapter.
  Still to verify when the CLI is rebuilt: `submilli blueprint lint` after
  `publish-local` in the new order (the chapter already linted after
  publishing, so this should hold).
- Site: `npm --prefix docs-site run check` and `run build` after adding the
  drafts; see the session notes.
