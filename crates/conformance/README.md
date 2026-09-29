# Conformance suites

Two suites live here:

- **test262** (this document): runtime behavior against the ECMAScript standard.
- **TypeScript** ([`typescript/README.md`](typescript/README.md)): what the
  typechecker infers, against what `tsc` infers for the same program.

[`COVERAGE.md`](COVERAGE.md) lists each feature Submilli supports, whether every
upstream test about it has been dealt with, and what both suites check of it. In the
TypeScript suite, every divergence from `tsc` is explained or listed as not yet
explained, and every upstream test left out is listed with its reason.

## test262

Hand-ported, vendored subset of [tc39/test262](https://github.com/tc39/test262),
the official ECMA-262 conformance suite, adapted to our strict TypeScript
subset. Run via `cargo test -p conformance`.

**Why a hand-ported vendored subset (not a scripted port):** test262 cases
assume dynamic typing, `undefined`, `var`, sparse arrays, `Symbol`,
property descriptors, and an `eval`-based harness. Translating those into a
statically-typed subset requires judgment per case (splitting heterogeneous
arrays by element type, adding annotations, choosing `null` for `undefined`),
which a script can't do reliably. The port is curated for *intent coverage*:
each ported case mirrors one test262 case's intent and records its
provenance; wholesale-skipped material is recorded in `SKIPS.md`.

Ported cases derive from test262 and remain governed by its BSD license —
see `TEST262-LICENSE`.

## Layout

```
harness.ts        the test262 assertion shim, prepended to every case
cases/<Area>/...  ported cases, mirroring test262's test/built-ins/ tree
rejected/...      verbatim test262 cases excluded by a design decision
SKIPS.md          skip/reject conventions + blanket rules
skips/<Area>.md   per-area record of what was skipped or rejected, and why
```

`CONFORMANCE_FILTER=<path substring> cargo test -p conformance` runs the
matching subset of cases.

A ported case is a self-contained program with a `main(): void` entry point
and a provenance header:

```ts
// test262: test/built-ins/Array/prototype/includes/search-found-returns-true.js
```

## Runner directives

In the first comment block of a case:

- `// expect-error: <substring>` — pins a **by-design compile error**
  (documented divergence). Compilation must fail; every needle must match.
- `// expect-fail: <reason>` — **known gap**: the standard says this should
  work and it currently doesn't. The ported case keeps asserting the
  *standard* behavior; the runner requires it to fail (compile error or
  trap) and errors if it starts passing, so a closed gap surfaces. Each
  gap has a Linear issue; the reason text describes the gap itself.

Cases in `rejected/` are kept **verbatim** (original `.js`, original
assertions) so they can be revived if a design decision is revisited. They
are never compiled; each carries a `// rejected: <reason>` first line, and
`SKIPS.md` aggregates the reasons. The `rejected_cases_carry_reasons` test
keeps that honest.

## Harness mapping

Functions here can't carry properties, so the dotted test262 API maps to
free functions (all with SameValue semantics via `Object.is` — `NaN`
matches `NaN`, `+0` differs from `-0`):

| test262 | shim |
|:--------|:-----|
| `assert(v, msg)` | `assert(v, msg)` (built-in; `v` must be `boolean`) |
| `assert.sameValue(a, e, msg)` | `assertSameValue(a, e, msg)` |
| `assert.notSameValue(a, e, msg)` | `assertNotSameValue(a, e, msg)` |
| `assert.throws(ErrType, fn, msg)` | `assertThrows((): void => { … }, msg)` |
| `assert.compareArray(a, e, msg)` / `compareArray` | `assertCompareArray(a, e, msg)` |

`assertThrows` matches the base `Error` only — there are no error
subclasses yet, so test262's `TypeError`/`RangeError` distinctions are
erased by the port (note it in the case when the distinction was the
point of the test).

Caveat: `Object.is` on objects follows the language's structural `===`,
not JS reference identity — SameValue assertions over *object identity*
don't port; rewrite them against a discriminating field, or reject.

## Adaptation rules

Apply these when porting; anything that can't be expressed under them goes
to `rejected/` (design decision) or `SKIPS.md` (not ported, with reason).

1. Wrap the test body in `function main(): void { … }`; no top-level
   statements (top-level `const` is fine).
2. `var` → `let`/`const`; add type annotations where inference needs them.
3. `undefined` → `null`. Cases *about* `undefined` semantics (holes,
   missing properties) are rejected by design.
4. Heterogeneous arrays: split by element type, or use `unknown[]` with
   explicit element typing.
5. Absent optional arguments: portable only when our signature declares
   the parameter optional; otherwise skip the variant and record it.
6. `Date` → `Temporal`. Non-ISO calendars and `Calendar`/`TimeZone`
   objects are rejected by design.
7. Drop entirely (rejected by design, not skipped): `Symbol.*`, property
   descriptors (`prop-desc.js`, `length.js`, `name.js`,
   `not-a-constructor.js`), prototype-chain and `this`-coercion cases
   (`this-is-not-object.js`), getter/`valueOf` coercion traps
   (`return-abrupt-*`), `eval`, sparse/holey arrays, species
   constructors, locale-dependent (`toLocale*`) and Annex-B material.
8. Type-coercion cases (`ToInteger(fromIndex)` passing strings, etc.):
   the type system rejects the call outright — where the divergence is
   documented, pin it with `expect-error`; otherwise reject the case.
9. One test262 file → one ported file, same basename, `.ts` extension,
   mirrored directory. Keep the original assertion order and messages
   where they fit.
10. The test262 frontmatter (`/*--- … ---*/`) and copyright header are
    dropped; the provenance line replaces them.
