# JSON — skipped and rejected test262 material

Source: `test/built-ins/JSON/**` (parse/, stringify/, plus the JSON root).
Ported cases live under `cases/JSON/`; design-divergence vectors under
`cases/JSON/divergence/`. Blanket rules from `SKIPS.md` (prop-desc, length,
name, not-a-constructor, Symbol.*, return-abrupt/`*-abrupt`, proxies,
prototype-chain, eval) are not repeated below.

Our `JSON` deliberately diverges from ECMA-262 (spec.md §1.6,
docs/ecma-262-gaps.md §25): `JSON.parse`/`JSON.stringify` are typed compiler
intrinsics, `parse` is type-directed (throws on shape mismatch, never
returns `unknown`), `stringify` emits canonical lexicographically sorted
keys (RFC 8785 style), and there are no reviver/replacer/space arguments.

## Rejected (design decision; representatives under `rejected/JSON/`)

| test262 path / pattern | Reason |
|:--|:--|
| `parse/reviver-*.js` (29 files), `parse/15.12.2-2-*.js` reviver variants, `parse/revived-proxy*.js` | No reviver argument — conflicts with type-directed parsing. Representative: `rejected/JSON/parse/reviver-call-order.js`. |
| `stringify/replacer-*.js` (21 files) | No non-null replacer argument — conflicts with static typing. Representative: `rejected/JSON/stringify/replacer-array-order.js`. |
| `stringify/property-order.js` | Keys are emitted in lexicographic (RFC 8785 canonical) order, not insertion order. Counter-vector: `cases/JSON/divergence/sorted-key-order.ts`. |
| `parse/text-object.js`, `parse/text-non-string-primitive.js` | `parse` takes a `string`; no ToString coercion of objects/primitives. Representative: `rejected/JSON/parse/text-object.js`. |
| `parse/S15.12.2_A1.js`, `parse/duplicate-proto.js` | No prototypes — `__proto__` is not special. Representative: `rejected/JSON/parse/S15.12.2_A1.js`. |
| `stringify/value-string-escape-unicode.js` | Strings are well-formed Unicode; lone surrogate code units are unrepresentable (lexer rejects lone `\uD834`). Paired-surrogate output is covered by `value-string-escape-ascii.ts`'s mechanism. |
| `stringify/value-bigint*.js` | JS throws TypeError on bigint; here bigint serializes by design (`toJson`). Representative: `rejected/JSON/stringify/value-bigint.js`. |
| `stringify/value-boolean-object.js`, `value-number-object.js`, `value-string-object.js` | No wrapper objects (`new Boolean(...)` etc.). |
| `stringify/value-function.js`, `value-symbol.js` | No first-class dropped-from-output values: functions aren't serializable operands and `Symbol` doesn't exist. |
| `stringify/value-tojson-*.js` | JS `toJSON` method protocol; the override hook here is a `toJson: () => string` field with different semantics (no key/receiver arguments, no dynamic dispatch on primitives). |
| `stringify/value-array-circular.js`, `value-object-circular.js` | Cycle detection via TypeError relies on reference identity; the test's cyclic constructions are ill-typed here. |
| `15.12-0-1.js` … `15.12-0-4.js`, `parse/builtin.js`, `stringify/builtin.js` | `JSON` is a compiler intrinsic, not a value — `typeof JSON`/property reflection cannot exist (bare `JSON` is a compile error). |
| `isRawJSON/**`, `rawJSON/**` | `JSON.rawJSON`/`JSON.isRawJSON` deferred (ES2025, niche — gaps doc §25). |

## Known gaps (`expect-fail`)

None — all 26 ported/divergence cases pass.

## Not ported (portable in principle, below the curation bar)

| test262 path / pattern | Reason |
|:--|:--|
| `parse/15.12.1.1-0-3.js` … `-0-8.js` | Same invalid-whitespace pattern as the ported `-0-2.js` and `invalid-whitespace.js` (FF/NBSP/ZWSP/BOM/U+2028/U+2029). |
| `parse/15.12.1.1-g2-3.js`, `-g2-4.js` | Same bad-string-delimiter pattern as the ported `-g2-2.js`. |
| `parse/15.12.1.1-g4-2.js` … `-g4-4.js` | Same raw-control-char pattern as the ported `-g4-1.js` (and the ported `15.12.2-2-1.ts` loops over all 32). |
| `parse/15.12.1.1-g5-3.js` | Same malformed-`\u` pattern as the ported `-g5-2.js`. |
| `parse/15.12.1.1-g6-3.js` … `-g6-6.js` | Same escape-character pattern as the ported `-g6-1/-g6-2/-g6-7` (`\b`, `\f`, `\n`, `\r`). |
| `parse/15.12.2-2-2.js` … `-2-10.js` | Same control-char-in-token pattern as the ported `-2-1.js`. |

## Per-case adaptations

- `parse/text-negative-zero.ts` — dropped the original's final
  `JSON.parse(-0)` (ToString coercion of a number argument).
- `stringify/value-primitive-top-level.ts` preserves top-level undefined, which
  produces undefined rather than a JSON document.
- `stringify/value-number-negative-zero.ts` — heterogeneous `['-0', 0, -0]`
  split by element type.
- `stringify/value-string-escape-ascii.ts` — the original's computed
  property name is replaced by per-character assertions plus one
  string-literal property name.
- `parse/15.12.1.1-0-9.ts` — the heterogeneous `[true, null, 123.456]`
  parses into `(boolean | number | null)[]` via a type alias.
- SyntaxError/TypeError distinctions are erased: `assertThrows` matches the
  base `Error` only.
