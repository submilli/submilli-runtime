# String

Source: `test/built-ins/String/**` (1223 files). The instance surface is
broad (spec.md §1.7 + the regex-backed methods in docs/regex.md), so the
port covers every implemented method and both statics with at least one
behavioral case. 74 ported (64 passing, 10 `expect-fail`). Representative
rejected originals under `rejected/String/`.

Blanket rules (SKIPS.md) cover `length.js`, `name.js`, `prop-desc.js`,
`not-a-constructor.js`, `this-is-null-throws.js` / `this-is-undefined-throws.js` /
`this-value-not-obj-coercible.js` / `this-is-not-object.js`, the
`return-abrupt-*` coercion traps, every `Symbol.*` case
(`Symbol.iterator/**`, `symbol-*-coercion`, `cstm-*` custom-matcher
protocol cases for match/matchAll/search/replace), the `eval`-based Sputnik
cases, and the prototype-borrowing receivers (`__obj.charAt =
String.prototype.charAt` on Numbers/Booleans/Objects — ports use plain
string receivers instead). Representatives copied:
`rejected/String/prototype/charAt/S15.5.4.4_A5.js` (receiver coercion trap).

Porting adaptations used throughout (README rules):

- Lone-surrogate string literals (`'\uD800'`) in earlier ports are built with
  `String.fromCharCode(...)`, from when the lexer rejected unpaired surrogate
  escapes; it accepts them now. Affects at/codePointAt/padStart/padEnd/
  isWellFormed/toWellFormed ports.
- `\xHH` escapes are spelled `\u00HH` (no `\xHH` escape form).
- `new String(...)` / `new Object(...)` wrapper receivers become plain
  strings; coerced arguments collapse to their coercion results where the
  result, not the coercion, was the point.

## Known gaps (`expect-fail`)

| Case | Gap |
|:--|:--|
| `prototype/match/S15.5.4.10_A2_T3` | `match` with a g-flagged RegExp returns a single `RegExpMatch` instead of the array of all matched substrings (the prelude doc itself calls the g-flag array shape a follow-up). |
| `prototype/matchAll/flags-nonglobal-throws` | `matchAll` with a non-g RegExp should throw a TypeError; it matches the whole string anyway. Undocumented — neither spec.md nor docs/regex.md records the laxer behavior. |
| `prototype/replace/S15.5.4.11_A2_T4`, `_A2_T5` | The `` $` `` and `$'` replacement tokens pass through literally (`$&` too, probed separately); `$$` and `$N` do substitute. Contradicts the prelude doc-comment on `replace`, which claims all four tokens are honoured. |
| `prototype/replace/S15.5.4.11_A3_T1` | `$11` with only two capture groups should resolve as capture 1 + literal `"1"` (ECMA GetSubstitution); the match is replaced by the empty string instead (treated as missing group 11). |
| `prototype/repeat/count-is-infinity-throws` | `repeat(Infinity)` should throw a RangeError; the count saturates and `""` is returned for an empty receiver. (Negative counts do throw — `count-less-than-zero-throws` passes.) |
| `prototype/trim/15.5.4.20-3-2`, `-3-4`, `prototype/trimStart/this-value-whitespace`, `prototype/trimEnd/this-value-whitespace` | The trim family does not strip U+FEFF (ZWNBSP). ECMA WhiteSpace includes it; the host's Unicode White_Space set does not. Probed: U+FEFF is the only ECMA whitespace code point left untrimmed. |

## Rejected (design decisions)

| Pattern | Reason |
|:--|:--|
| `prototype/at/returns-undefined-for-out-of-range-index.js`, `prototype/codePointAt/returns-undefined-on-position-*.js`, `prototype/charAt` out-of-range-undefined variants | No `undefined`: `at()`/`charAt()` return `""`, `charCodeAt()`/`codePointAt()` return `NaN` out of range (spec.md §1.7). The NaN behavior is ported (`charCodeAt/S15.5.4.5_A{2,3}`); both undefined originals copied. |
| `prototype/match/S15.5.4.10_A2_T*.js` array-shape assertions (`m[0]`, `m.index` on the result array), `prototype/match/S15.5.4.10_A1_T*.js` | `RegExpExecArray`'s array-with-properties shape is replaced by the plain `RegExpMatch` interface (`.match`/`.index`/`.input`/`.groups`/`.namedGroups`) — documented divergence, docs/regex.md. Copied: `rejected/String/prototype/match/S15.5.4.10_A2_T10.js`. Basic match/search/matchAll behavior is exercised by the hand-written fixtures (`string_match_and_search.subm`, `string_match_all.subm`). |
| `raw/**` | `String.raw` / tagged templates out of scope. Copied: `rejected/String/raw/raw.js`. |
| `prototype/toLocaleLowerCase/**`, `prototype/toLocaleUpperCase/**`, `localeCompare` locale-dependent rows | No locale/ICU; `toLowerCase`/`toUpperCase` are already Unicode-correct. Copied: `rejected/String/prototype/toLocaleLowerCase/S15.5.4.17_A1_T1.js`. |
| `prototype/{includes,startsWith,endsWith}/searchstring-is-regexp-throws.js`, `return-abrupt-from-searchstring-regexp-test.js` | `searchString: string` — passing a RegExp is a compile-time type error, so the runtime `IsRegExp` TypeError path cannot exist. Copied: `rejected/String/prototype/endsWith/searchstring-is-regexp-throws.js`. |
| `S15.5.1.1_*` / `S15.5.2.1_*` / `S15.5.3*` / `S15.5.5*` / `S8.*` / `S9.*` (area root), `numeric-properties.js`, `proto-from-ctor-realm.js`, `is-a-constructor.js` | `new String(...)` boxing, `String(...)` general coercion semantics, prototype chains, realms, and indexed own-properties of wrapper objects — no wrapper objects, no prototypes. (`String(x)` exists but as `.toString()` sugar; covered by interpreter fixtures.) |

## Not in spec (members)

| Pattern | Reason |
|:--|:--|
| `prototype/substr/**` (Annex B, under `annexB/`) and Annex-B HTML methods | Annex-B legacy, rejected by the gap analysis (docs/ecma-262-gaps.md §22). |

## Not ported (below the curation bar)

| Pattern | Reason |
|:--|:--|
| `prototype/charAt/S15.5.4.4_A1*.js`, `prototype/charCodeAt/S15.5.4.5_A1*.js`, `S15.5.4.4_A4_T*.js`, `pos-rounding.js`, `pos-coerce-*.js` | Basic positive-index reads and pos coercion; the in-range behavior is covered by `at/returns-item*` and the hand-written `conformance_string.subm`; the coercion rows are type-rejected anyway. |
| `prototype/indexOf/S15.5.4.7_A1_T*.js`, `A3_T*.js`, `A5_T*.js`, `lastIndexOf/S15.5.4.8_A1_T*.js` (eval/coercion variants) | Same intent as the ported `position-tointeger` numeric subset and `S15.5.4.7_A2_T1`; remaining rows differ only in coercion vehicle. |
| `prototype/indexOf/position-tointeger.js` non-numeric rows (strings, booleans, arrays, objects as position), `searchstring-tostring*.js` | Type-rejected (no implicit ToInteger/ToString of arguments); ported numeric subset keeps the truncation rows. |
| `prototype/slice/S15.5.4.13_A1_T*.js`, `A2_T{1,4,5,6,7,9}.js`, `A3_T*.js`; `substring/S15.5.4.15_A1_T*.js`, `A2_T{1..7,9}.js`, `A3_T*.js` | One-assert files repeating the same clamp table; the ported `A2_T2`/`A2_T8` (slice) and `A2_T8`/`A2_T10` (substring) plus `conformance_string.subm` cover whole-string, empty, swap, and negative cases. |
| `prototype/split/**` remaining ~120 files (regex separators per-pattern, instance-is-number coercions, `separator-undefined` variants) | Separator coercion and `undefined`-separator semantics are type-rejected; regex-separator behavior is RegExp-area material. Note: capture groups are NOT inserted between split parts (documented divergence in the prelude doc), so the capture-group split cases (`argument-is-regexp-reg-exp-d-*`) would pin that divergence — left to the RegExp area. |
| `prototype/trim/15.5.4.20-1-*.js`, `-2-*.js`, `-4-*.js` | Receiver-coercion matrix (booleans, numbers, objects with custom toString) and per-character singletons; the ported `-3-2`/`-3-4` aggregate the full whitespace set. |
| `prototype/toLowerCase/S15.5.4.16_A1_T*.js`, `toUpperCase/S15.5.4.18_A1_T*.js`, `Final_Sigma_U180E.js`, `special_casing_conditional.js` | Coercion variants of ASCII casing (covered by `conformance_string.subm`) and conditional SpecialCasing (Final_Sigma) — the ported `special_casing` + `supplementary_plane` pairs carry the Unicode-correctness intent. |
| `prototype/normalize/return-normalized-string-from-coerced-form.js` | Form argument coercion (arrays/objects coercing to "NFC" etc.) — type-rejected. |
| `prototype/replace/S15.5.4.11_A1_T*.js`, `A4_T*.js`, `A5_T1.js`, `A12.js`, `15.5.4.11-1.js`, `replaceValue-evaluation-order*.js`, `regexp-capture-by-index.js`; `replaceAll/replaceValue-call-*.js`, `replaceValue-fn-*.js`, `getSubstitution-*.js`; `match`/`search`/`matchAll` `cstm-*`, `invoke-builtin-*`, `regexp-prototype-*` | Function replacers (deferred, docs/regex.md), Symbol protocol invocation order, and replaceValue coercion — not expressible; the `getSubstitution` token semantics that are expressible are pinned by the ported `S15.5.4.11_A2/A3` family (three of which are `expect-fail`, see above). |
| `prototype/endsWith/String.prototype.endsWith_*.js`, `includes/String.prototype.includes_*.js`, `startsWith/startsWith.js`, `endsWith.js`, `includes.js`, `coerced-values-of-position.js` | Same search semantics as the ported found/not-found/empty/out-of-bounds quartets; coerced-position rows are type-rejected. |
| `prototype/isWellFormed/to-string*.js`, `toWellFormed/to-string*.js` | Receiver ToString coercion — type-rejected; the behavioral `returns-*` cases are ported. |
| `fromCodePoint/to-number-conversions.js`, `argument-is-Symbol.js`, `argument-not-coercible.js`; `fromCharCode/S15.5.3.2_A1.js`, `A4.js`, `S9.7_A3.*.js`, `touint16-tonumber-throws-*.js` | Argument coercion (booleans, strings, Symbols, valueOf traps) — type-rejected; numeric behavior is ported. |
