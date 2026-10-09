# BigInt

Source: `test/built-ins/BigInt/**` (77 files). The portable core is the
`BigInt()` conversion (number and string arms), its error cases, and
`prototype/toString` — 15 ported (13 passing, 2 `expect-fail`), plus 1
divergence pin under `cases/BigInt/divergence/`. Representative rejected
originals under `rejected/BigInt/`.

Arithmetic, comparison, division-truncation, and negative-zero vectors live
under `test/language/expressions/**`, outside this area's source tree; the
hand-written fixture `conformance_bigint.subm` (interpreter crate) covers
exact arithmetic, truncating division, and division-by-zero in the meantime.

Blanket rules (SKIPS.md) cover `length.js`, `name.js`,
`not-a-constructor.js`, `prop-desc.js`, `proto.js`,
`prototype/constructor.js`, `prototype/Symbol.toStringTag.js`, and the
coercion-trap cases (`tostring-throws.js`, `valueof-throws.js`,
`call-value-of-when-to-string-present.js`, the `valueOf`-object arms of the
`*-rangeerror.js` files) without further note. The same grounds reject
`is-a-constructor.js` (constructor-ness reflection) and
`wrapper-object-ordinary-toprimitive.js` (`Object(1n)` wrappers).
Representatives copied: `rejected/BigInt/prop-desc.js`,
`rejected/BigInt/constructor-coercion.js`.

## Known gaps (`expect-fail`)

| Case | Gap |
|:--|:--|
| `constructor-from-hex-string` | StringToBigInt accepts `0x`/`0X` (and `0b`/`0o`) radix prefixes; `BigInt(string)` parses decimal only (spec.md §2.6 describes the string arm as "a decimal of arbitrary length" — `Number(string)`, by contrast, explicitly takes the prefixes) and throws `invalid bigint literal`. `constructor-from-binary-string.js` and `constructor-from-octal-string.js` hit the same gap; hex is the ported representative. |
| `constructor-empty-string` | StringToBigInt maps empty / whitespace-only strings to `0n`; `BigInt("")` throws `invalid bigint literal`. Undocumented — spec.md is silent on the empty string. |

## Divergence pins (`expect-error`)

| Case | Divergence |
|:--|:--|
| `divergence/mixed-comparison-rejected` | number↔bigint never mix implicitly — comparison included (`10n < 36` is a compile error; JS defines mixed relational comparison). Derived from `prototype/toString/a-z.js`, whose loop compares a bigint counter against a number radix; the ported `a-z.ts` uses a number counter with explicit `BigInt(i)`. |

## Rejected (design decisions)

| Pattern | Reason |
|:--|:--|
| `asIntN/**`, `asUintN/**` (28 files) | Deferred — `BigInt.asIntN`/`asUintN` land with the bitwise operators (`docs/ecma-262-gaps.md` §BigInt). Copied: `rejected/BigInt/asIntN/arithmetic.js`, `rejected/BigInt/asUintN/arithmetic.js`. |
| `prototype/valueOf/**` (8 files) | No boxing or prototype model; `valueOf` rejected (`docs/ecma-262-gaps.md`). Copied: `rejected/BigInt/prototype/valueOf/return.js`. |
| `prototype/toLocaleString/not-a-constructor.js` | `toLocaleString` rejected — locale-dependent output. Copied. |
| `parseInt/nonexistent.js` | Asserts via `hasOwnProperty` reflection that `BigInt.parseInt` is absent; no property reflection here, and the typechecker already rejects `BigInt.parseInt` statically. |
| `prototype/toString/prototype-call.js`, `thisbigintvalue-not-valid-throws.js`, `radix-tointegerorinfinity-throws-symbol.js`, `radix-tointegerorinfinity-throws-toprimitive-or-bigint.js` | `this`-rebinding via `.call`, `Symbol.toPrimitive`, and radix coercion traps — no prototypes, no `Symbol`, radix is typed `number`. |

## Not ported (below the curation bar / row drops)

| Pattern | Reason |
|:--|:--|
| `constructor-trailing-leading-spaces.js` rows `"   0b1111"` and `"     "` | Ride the two `expect-fail` gaps above; the decimal-with-whitespace rows are ported. |
| `prototype/toString/default-radix.js` `undefined`-argument rows | Ported alongside omitted-argument rows. |
| `prototype/toString/radix-err.js` `null` radix row | Compile error (`radix` is `number`); the 0/1/37 rows are ported. |
| `string-is-code-units-of-decimal-digits-only.js` `BigInt(0n)` row | `BigInt()` takes `string \| number`; the other rows are ported. |
