# Array

Source: `test/built-ins/Array/**` (3081 files — the largest area). The port
covers every implemented instance method (spec.md §1.2: push/pop/shift/
unshift/splice, at/slice/concat, indexOf/lastIndexOf/includes, the find
family, map/filter/reduce/reduceRight/forEach/some/every, join/reverse/sort,
fill/copyWithin, flat/flatMap, keys/values/entries, toReversed/toSorted/
toSpliced/with) and the statics `Array.from`/`Array.of`/`Array.isArray` with
at least one behavioral case each. 71 ported (62 passing, 6 `expect-fail`,
3 `expect-error` incl. `cases/Array/divergence/`), plus the pre-existing
`includes/using-fromindex`. Representative rejected originals under
`rejected/Array/`.

Blanket rules (SKIPS.md) cover `length.js`, `name.js`, `prop-desc.js`,
`not-a-constructor.js`, `is-a-constructor.js`, `descriptor.js`,
`property-descriptor.js`, `this-is-not-object.js` / `this-value-nullish.js` /
`this-value-boolean.js` and the whole receiver-coercion matrix
(`15.4.4.x-1-*`, `-2-*`, `-3-*`: methods applied to undefined/null/booleans/
numbers/strings/array-likes via `Array.prototype.m.call(...)`), every
`return-abrupt-*` coercion trap, `Symbol.*` (`Symbol.iterator/**`,
`Symbol.unscopables/**`, `Symbol.species/**`), all `create-species-*` /
`create-ctor-*` / `create-proxy*` species-construction cases,
`proto-from-ctor-realm*`, proxy/revoked-proxy cases, `eval`-based Sputnik
cases, and `toLocaleString/**`.

Porting adaptations used throughout (README rules):

- Sparse arrays appear in many otherwise-behavioral Sputnik cases
  (`x[0] = 0; x[3] = 3`); ports keep the dense rows and drop the holes.
- `x[i] === undefined` probes past the end become `x.at(i) === null`
  (indexed OOB reads throw here — pinned in `cases/Array/divergence/`).
- HOF callbacks receive only the element: callbacks that branched on the
  `idx` argument are rewritten to branch on the element value over
  `[0, 1, 2, ...]` arrays, which preserves the visit-order/short-circuit
  intent. The (value, index, array) signature itself is pinned as a known
  gap (`find/predicate-call-parameters`).
- `reduce`/`reduceRight` ports pass an explicit initial value (required
  here); the absent-initial fold result is unchanged for the ported cases.
- `IteratorResult.value` is read through a runtime-checked
  `as IteratorYieldResult<T>` cast in the keys/values/entries iteration
  ports — `done` is typed `boolean`, not a literal, so it does not narrow
  the union (see "Shim/type-system limitations" below).

## Known gaps (`expect-fail`)

| Case | Gap |
|:--|:--|
| `prototype/includes/samevaluezero` | `includes` dispatches through the `equals` vtable (NaN !== NaN) instead of SameValueZero — `[NaN].includes(NaN)` returns `false`, standard says `true`. (`indexOf`'s StrictEquality NaN behavior is correct — `indexOf/15.4.4.14-9-10` passes.) |
| `prototype/indexOf/15.4.4.14-9-6` | Searching for `null` in a `(T \| null)[]` array traps **uncatchably** (null dereference in the equals dispatch) instead of returning the matching index. Affects `indexOf`/`includes` (probed); the null rows of the includes search-found/not-found ports are dropped and pinned here. Scanning *over* null elements with a non-null needle works. |
| `prototype/join/S15.4.4.5_A1.3_T1` | ECMA says undefined/null elements join as `""`; a null element makes `join` trap uncatchably. |
| `prototype/push/S15.4.4.7_A1_T2` | Standard `push` is variadic; ours is `push(elem: T)` — a multi-argument push is a compile-time arity error. (`unshift` *is* variadic.) |
| `prototype/concat/S15.4.4.4_A1_T2` | Standard `concat` appends non-array arguments as elements; ours is `concat(...others: T[][])` — a scalar argument is a compile-time type error. |
| `prototype/find/predicate-call-parameters` | Standard HOF callbacks receive `(value, index, array)`; every Array callback type here is single-parameter (`(T) => ...`), so a multi-parameter callback is a compile error. One representative pinned; the same applies to forEach/map/filter/some/every/findIndex/findLast(Index)/flatMap, `sort`'s comparator excepted. `Array.from` now supports type-changing `(T) => U` mappers, but still has no index argument. |

## Rejected (design decisions)

| Pattern | Reason |
|:--|:--|
| Sparse/holey-array cases everywhere (`sparse.js`, `fill-holes.js`, `holes-not-preserved.js`, `*-undefined-for-holes-*`, the `x[0]=0; x[3]=3` Sputnik bodies, `new Array(10)` length-only arrays, `length =` truncation tricks) | Holes are unrepresentable: OOB writes throw instead of sparse-extending (spec.md §1.2). Copied: `rejected/Array/prototype/includes/sparse.js`. |
| `undefined`-semantics cases (`at/returns-undefined-*`, pop/shift/find on empty returning undefined, `join(undefined)`, `Array.of(undefined, ...)`) | No `undefined`; `null` replaces it (`at`/`pop`/`shift`/`find` return `T \| null`). The null-returning behavior is ported. Copied: `rejected/Array/prototype/at/returns-undefined-for-out-of-range-index.js`. |
| Receiver-coercion matrix (`15.4.4.x-1-*` / `-2-*` / `-3-*`, `S15.4.4.x_A2_T*` generic-receiver rows, `throws-with-string-receiver.js`, `call-with-boolean.js`, array-like receivers with mutable `length`) | Array methods exist only on real arrays; `Array.prototype.m.call(obj)` has no equivalent (no prototypes). Copied: `rejected/Array/prototype/every/15.4.4.16-1-1.js`. |
| Species/ctor-realm construction (`create-species-*`, `create-ctor-*`, `create-proxy*`, `proto-from-ctor-realm*`, `this-value-ctor-*`) | No species constructors, prototypes, or realms; every producing method returns a plain array. Copied: `rejected/Array/prototype/slice/create-species.js`. |
| Mid-iteration length manipulation through getters/proxies (`length-decreased-while-iterating.js`, `comparefn-grow/shrink.js`, `coerced-*-resize.js`, `precise-getter-*`) | Requires property descriptors / array-like receivers; plain-array mutation during HOFs is exercised by the interpreter fixtures instead. Copied: `rejected/Array/prototype/toSpliced/length-decreased-while-iterating.js`. |
| Property descriptors (`property-descriptor.js`, `descriptor.js`, `prop-desc.js`, `set-length-*`, frozen/non-writable cases) | No descriptor machinery, no `Object.freeze`. Copied: `rejected/Array/prototype/with/property-descriptor.js`. |
| `reduce`/`reduceRight` absent-initial cases (`15.4.4.21-5-*`, `-8-b-*`, `-8-c-*`, `-10-2` original form) | `reduce` requires an explicit initial value by design (spec.md §1.2) — the absent-initial seed and its empty-array TypeError cannot exist; pinned by `cases/Array/divergence/reduce-requires-initial-value.ts`. Copied: `rejected/Array/prototype/reduce/15.4.4.21-8-c-1.js`. |
| Argument-coercion cases (`coerced-indexes.js`, `coerced-values-*.js`, `tointeger-fromindex.js` non-numeric rows, `index-casted-to-number.js`, `position-tointeger` analogues) | Non-numeric indexes/counts are compile-time type errors; the *numeric* truncation rows are ported (`slice/S15.4.4.10_A2.1_T1`, `splice/S15.4.4.12_A2.1_T1`). Copied: `rejected/Array/prototype/fill/coerced-indexes.js`. |
| `Symbol.iterator` protocol cases (`prototype/Symbol.iterator.js`, `Symbol.iterator/**`, `from/iter-get-iter-*`) | No `Symbol`; the named `.iterator()` protocol drives for-of and `Array.from` (ported `from/iter-map-fn-return`). Copied: `rejected/Array/prototype/Symbol.iterator.js`. |
| `[{}].includes({})`-style reference-identity rows, `reverse() === x` / `notSameValue(copy, original)` on structurally equal arrays | `Object.is`/`===` are structural here — identity assertions don't port (README caveat); rewritten against contents where possible, dropped otherwise. |

## Not in spec (members)

| Pattern | Reason |
|:--|:--|
| `fromAsync/**` (95 files) | Async-bound; `async`/`Promise` deferred (docs/ecma-262-gaps.md §23). |
| `Array(...)` / `new Array(...)` constructor cases (area root `S15.4.1*`, `S15.4.2*`, `property-cast-*`, `15.4.5*`) | No `Array` constructor calls — arrays come from literals and the statics; `new Array(len)` length-only form is sparse anyway. |
| `prototype/toString/**`, `prototype/toLocaleString/**` | `toString` rides the generic Object surface (covered by interpreter fixtures); no locale. |
| `prototype/sort/comparefn-nonfunction-call-throws.js` and friends | `sort(true)` etc. are compile-time type errors (`compareFn` is function-or-omitted); the runtime TypeError path cannot exist. |

## Not ported (below the curation bar)

| Pattern | Reason |
|:--|:--|
| The HOF bulk (`every`/`some`/`forEach`/`map`/`filter`/`reduce`/`reduceRight`, ~1600 files: `-7-b-*`/`-8-b-*` deleted/added-element visibility, `-7-c-i-*`/`-8-c-i-*` element-kind matrices, thisArg cases, callback-arity rows) | Per-method one strong order/short-circuit case is ported; the matrices vary receiver kind and coercion vehicle (rejected categories above), thisArg doesn't exist, and the callback-arity intent is pinned once by the expect-fail `find/predicate-call-parameters`. |
| `indexOf`/`lastIndexOf`/`includes` remainder (~380 files: per-type found/not-found rows, fromIndex coercion vehicles) | StrictEquality/SameValueZero intent carried by the ported NaN/±0/fromIndex cases plus `includes/using-fromindex`; remaining rows differ only in element kind or coercion vehicle. |
| `concat` remainder (Symbol.isConcatSpreadable, array-likes, large-index rows) | Spreadability is type-determined here (arrays spread, nothing else accepted); the scalar-argument gap is pinned by the expect-fail `S15.4.4.4_A1_T2`. |
| `sort` remainder (other `stability-*` sizes, `bug_596_*` ToString-call counts, comparator-throws ordering) | `stability-5-elements` pins stability; default lexicographic order is pinned by `toSorted/comparefn-default` (ours matches JS); comparator-abrupt cases are getter/this-coercion territory. |
| `splice`/`slice` Sputnik clamp tables (`S15.4.4.12_A1.*`, `S15.4.4.10_A1.*` remainder) | One-assert files repeating the same negative/clamp table; the ported start/negative/fractional/at-length cases plus `conformance_array.subm` cover the table corners. |
| `join` separator-coercion rows (`S15.4.4.5_A2_T*`, `A3.*`, `join(true)`, `join(Infinity)`) | Separator is `string?` — non-string separators are type errors. Default/custom/empty-array rows are ported; the null-element row is the expect-fail `A1.3_T1`. |
| `from`/`of` remainder (`iter-*` ctor/err protocol cases, `mapfn-is-not-callable`, `sets-length.js`, custom-`this` construction) | Iterator-protocol error paths and custom constructors are Symbol/species territory; the behavioral copy/string/iterable/mapper cases are ported. `Array.of` over mixed types needs an explicit union annotation (ported case). |
| `isArray` remainder (wrapper objects, proxies, host realms) | No wrapper objects/proxies/realms; true/false primitive cases ported. |
| `with` remainder (`index-casted-to-number`, `negative-fractional-index-truncated-to-zero`, `holes-not-preserved`, `ignores-species`, frozen) | Casting/holes/species/frozen are rejected categories; negative/OOB-throw cases are ported (ours throws the base `Error`, not `RangeError` — no error subclasses yet). |

## Shim/type-system limitations observed

- `IteratorResult<T>`'s `done` is typed `boolean` on both union members, so
  `if (r.done === false)` does **not** narrow to expose `value` — contrary
  to the prelude doc-comment ("discriminate on the `done` boolean"). Ports
  use a runtime-checked `as IteratorYieldResult<T>` cast instead.
- A call nested directly in `assertSameValue(...)` receives an `unknown`
  expected-type hint that pins generic returns (`reduce`'s `U` infers as
  `unknown` and then conflicts); ports assign to a `const` first.
- `console.log(null)` and `join` on a null element trap — `show()` in
  harness.ts handles null explicitly, so shim messages are safe.
