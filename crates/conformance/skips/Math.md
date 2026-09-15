# Math

Source: `test/built-ins/Math/**` (327 files). The area is a good fit — every
spec.md §1.10 member exists — so the port covers each implemented function and
constant with at least one behavioral case. 60 ported (57 passing, 3
`expect-fail`), plus 4 divergence pins under `cases/Math/divergence/`.
Representative rejected originals under `rejected/Math/`.

Blanket rules (SKIPS.md) cover `length.js`, `name.js`, `not-a-constructor.js`,
`prop-desc.js` per member (157 files), the area-level `prop-desc.js`,
`proto.js`, and `Symbol.toStringTag.js`, and the coercion-trap cases
(`Math.max_each-element-coerced.js`, `Math.min_each-element-coerced.js`,
`Math.hypot_ToNumberErr.js`, `15.8.2.11-1.js` / `15.8.2.12-1.js`
(`Math.max({})`), `Log10`/`log2` `null`/`undefined` rows) without further
note. Representatives copied: `rejected/Math/prop-desc.js`,
`rejected/Math/max/Math.max_each-element-coerced.js`,
`rejected/Math/hypot/Math.hypot_ToNumberErr.js`.

## Known gaps (`expect-fail`)

| Case | Gap |
|:--|:--|
| `pow/applying-the-exp-operator_A1` | `Math.pow(1, NaN)` returns `1` (IEEE 754 pow via the host's `powf`); the standard returns `NaN` for any NaN exponent. Undocumented — spec.md's pow has no divergence note. |
| `hypot/Math.hypot_InfinityNaN` | `Math.hypot(NaN, Infinity)` returns `NaN` (naive sum of squares: `NaN + Infinity` is `NaN`); the standard checks for an infinite argument first. Undocumented — spec.md documents only the overflow divergence. |
| `round/S15.8.2.15_A7` (checks 4–10) | `Math.round` is `floor(x + 0.5)`, which rounds `0.5 - 2**-54` up to `1` and bumps odd integers in `[2**52 + 1, 2**53 - 1]` (and negatives) to the next even integer. Undocumented — spec.md documents only the sign-of-zero divergence. Checks 1–3 (the `-0` results) are that documented divergence; they live in `rejected/`, not in the port. |

## Rejected (design decisions)

| Pattern | Reason |
|:--|:--|
| `round/S15.8.2.15_A3.js`, `round/S15.8.2.15_A7.js` checks 1–3 | `Math.round` returns `+0` where JS keeps `-0` — documented divergence (spec.md §1.10), pinned in `cases/Math/divergence/round-negative-zero.ts`. Copied: `rejected/Math/round/S15.8.2.15_A3.js`. |
| `clz32/int32bit.js`, `clz32/infinity.js` | JS modulo-2^32 `ToUint32` wrap; ours saturates — documented divergence, pinned in `cases/Math/divergence/clz32-saturating.ts`. Both copied. |
| `imul/results.js` (rows with inputs ≥ 2^31) | Same saturating-vs-wrap divergence, pinned in `cases/Math/divergence/imul-saturating.ts`. Original copied; the in-range subset is ported at `cases/Math/imul/results.ts`. |

## Not in spec (members)

| Pattern | Reason |
|:--|:--|
| `f16round/**` | Member not in spec.md §1.10 (ES2025 addition); the gap analysis (`docs/ecma-262-gaps.md`) records Math as complete without it. |
| `sumPrecise/**` | Member not in spec.md §1.10 (Stage-3 proposal vendored into test262). |

## Not ported (below the curation bar)

| Pattern | Reason |
|:--|:--|
| Per-member NaN-propagation singletons (`abs/S15.8.2.1_A1`, `acos/S15.8.2.2_A1`, `asin/S15.8.2.3_A1`, `atan/S15.8.2.4_A1`, `ceil/S15.8.2.6_A1`, `cos/S15.8.2.7_A1`, `exp/S15.8.2.8_A1`, `floor/S15.8.2.9_A1`, `log/S15.8.2.10_A1`, `sin/S15.8.2.16_A1`, `sqrt/S15.8.2.17_A1`, `tan/S15.8.2.18_A1`, `fround/Math.fround_NaN`, `clz32/Math.clz32_1`) | One representative NaN case per shape is ported (`round/S15.8.2.15_A1`, `acosh/nan-returns`, the `*-specialVals` families); the rest are the same single assert against a different member, also exercised by the hand-written `conformance_math.subm` fixture. |
| Remaining ±0 / ±Infinity singletons per member (`abs/S15.8.2.1_A3`, `acos/S15.8.2.2_A{3,4}`, `acosh/arg-is-{one,infinity}`, `asin/S15.8.2.3_A{2,3,4}`, `atan/S15.8.2.4_A2`, `atan2/S15.8.2.5_A{4,5,8,14}` and the unlisted quadrant cases, `ceil/S15.8.2.6_A{2,3,4,5}`, `cos/S15.8.2.7_A{2,3,5}`, `exp/S15.8.2.8_A{2,3,4}`, `floor/S15.8.2.9_A{2,4,5,6,7}`, `fround/Math.fround_Infinity`, `hypot/Math.hypot_{Infinity,NegInfinity,Success_2}`, `log/S15.8.2.10_A{2,4,5}`, `round/S15.8.2.15_A{2,4,5,6}`, `sin/S15.8.2.16_A{4,5}`, `sqrt/S15.8.2.17_A{3,5}`, `tan/S15.8.2.18_A{2,4,5}`, `trunc/Math.trunc_*`, `trunc/trunc-specialVals`, `max/S15.8.2.12_A2` 3-arg matrix already in `max/S15.8.2.11_A2`, `min/S15.8.2.12_A2`) | Each member already has a ported case covering its special-value table's meatiest rows; these repeat single rows of the same table. |
| `sqrt/results.js`, `pow/applying-the-exp-operator_A{3..22}` | 1000-pair exactness table and the remaining 20 pow special-value files; the ported A2/A23/int32_min cases plus `conformance_math.subm` cover the table's intent. A19/A20 (`pow(±1, ±Infinity)` is NaN) would hit the same IEEE-pow gap as the `expect-fail` A1. |
| Constant `value.js` `typeof` arms | `typeof Math.E === "number"` is subsumed by static typing (`typeof` is a narrowing guard only); the `assertNotSameValue(…, NaN)` arm is ported. |
