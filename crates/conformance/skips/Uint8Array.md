# Uint8Array — skipped and rejected test262 material

Source: `test/built-ins/Uint8Array/**` plus the `%TypedArray%` material in
`test/built-ins/TypedArray/prototype/**` and `test/built-ins/TypedArrayConstructors/**`,
filtered to the Uint8Array element type. Ported cases live under
`cases/Uint8Array/`; design-divergence vectors under `cases/Uint8Array/divergence/`.
Blanket rules from `SKIPS.md` (prop-desc, length, name, not-a-constructor,
this-coercion, Symbol.*, species, prototype-chain, return-abrupt) are not
repeated below; the area's spellings of the same material
(`invoked-as-func`/`invoked-as-method`, `ignores-receiver`, `nonconstructor`,
`descriptor`, `this-is-not-typedarray-instance`,
`get-length-uses-internal-arraylength` / `get-length-ignores-length-prop`)
fall under them.

Our `Uint8Array` deliberately diverges from ECMA-262 (spec.md §1.2): it is a
primitive packed-byte type that **owns its storage** — there is no
`ArrayBuffer`, no views, no `.buffer`/`.byteOffset`. `subarray` is a deep
copy, and the base64/hex codecs are the
`fromBase64`/`fromHex` statics + `toBase64`/`toHex` methods with a typed
`Base64Options` (`alphabet?`, `omitPadding?` — no `lastChunkHandling`).
Counter-vector: `divergence/subarray-deep-copy.ts`. Byte writes go through
ToUint8 as in the standard (`-1` stores 255); the interpreter fixture
`uint8array_wraps_like_to_uint8.ts` pins it.

## Rejected (design decision; representatives under `rejected/Uint8Array/`)

| test262 path / pattern | Reason |
|:--|:--|
| `TypedArray/**` and `Uint8Array/**` cases mentioning `ArrayBuffer`/`SharedArrayBuffer`/`DataView`: `detached-buffer*.js`, `resizable-buffer*.js`, `immutable-buffer.js`, `*-resize*.js`, `*-grow*.js`, `*-shrink*.js`, `*-detach*.js`, `*-sab.js`, `buffer/**`, `byteLength/**`, `byteOffset/**`, `subarray/result-is-new-instance-with-shared-buffer.js`, `set/typedarray-arg-set-values-same-buffer-*.js`, … | No buffer backing at all — `Uint8Array` owns its storage. Representatives: `rejected/Uint8Array/prototype/toBase64/detached-buffer.js`, `rejected/Uint8Array/prototype/subarray/result-is-new-instance-with-shared-buffer.js`, `rejected/Uint8Array/prototype/set/typedarray-arg-set-values-same-buffer-other-type.js`. |
| `TypedArray/**` / `TypedArrayConstructors/**` material for other element types (`Int8Array`…`Float64Array`, `BigInt64Array`, `Uint8ClampedArray`, the `BigInt/` subdirectories, `ctors*/**`, `from/**`, `of/**`, internals) | Only `Uint8Array` exists; the rest of the family (and the abstract `%TypedArray%` base, `Symbol.iterator`, `Symbol.toStringTag`, `keys`/`values`/`entries` iterators, `toLocaleString`) is deferred or rejected (docs/ecma-262-gaps.md §23). |
| Byte-conversion coercion vectors (`set/array-arg-src-tonumber-value-type-conversions.js`, `fill/fill-values-conversion-operations*.js`, `set/array-arg-src-tonumber-value-conversions.js`, `with/early-type-coercion.js`, `with/index-casted-to-number.js`) | Writes take typed `number`s — no ToNumber coercion of strings/objects/booleans. Representative: `rejected/Uint8Array/prototype/set/array-arg-src-tonumber-value-type-conversions.js`. |
| `Uint8Array/prototype/setFromBase64/**`, `setFromHex/**` | In-place decode into an existing array (offset/written bookkeeping, partial writes into views) is absent by design; decoding goes through the `fromBase64`/`fromHex` statics, which return fresh arrays. Representatives: `rejected/Uint8Array/prototype/setFromBase64/results.js`, `rejected/Uint8Array/prototype/setFromHex/results.js`. |
| `fromBase64/option-coercion.js`, `fromBase64/string-coercion.js`, `toBase64/option-coercion.js`, `toBase64/receiver-not-uint8array.js`, `toHex/receiver-not-uint8array.js` | Option-bag getter traps, ToString coercion, and receiver-brand checks; arguments are statically typed. Representative: `rejected/Uint8Array/fromBase64/option-coercion.js`. |
| `TypedArray/prototype/*/predicate-call-parameters.js`, `callbackfn-arguments-*.js`, `callbackfn-this.js`, `predicate-call-this-*.js`, `*-is-not-callable*.js`, `callbackfn-returns-abrupt.js`, `return-abrupt-from-predicate-call.js` | `arguments` objects, `thisArg`, non-callable arguments, and abrupt-completion plumbing — all compile errors or generic try/catch behavior here. |
| `at/index-non-numeric-argument-tointeger*.js`, `*/tointeger-*.js`, `*/fromIndex-infinity.js`, `*/fromIndex-minus-zero.js`, `*/coerced-*.js`, `with/valid-typedarray-index-checked-after-coercions.js`, `with/order-of-evaluation.js` | ToInteger coercion of non-number index arguments (strings, objects, `undefined`); indexes are typed `number`. |
| `at/returns-undefined-for-out-of-range-index.js`, `find/return-undefined-if-predicate-returns-false-value.js` (and friends asserting `undefined`) | No `undefined`: `at` and `find*` return `T \| null`. The null-returning behavior is asserted in the ported find/findLast cases and the interpreter fixture `uint8array_at.subm`. |

## Known gaps (`expect-fail`)

| Case | Gap |
|:--|:--|
| `cases/Uint8Array/fromBase64/whitespace.ts` | `fromBase64` rejects ASCII whitespace (space/tab/LF/FF/CR) inside the input; the standard ignores it. |
| `cases/Uint8Array/fromBase64/last-chunk-handling-trailing-bits.ts` | `fromBase64` rejects a final chunk with non-zero padding bits (`'ZXhhZh=='`); the standard's default loose handling discards the extra bits. |
| `cases/Uint8Array/prototype/with/index-bigger-or-eq-than-length.ts` | `with(index)` past the end raises an uncatchable Wasm trap; the standard throws a catchable RangeError (indexed access already throws a catchable Error). |

## Not ported (portable in principle, below the curation bar)

| test262 path / pattern | Reason |
|:--|:--|
| `fromBase64/last-chunk-handling.js`, `last-chunk-invalid.js` — the `lastChunkHandling` option rows | `Base64Options` has no `lastChunkHandling` (spec.md §1.2): `strict` and `stop-before-partial` are inexpressible; an option literal carrying the field is a compile error. The default-mode rows are ported in `last-chunk-handling.ts` / `last-chunk-invalid.ts`; the loose trailing-bits rows are the pinned gap above. |
| `toBase64/omit-padding.js` ToBoolean rows (`{ omitPadding: 0 }`) | `omitPadding` is typed `boolean`; numbers never coerce. Valid rows ported in `omit-padding.ts`. |
| `toBase64/alphabet.js` invalid-alphabet row | Statically rejected; pinned by `cases/Uint8Array/prototype/toBase64/alphabet-invalid.ts` (`expect-error`). |
| `sort/sorted-values.js` float/negative/Infinity/NaN blocks | Other element types; the byte-valued blocks are ported in `sort/sorted-values.ts`. |
| `indexOf`/`includes`/`lastIndexOf` `strict-comparison.js`, `length-zero-returns-*.js`, `fromIndex-equal-or-greater-length-returns-*.js`, `no-arg.js` | Strict-comparison vectors mix element types (`"42"`, `undefined`); the in-range behavior duplicates the ported search cases and the interpreter fixtures. |
| `slice/results-with-different-relative-indexes.js`, `arraylength-internal.js`, `bit-precision.js`, `slice/result-does-not-copy-ordinary-properties.js` | Covered by the two ported slice cases; expando properties don't exist. |
| `copyWithin/negative-target.js`, `negative-end.js`, `negative-out-of-bounds-*.js`, `non-negative-*.js` (rest), `undefined-end.js`, `return-this.js`, `fill/return-this.js`, `fill/absent-indices-computed-from-initial-length.js`, `fill/fill-values-custom-start-and-end.js` | Same clamping algebra as the two ported copyWithin cases and the fill trio; `returns this` is reference identity (covered value-wise by interpreter fixtures). |
| `reverse/returns-original-object.js`, `toReversed/ignores-species.js`, `length-property-ignored.js`, `find/predicate-not-called-on-empty-array.js`, `findIndex/**`, `findLastIndex/**`, `every/**`, `some/**`, `forEach/**`, `map/**`, `filter/**`, `reduce/**`, `reduceRight/**`, `entries/**`, `keys/**`, `values/**` | Reference-identity asserts, or surface already exercised by the ported find/findLast cases and the substantial interpreter fixtures (`uint8array_*.subm`); iterators are rejected above. |
| `toString.js`, `toLocaleString/**`, `join/custom-separator-*.js`, `join/get-length-uses-internal-arraylength.js` | `toString` is covered by `uint8array_to_string.subm`; locale is rejected; custom-separator vectors duplicate `uint8array_join.subm`. |
| `Uint8Array` constructor cases (`TypedArrayConstructors/Uint8Array/**`) | Constructor overloads (length / buffer / iterable / species) don't exist — construction is `new Uint8Array(number[])` + statics (`alloc`, `of`, `fromArray`, `fromBytes`); covered by interpreter fixtures. |

## Per-case adaptations

- `testWithTypedArrayConstructors(...)` harness loops are instantiated at
  `Uint8Array` directly; provenance points at the original
  `test/built-ins/TypedArray/...` path.
- `assert.compareArray(typedArray, [...])` becomes a local
  `assertBytes(actual: Uint8Array, expected: number[], msg)` helper —
  the shim's `assertCompareArray` takes `T[]`, not `Uint8Array`.
- `.buffer.byteLength` assertions are dropped (no buffer backing);
  `new TA(0)` becomes `Uint8Array.alloc(0)`.
- `notSameValue(result, receiver)` identity checks (toReversed/toSorted/with
  `immutable.js`) are rewritten as mutation probes: write to the result,
  assert the receiver unchanged.
- `set` return value asserts (`returns undefined`) are dropped — `set`
  returns `void`.
- SyntaxError/TypeError/RangeError distinctions are erased: `assertThrows`
  matches the base `Error`.
- JS `undefined` results (`find` miss) map to `null`.
