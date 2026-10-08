# encodeURIComponent

Source: `test/built-ins/encodeURIComponent/**` (31 files). The function exists as an
auto-imported free function (spec.md "URI handling"). The port covers the escape
set, UTF-8 encoding of every BMP range and of surrogate pairs, and the literal-URL
vectors. 20 ported, all passing. Representative rejected originals are under
`rejected/encodeURIComponent/`.

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

## Rejected (design decisions)

| Pattern | Reason |
|:--|:--|
| `S15.1.3.4_A5.1` to `S15.1.3.4_A5.6` (enumerability, `hasOwnProperty`/`delete` of `length`, `length` writability and value, global-object enumeration via `this`, `.prototype`) | Reflection on the function object. Functions carry no properties (`encodeURIComponent.length` is a compile error), and there is no global `this`. Copied: `rejected/encodeURIComponent/S15.1.3.4_A5.2.js`. |
| `S15.1.3.4_A5.7` (`new encodeURIComponent()` throws TypeError) | IsConstructor reflection. `new` on a function type is a compile error. Copied. |
| `S15.1.3.4_A6_T1` (ToPrimitive of an object via `valueOf`/`toString`) | The parameter is typed `string`, so an object argument is a compile error. This is the coercion-trap blanket rule. Copied. |
