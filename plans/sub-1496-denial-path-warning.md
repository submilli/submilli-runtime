# SUB-1496: denial-path warning validation

## Decision: reject this candidate

A denied semantic check followed by a privileged write is observable behavior,
but does not by itself violate the existing authoring contract. Do not introduce
this warning or promote it in CI. Retain the executable examples for source-based
LLM review, where the package's documented authorization contract is available.
This is the stop condition agreed for SUB-1496, not a claim that all narrower
future rules are impossible.

## Candidate and missing premise

The candidate would report a direct public function with a statically resolved
`submilli:security.check`, a literal capability and plain literal payload, and a
locally feasible denial handler reaching `submilli:fs.writeText` with literal
arguments. There are no helper calls, loops, getters, callbacks, aliases, mutable
selectors, prior ownership lookups, or dynamic dispatch in that subset. Rethrow
and early return before the write exclude the path. An independent fallback
check is outside this initial subset and must cause abstention, not a warning.
Unknown control flow is neither a warning nor a safety conclusion.

The strongest justified statement is conditional: **if the semantic check denies
and execution has sufficient resources, this handler attempts the specified
filesystem write; it can succeed if the package's filesystem grant permits it.**
It does not prove that every caller can exploit it, that the host gate is bypassed,
or that the write violates the package's contract.

To call that behavior a defect requires a further premise: this particular write
must require the denied permission, with no independent authorization. Existing
`@capability` tags do not supply that premise. `doc_comment.rs` represents tags
and payload bindings; `typechecker/rules/capability_consistency.rs` validates tags
against check invocations and their payloads. Neither declares which effect must
be guarded. `authority.rs` explicitly records syntactic check invocations rather
than successful authorization. Capability names and matching path strings do not
establish mandatory authorization either.

The `unsafe` and `optional` examples have byte-identical TypeScript, annotations,
and generated capability schemas. Their documented contracts differ: one requires
caller permission for the fixed write; the other explicitly permits that public
operation and uses the check only to select its response. Both execute the write
on denial. The latter is intentionally unusual, but supported by the existing
runtime and metadata contract. Rejecting it would introduce a new authoring rule.

The more conventional `logging` case writes a fixed denial audit record under
package authority while preserving the denied protected operation. The independently
checked `fallback` case demonstrates another legitimate authorization path.
Excluding filenames, inferring meaning from capability names, treating every
catch/write as unsafe, or requiring authors to rewrite these patterns would not
satisfy the release criterion. No new annotation or generalized proof engine is
introduced in this cycle.

## Executable evidence

The separate corpus is in `scripts/security-denial-validation/`; SUB-1428's frozen
corpus and results are unchanged. Each fixture is a real package published into
a private temporary store and run through the freshly built CLI. Each scenario
has an empty temporary VFS, explicit caller permissions, and an independently
controlled package `fs.write` grant. The verifier checks the returned outcome and
all files in that VFS, including absence of unexpected effects. Temporary source
copies keep generated metadata and caches out of the committed corpus.

| Case | Caller checks denied, package write allowed | Contract disposition |
| --- | --- | --- |
| unsafe | `/record` contains `record`; returns `handled` | Violates mandatory caller permission |
| rethrow | `acme.write` denial propagates; no file | Safe |
| return | Returns `handled`; no file | Safe |
| fallback | `acme.fallback` denial propagates; no file | Safe; independent check outside candidate subset |
| logging | `/audit` contains `denied`; `acme.write` denial propagates; no `/record` | Legitimate independently permitted audit write |
| optional | Same behavior as unsafe | Explicitly permitted by its different package contract |

Every case also succeeds with primary caller permission, and fails without the
host write grant after primary authorization. Fallback-only permission succeeds
when primary permission is denied, but still needs the host write grant. Unsafe,
optional, and logging denial branches are also tested with the host write grant
denied. Total: 23 policy executions. Source and generated-schema equality for the
counterexample pair are asserted by the verifier.

Run from the repository root:

```sh
cargo build -p submilli
python3 scripts/security-denial-validation/verify.py --binary target/debug/submilli
```

All 23 executions passed on 2026-10-08. No model or external API is called. These
are Submilli package-policy tests, not a TypeScript/Node compatibility claim;
compiler syntax, typing, and runtime semantics are unchanged. No production
analyzer exists for this rejected candidate, so there are no emitted findings,
suppressions, or uncertainty diagnostics to audit.

## Maintained-package candidate audit

Source revision: `466dbc28d52a91d0012c553c4a3243905e429a8f`. Inventory is all 14
packages declared in root `submilli.toml`, covering all 23 `.ts` source files
under their `src/` trees (no `.subm` source files). Tests and documentation examples
are outside this production-route inventory. Search for `catch` and `finally`
located handlers; each enclosing try, handler, and continuation was read to
classify it. This is a manual audit of the proposed **direct check/handler/write**
subset, not a whole-program security review. No candidate checker was run.

| Package (`@submilli/`) | Source files | Handler disposition |
| --- | ---: | --- |
| jina | 1 | No handlers |
| linear | 1 | `lib.ts:1175`: JSON error-message parsing fallback; no check/write sequence |
| slack-user | 1 | No handlers |
| slack-bot | 1 | No handlers |
| google-calendar | 1 | `lib.ts:1021`: timestamp parsing; throws validation error |
| gmail | 1 | No handlers |
| google-drive | 1 | No handlers |
| notion | 10 | `lib.ts:866`, `pages.ts:204`: batch loops and helper calls; out of subset; handlers rethrow, including semantic denial |
| github | 1 | `lib.ts:2776`: timestamp parsing; throws validation error |
| sentry | 1 | `lib.ts:938`: JSON error-message parsing fallback; no check/write sequence |
| brave-search | 1 | `lib.ts:263,285,329`: response normalization and error-message parsing; no check/write sequence |
| exa | 1 | `lib.ts:230,262,285,382`: URL/date validation and response normalization; no check/write sequence |
| firecrawl | 1 | `lib.ts:599,625,669,683,697,718,730,742`: URL validation and response normalization; no check/write sequence |
| typesafe | 1 | `lib.ts:298`: response decoding; throws validation error |

No `finally` handlers were present. Zero direct candidates were found; the two
Notion handlers are explicitly unsupported rather than certified safe. No package
exemptions, suppressions, or source rewrites were used. This inventory cannot
rescue the warning's contract: the executable legitimate counterexamples block
promotion even if current maintained packages contain no matching sites.
