# Number and the global numeric functions

Sources: `test/built-ins/Number/**` (340 files), `test/built-ins/parseInt/**`
(55), `test/built-ins/parseFloat/**` (54), `test/built-ins/isNaN/**` (15),
`test/built-ins/isFinite/**` (15). 44 ported (38 passing, 4 `expect-fail`,
2 `expect-error` pins), under `cases/Number/`, `cases/parseInt/`,
`cases/parseFloat/`, `cases/isNaN/`, `cases/isFinite/`. Representative
rejected originals under `rejected/Number/`, `rejected/parseInt/`,
`rejected/parseFloat/`, `rejected/isNaN/`.

Blanket rules (SKIPS.md) cover, without further note: every per-member
`prop-desc.js` / `length.js` / `name.js` / `not-a-constructor.js` /
`is-a-constructor.js`, `proto-from-ctor-realm.js`, the
`return-abrupt-*` / `toprimitive-*` / `toFixed-tonumber-throws-*` /
`numeric-literal-tostring-radix-poisoned.js` /
`precision-cannot-be-coerced-*` coercion traps, the
`this-type-not-number-or-number-object.js` receiver-coercion cases, the
constant-descriptor write tests (`MAX_VALUE`/`MIN_VALUE` `S15.7.3.x_A2-A4`,
`POSITIVE_INFINITY`/`NEGATIVE_INFINITY` `S15.7.3.5/6_A1-A2` and
`prop-desc.js`), the Number-constructor property tests (`S15.7.3_A1-A8`,
`S15.7.3-1/2`, `15.7.4-1`, `S8.12.8_A3/A4`), and the global-property tests
(`parseInt`/`parseFloat` `S15.1.2.x_A7-A9`, `isNaN`/`isFinite`
`S15.1.2.4/5_A2.6-A2.7`).

## Known gaps (`expect-fail`)

| Case | Gap |
|:--|:--|
| `parseInt/S15.1.2.2_A2_T10` | `parseInt` skips only ASCII whitespace; the standard's StrWhiteSpace includes the Unicode space separators (U+1680, U+2000-200A, U+202F, U+205F, U+3000), which currently make the parse return `NaN`. `Number(string)` trims them correctly — only the prefix parsers diverge. |
| `parseFloat/S15.1.2.3_A2_T10` | Same Unicode-whitespace gap for `parseFloat`. The `*_U180E` variants (U+180E was whitespace in old Unicode) stay unported either way. |
| `Number/prototype/toExponential/return-values` | `toExponential` rounds exact ties half-to-even — `(25).toExponential(0)` is `"2e+1"` — where the standard picks the larger candidate (`"3e+1"`). spec.md documents the half-to-even tie rule only for `toFixed`; for `toExponential` it says "follow the standard's forms", so this is a gap, not a documented divergence. All other rows of the table pass. |
| `Number/prototype/toPrecision/infinity` | `toPrecision` validates the 1..100 precision range before the NaN/Infinity short-circuit and throws; the standard returns `"Infinity"`/`"-Infinity"` (and `"NaN"`) first, only then range-checks. `toExponential` gets this order right (its `infinity.js` is ported and passes); `toPrecision/nan.js` would hit the same gap but is built on a `valueOf` coercion trap, so it stays rejected. |

## Pinned divergences (`expect-error`)

| Case | Divergence |
|:--|:--|
| `Number/S9.3_A4.1_T1` | `Number(true)` — `Number(x)` takes `string \| bigint`; ToNumber(boolean) does not exist (spec.md "Numeric globals"). |
| `parseInt/S15.1.2.2_A1_T1` | `parseInt(true)` — the argument is typed `string`; no ToString coercion. |

## Rejected (design decisions)

| Pattern | Reason |
|:--|:--|
| `new Number(...)` receivers and `Number.prototype` receivers (`S15.7.2.1_A*`, `S15.7.4_A*`, `S15.7.5_A1_T*`, `S9.3_A5_T1`, `S9.1_A1_T1`, the `(new Number(x)).toString/toFixed/...` arms of `prototype/*` cases) | No primitive wrapper objects; `Number` is a call signature, not a constructor. Copied: `rejected/Number/S15.7.2.1_A1.js`. Ports of mixed cases keep only the plain-value arms. |
| `prototype/valueOf/**` | No boxing. Copied: `rejected/Number/prototype/valueOf/S15.7.4.4_A1_T01.js`. |
| `prototype/toLocaleString/**` | No locale/ICU. Copied: `rejected/Number/prototype/toLocaleString/prop-desc.js`. |
| ToNumber/ToString coercion of non-number/non-string arguments (`S9.3_A1-A4.2`, `isInteger`/`isSafeInteger`/`isNaN`/`isFinite` `arg-is-not-number.js`, `isNaN`/`isFinite` `tonumber-operations.js`, `parseInt` `S15.1.2.2_A1_T2-T7` and radix coercion `S15.1.2.2_A3.1_T1-T7`, `parseFloat` `S15.1.2.3_A1_T*` and `15.1.2.3-2-1`-adjacent object cases) | Arguments are statically typed; the coercing calls are compile errors. Two are pinned as `expect-error` (above); representatives copied: `rejected/parseInt/S15.1.2.2_A3.1_T1.js`, `rejected/parseFloat/S15.1.2.3_A1_T1.js`, `rejected/isNaN/tonumber-operations.js`. |
| Constant property-descriptor tests (`MAX_SAFE_INTEGER.js`, `MIN_SAFE_INTEGER.js`, `EPSILON.js`/`NaN.js` `verifyProperty` arms) | No descriptors. The value assertions are ported (`cases/Number/NaN.ts`, `EPSILON.ts`); the descriptor-only constants are covered by the fixture `number_static_constants.subm`. Copied: `rejected/Number/MAX_SAFE_INTEGER.js`, `rejected/Number/prop-desc.js`. |

## Not ported (below the curation bar)

| Pattern | Reason |
|:--|:--|
| `Number/S9.3.1_A1`, `S9.3.1_A4_T1/T2`, `S9.3.1_A5_T2/T3`, `S9.3.1_A6_T1/T2`, `S9.3.1_A3_T2`, `S9.3.1_A7-A31` | Empty-string, plus-sign, `dynaString`, and single-hex-digit variants of the ported `S9.3.1_A2`/`A3_T1`/`A5_T1`/`A32` grammar vectors; `Number("Infinity")`/sign handling is exercised by the ported `A3_T1` and `parseFloat/S15.1.2.3_A5_T1`, hex by `string-hex-literal-invalid` plus the fixture `number_radix_strings.subm`. |
| `Number/S9.3.1_A2_U180E`, `S9.3.1_A3_T1/T2_U180E`, `parseInt/S15.1.2.2_A2_T10_U180E`, `parseFloat/S15.1.2.3_A2_T10_U180E` | U+180E is whitespace only in pre-6.3 Unicode; test262 keeps these as historical pins. Not meaningful to port. |
| `Number/string-binary-literal.js`, `string-octal-literal.js`, `string-binary-literal-invalid.js`, `string-octal-literal-invald.js` | Same grammar shape as the ported `string-hex-literal-invalid`; the valid prefixes and `0b12`/`0o8`-style rejections are covered by the fixture `number_radix_strings.subm`. |
| `Number/string-numeric-separator-literal-*.js` (36 files) | All assert `Number("1_1")`-style separators are NaN in ToNumber; one shape repeated per grammar production. Junk-string NaN is covered by the ported grammar cases. |
| `Number/isInteger/non-integers/infinity/nan`, `isSafeInteger/not-safe-integer/not-integer/infinity/nan`, `isNaN/nan`, `isFinite/infinity/nan` | Single-assert complements of the ported true-side tables, also covered by the fixtures `number_static_predicates.subm`/`number_global_predicates.subm`. |
| `prototype/toString` radix files 3-36, `numeric-literal-tostring-default-radix.js`, `numeric-literal-tostring-radix-1/37.js`, `S15.7.4.2_A2_T*` digit tables | `a-z.ts` covers the full digit alphabet, `radix-2.ts` the NaN/Infinity/zero shapes; default radix and the 1/37 throws are in the fixture `number_to_string_radix.subm`. |
| `prototype/toFixed/S15.7.4.5_A1.*`, `return-type.js` | Receiver arms are boxing; the digit-coercion rows are type errors; the plain rows repeat the fixture `number_to_fixed.subm`. |
| `prototype/toExponential` `nan.js`, `range.js`, `this-is-0-*`, `undefined-fractiondigits.js`, `tointeger-fractiondigits.js`; `prototype/toPrecision` `exponential.js`, `this-is-0-*`, `undefined-precision-arg.js`, `tointeger-precision.js` | NaN/zero/omitted-argument shapes covered between the ported `return-values`/`range`/`infinity` cases and the fixture `number_to_precision_exponential.subm`; `undefined`-argument rows don't port (no `undefined`). |
| `parseInt/S15.1.2.2_A2_T2-T9`, `A4.1_T1/T2`, `A5.2_T1/T2`, `A6.1_T2-T6`, `A7.1-A7.3`, `A8`, `15.1.2.2-2-1`; `parseFloat/S15.1.2.3_A2_T2-T9`, `A3_T2/T3`, `A4_T3-T7`, `A5_T2-T4`, `S15.1.2.3_A6`, `tonumber-numeric-separator-*` | Whitespace/garbage/radix-table variants of the ported representatives (`A2_T1`, `A4.2_T1`, `A5.1_T1`, `A6.1_T1`, `A3_T1`, `A4_T1/T2`, `A5_T1`); the 65536-code-point scans (`A8`, `A6`) and per-digit loops repeat the same predicate per character. Hex-prefix radix-0 detection is in the fixture `number_parse_statics.subm`. |
