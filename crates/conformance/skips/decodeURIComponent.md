# decodeURIComponent

Source: `test/built-ins/decodeURIComponent/**` (56 files). The function exists as an
auto-imported free function (spec.md "URI handling"). The port covers every
malformed-escape family, the valid 1- to 4-byte UTF-8 decodings, the reserved
set, and the literal-URL vectors. 45 ported, all passing.
Representative rejected originals are under `rejected/decodeURIComponent/`.

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
- The throwing checks inline their try/catch, which tests for the built-in
  `URIError`, instead of calling a helper.

One loop is trimmed for run time, and the case says so in its header.
`A2.5_T1` walks all ~983k four-byte sequences upstream. The port samples the
third byte at five values and keeps every first, second and fourth byte.

## Rejected (design decisions)

| Pattern | Reason |
|:--|:--|
| `S15.1.3.2_A5.1` to `S15.1.3.2_A5.6` (enumerability, `hasOwnProperty`/`delete` of `length`, `length` writability and value, global-object enumeration via `this`, `.prototype`) | Reflection on the function object. Functions carry no properties (`decodeURIComponent.length` is a compile error), and there is no global `this`. Copied: `rejected/decodeURIComponent/S15.1.3.2_A5.2.js`. |
| `S15.1.3.2_A5.7` (`new decodeURIComponent()` throws TypeError) | IsConstructor reflection. `new` on a function type is a compile error. Copied. |
| `S15.1.3.2_A6_T1` (ToPrimitive of an object via `valueOf`/`toString`) | The parameter is typed `string`, so an object argument is a compile error. This is the coercion-trap blanket rule. Copied. |
