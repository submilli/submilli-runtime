# Map — skipped and rejected test262 material

Source: `test/built-ins/Map/**`. Ported cases live under `cases/Map/`;
design-divergence vectors under `cases/Map/divergence/`. Blanket rules from
`SKIPS.md` (prop-desc, length, name, not-a-constructor, this-coercion,
Symbol.*, species, newtarget, prototype-chain, return-abrupt) are not
repeated below.

Our `Map` deliberately diverges from ECMA-262 (spec.md §2.7): keys are
statically typed and compare via the structural equals/hash vtable, not
SameValueZero reference identity. Counter-vector for the keying divergence:
`cases/Map/divergence/structural-object-keys.ts`.

## Rejected (design decision; representatives under `rejected/Map/`)

| test262 path / pattern | Reason |
|:--|:--|
| `valid-keys.js` | Symbol/undefined/TypedArray/WeakRef keys plus reference-identity object keying. Representative: `rejected/Map/valid-keys.js`. |
| `bigint-number-same-value.js` | A single Map cannot mix number and bigint keys — keys are statically typed. Representative: `rejected/Map/bigint-number-same-value.js`. |
| `prototype/forEach/callback-parameters.js`, `second-parameter-as-callback-context.js`, `callback-this-*.js` | `Map#forEach` calls the callback as `(value, key)` by design — no third collection argument, no `thisArg` (spec.md §2.7). Representatives: `rejected/Map/prototype/forEach/callback-parameters.js`, `callback-this-strict.js`. |
| `iterable-calls-set.js`, `map-iterable-empty-does-not-call-set.js`, `map-no-iterable-does-not-call-set.js`, `does-not-throw-when-set-is-not-callable.js`, `get-set-method-failure.js`, `map-iterable-throws-when-set-is-not-callable.js`, `iterator-*-failure.js`, `iterator-close-*.js`, `iterator-is-undefined-throws.js`, `iterator-item-*.js`, `iterator-items-*.js` | Construction observes dynamic dispatch on `this.set` and the `Symbol.iterator` close/abrupt protocol — no prototypes, no patchable methods. |
| `prototype/*/does-not-have-mapdata-internal-slot*.js`, `prototype/*/context-is-*.js`, `prototype/*/this-not-object-throw.js` | Internal-slot/this-coercion checks; cross-type calls are compile errors here. |

Keys compare by SameValueZero, as in the standard: `NaN` is one key, and a
`-0` key is stored as `+0`. The interpreter fixture
`collection_keys_same_value_zero.ts` pins the `-0` normalization, which the
test262 vectors (`Set/prototype/*/converts-negative-zero.js`) only reach through
set-like arguments.

## Not ported (portable in principle, below the curation bar)

| test262 path / pattern | Reason |
|:--|:--|
| `map-no-iterable.js`, `map.js` | `new Map()` size-0 behavior covered by the ported size cases. |
| `groupBy/**` | `Map.groupBy` deferred (docs/ecma-262-gaps.md §24). |
| `prototype/get/getOrInsert/**`, `getOrInsertComputed/**` | Methods don't exist yet (upstream proposal); not in spec.md §2.7. |
| `prototype/has/return-true-different-key-types.js` | NaN-positive portion duplicates the ported `get/returns-value-different-key-types.ts`; the rest duplicates the ported has cases. |
| `prototype/set/append-new-values-return-map.js`, `replaces-a-value-returns-map.js` | `set` returns the receiver — reference-identity asserts don't port; chainability is covered by the adapted `Set/prototype/add/returns-this.ts`. |
| `prototype/set/append-new-values-normalizes-zero-key.js` | Same get-after-±0-set mechanism as the ported `get/returns-value-normalized-zero-key.ts`. |
| `prototype/size/returns-count-of-present-values-by-insertion.js`, `by-iterable.js` | Keys are `0, undefined, false, NaN, null, '', Symbol()` — the undefined/Symbol keys are rejected by design and the null-key portion is covered by the ported `set/append-new-values.ts`. |
| `prototype/forEach/iterates-values-deleted-then-readded.js` | Same live-iteration mechanics as the ported `iterates-values-added-after-foreach-begins.ts`; the Set-side variant is ported as `cases/Set/prototype/forEach/iterates-values-deleted-then-readded.ts`. |
| `prototype/forEach/callback-result-is-abrupt.js`, `first-argument-is-not-callable.js`, `return-undefined.js` | Throw-propagation is generic try/catch behavior; non-callable arguments are compile errors; `forEach` returns void. |
| `prototype/keys/returns-iterator.js`, `values/returns-iterator.js`, `entries/returns-iterator-empty.js`, `keys|values/returns-iterator-empty.js` | Same cursor mechanics as the ported `entries/returns-iterator.ts` (and `Set/prototype/values/returns-iterator.ts`). |
| `prototype/clear/returns-undefined.js`, `clear.js`, `map-data-list-is-preserved.js` | `clear` returns void; the data-list identity is unobservable without live iterators. |
| `prototype/delete/delete.js` | Descriptor-only check. |

## Per-case adaptations

- `undefined` → `null` throughout (`get/returns-undefined.ts`).
- Heterogeneous keys/values use union key types (`type Key = string | number | …`);
  `(T | U)[]` must be spelled through a type alias — the parser reads a
  parenthesized union element type as a function-type parameter list.
- Exhausted iterator results carry no `value` field (instead of JS's
  `value: undefined`); ports assert `!("value" in result)`. Yield results
  are narrowed via `"value" in r` and copied to a local first, because any
  call invalidates the narrowing.
- Mixed-entry `new Map([...])` initializers are rewritten as `set()` calls
  except in `clear/clear-map.ts`, which pins the codegen gap.
- TypeError distinctions are erased: `assertThrows` matches the base `Error`.
