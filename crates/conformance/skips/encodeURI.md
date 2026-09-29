# encodeURI

Source: `test/built-ins/encodeURI/**` (31 files). The function exists as an
auto-imported free function (spec.md "URI handling"). The port covers the escape
set, UTF-8 encoding of every BMP range and of surrogate pairs, and the literal-URL
vectors. 15 ported, all passing, plus 1 divergence pin under
`cases/encodeURI/divergence/`. Representative rejected originals are under
`rejected/encodeURI/`.

Blanket rules (SKIPS.md) cover `name.js`, `not-a-constructor.js` and
`prop-desc.js` without further note.

Porting adaptations used throughout (README rules):

- The harness helpers `decimalToHexString` / `decimalToPercentHexString`
  (test262 `harness/decimalToHexString.js`) are copied into each case.
  Bitwise operators are deferred (spec.md), so their `&` / `>>` byte arithmetic,
  and the byte arithmetic in the case bodies, is spelled with `Math.floor` and
  `%`. In the hot loops `decimalToPercentHexString` is inlined at its call sites,
  because a user-function call costs more than the rest of the loop body.
- The `indexO`/`indexP` range reporting before each `Test262Error` becomes one
  assert that names the first failing value and the failure count.
- Labeled `continue l` becomes an early return from a small helper.

## Divergence pins (`cases/encodeURI/divergence/`)

| Pin | Divergence |
|:--|:--|
| `lone-surrogate-becomes-replacement` | A lone surrogate is replaced by U+FFFD at the string boundary before the encoder sees it, so it encodes as `%EF%BF%BD`. JS throws `URIError`. Documented in spec.md "URI handling". Well-formed pairs built with `String.fromCharCode(hi, lo)` still encode as one code point (ported `S15.1.3.3_A2.4_T1/T2` pass). |

## Rejected (design decisions)

| Pattern | Reason |
|:--|:--|
| `S15.1.3.3_A1.1_T1`, `S15.1.3.3_A1.1_T2`, `S15.1.3.3_A1.2_T1`, `S15.1.3.3_A1.2_T2`, `S15.1.3.3_A1.3_T1` | They expect a `URIError` for lone surrogates. Replacing them with U+FFFD is the documented divergence pinned above. Copied: `rejected/encodeURI/S15.1.3.3_A1.1_T1.js`. |
| `S15.1.3.3_A5.1` to `S15.1.3.3_A5.6` (enumerability, `hasOwnProperty`/`delete` of `length`, `length` writability and value, global-object enumeration via `this`, `.prototype`) | Reflection on the function object. Functions carry no properties (`encodeURI.length` is a compile error), and there is no global `this`. Copied: `rejected/encodeURI/S15.1.3.3_A5.2.js`. |
| `S15.1.3.3_A5.7` (`new encodeURI()` throws TypeError) | IsConstructor reflection. `new` on a function type is a compile error. Copied. |
| `S15.1.3.3_A6_T1` (ToPrimitive of an object via `valueOf`/`toString`) | The parameter is typed `string`, so an object argument is a compile error. This is the coercion-trap blanket rule. Copied. |
