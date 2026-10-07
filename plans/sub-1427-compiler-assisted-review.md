# Compiler-assisted package security review

The goal is useful evidence for the LLM reviewer, followed by small independently
validated warning rules. It is not whole-program authorization proof or zero
static uncertainty. SUB-1427 delivers this first cycle; SUB-1428 evaluates it.

## Research and warning policy

Infer Pulse separates manifest errors from latent conditions, while explicitly
acknowledging false positives and negatives around unknown calls. Clang's thread
safety checker documents conditional-lock and alias limitations. CodeQL records
precision separately from severity. These are reasons to narrow our warning
contracts, not evidence that an arbitrary security analyzer can be infallible.

Sources:
- https://github.com/facebook/infer/blob/main/website/docs/checker-pulse.md
- https://clang.llvm.org/docs/ThreadSafetyAnalysis.html#known-limitations
- https://codeql.github.com/docs/writing-codeql-queries/metadata-for-codeql-queries/

Keep deterministic declaration/schema validation under its exact contract.
Do not infer a missing policy check from an overapproximated call path, or a
selector mismatch from missing provenance. Unknown calls, getters, aliases,
normalization, state, lookup confinement and remote semantics belong in review
context, not ordinary security warnings.

A possible later rule is a demonstrably feasible check-denial path that performs
a known privileged write. Even that needs an explicit contract excluding
legitimate independently authorized cleanup/fallbacks. Start with straight-line,
resolved operations and literal/primitive values; abstain outside that subset.
Do not ship it if the claim cannot be justified. Each rule owns safe and unsafe
fixtures, a maintained-package finding audit, independent review, and its own PR.
No known false positives are accepted. This criterion is not a mathematical
promise that our implementation contains no bugs.

## First PR and history disposition

Keep SUB-1420's call graph, source spans and potential-effect witnesses. Reverse
the broad SUB-1421 missing-public-check and SUB-1422 dominance warning machinery
already on main. Record check invocation locations and literal names without
claiming that they succeeded. Same-package helpers preserve caller attribution;
the mere absence of a direct check is not a reliable semantic defect test.

Start from current main, leaving unpublished SUB-1423/SUB-1425 commits and the
uncommitted SUB-1424/SUB-1428 experiment out of the replacement branch. Recovery
copies remain outside the PR. Gmail target binding and Drive/Notion upload-path
remediations need separate package PRs. Batch reshaping solely to satisfy the
abandoned analysis is unnecessary. Runtime gates, capability requirements and
pre-existing declaration/check-discipline diagnostics remain unchanged.

## Review evidence contract

`build authority-map` schema version 2 removes `guard` proof statuses and adds
syntactic `checks` to callables. It retains the full graph and expanded witnesses.
This is an intentional output/API compatibility change.

`build security-review` captures source once, compiles a private materialization
of those exact bytes, and provides generated capability schemas and a compact
map alongside source. It never consumes ambient installed dependency artifacts.
External dependency source remains a blocking coverage gap. Compile failures or
map-size failures stop before agent invocation and leave an incomplete report.

The compact map retains callable IDs, source spans, direct effects, check sites,
and shared call edges. Public/initialization callable IDs form `routes`. Follow
edges as source-located witness steps; repeated transitive effect lists are
empty in this view to avoid duplicating each sink across every caller. All
callables remain available, including those requiring source-level exposure
review. Potential paths are not necessarily feasible. An unresolved edge is a
review lead, not a compiler finding.

Source hashes include selected packages and compiler version. The evidence hash
also covers generated schemas, compact graph and limitations. Reports use schema
version 2 and retain both hashes plus `finding_origin: model`. Package source and
all generated fields remain untrusted data, never prompt instructions.

The existing source bound is 512 KiB; compact authority evidence is limited to
2 MiB of serialized hash input. There is no silent truncation: select a smaller
package with `-p` when a limit is exceeded. These are resource ceilings, not
claims that every model can use the full context effectively. The reviewer must
report incomplete coverage if it cannot inspect the supplied evidence.

General static-analysis limitations alone do not prevent an LLM from completing
a source review. Missing source or unresolved security questions do. A complete
review means one model inspected its inputs; it is never static certification.

## Following cycles

First compare source-only and map-assisted review on the same seeded defects
and safe counterparts: missing authorization, swallowed denial, selector mismatch,
and state/egress. Measure missed defects, false alarms, evidence quality, token
cost and elapsed time. Record model/configuration and repeat enough to expose
variation. Integration tests with mock agents prove plumbing, not model quality.
Only then select a narrow rule supported by the evidence and deliver it separately.
