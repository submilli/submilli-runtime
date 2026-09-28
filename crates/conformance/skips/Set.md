# Set — skipped and rejected test262 material

Source: `test/built-ins/Set/**`. Ported cases live under `cases/Set/`;
design-divergence vectors under `cases/Set/divergence/`. Blanket rules from
`SKIPS.md` (prop-desc, length, name, not-a-constructor, this-coercion,
Symbol.*, species, newtarget, prototype-chain, return-abrupt) are not
repeated below.

Our `Set` deliberately diverges from ECMA-262 (spec.md §2.7): elements are
statically typed and compare via the structural equals/hash vtable, not
SameValueZero reference identity; cursors are lazy *snapshots*; the ES2025
set-algebra methods take `Set` arguments only — no set-like
(`size`/`has`/`keys`) protocol. Counter-vector for the keying divergence:
`cases/Set/divergence/structural-object-elements.ts`.

## Rejected (design decision; representatives under `rejected/Set/`)

| test262 path / pattern | Reason |
|:--|:--|
| `union|intersection|difference|symmetricDifference|isSubsetOf|isSupersetOf|isDisjointFrom/allows-set-like-*.js`, `set-like-*.js`, `combines-Map.js`, `compares-Map.js`, `called-with-object.js`, `array-throws.js`, `size-is-a-number.js`, `has-is-callable.js`, `keys-is-callable.js`, `converts-negative-zero.js`, `add-not-called.js`, `builtins.js`, `receiver-not-set.js`, `require-internal-slot.js`, `subclass*.js` | Set algebra takes `Set` arguments only in v1 (spec.md §2.7) — no set-like protocol, no subclassing, no method-patching observation. Representatives: `rejected/Set/prototype/union/allows-set-like-object.js`, `converts-negative-zero.js`. |
| `prototype/has/*-symbol.js`, `*-undefined.js` | Symbol and undefined values are out of scope. Representatives: `rejected/Set/prototype/has/returns-true-when-value-present-symbol.js`, `returns-true-when-value-present-undefined.js`. |
| `valid-values.js`, `bigint-number-same-value.js` | Typed elements: no Symbol/undefined/TypedArray values, no number/bigint mixing, reference-identity object keying. (Map-side representatives: `rejected/Map/valid-keys.js`, `bigint-number-same-value.js`.) |
| `set-iterable-calls-add.js`, `set-iterable-empty-does-not-call-add.js`, `set-no-iterable-does-not-call-add.js`, `set-does-not-throw-when-add-is-not-callable.js`, `set-get-add-method-failure.js`, `set-iterable-throws-when-add-is-not-callable.js`, `set-iterator-*-failure.js`, `set-iterator-close-after-add-failure.js`, `set-like-iter-return.js` | Construction observes dynamic dispatch on `this.add` and the iterator close/abrupt protocol — no prototypes, no patchable methods. |
| `prototype/values/values-iteration-mutable.js` | Live-iterator mutation semantics — cursors here are documented snapshots (spec.md §2.7). |
| `prototype/forEach/this-*.js`, `callback-not-callable-*.js` | `thisArg`/this-binding protocol and non-callable arguments (compile errors here). |
| `prototype/*/does-not-have-setdata-internal-slot*.js`, `this-not-object-throw-*.js` | Internal-slot/this-coercion checks; cross-type calls are compile errors here. |

## Known gaps (`expect-fail`)

| Case | Gap |
|:--|:--|
| `cases/Set/prototype/has/returns-true-when-value-present-nan.ts` | NaN elements are unfindable: equality runs through the `equals` vtable, which uses IEEE `===` for numbers, not SameValueZero — `has(NaN)` is false after `add(NaN)`, and repeated adds duplicate the element. |
| `cases/Set/prototype/has/returns-true-when-value-present-null.ts` | `null` elements trap at runtime (equals/hash vtable dispatch on a null ref); the standard stores and finds null. |
| `cases/Set/prototype/forEach/iterates-values-deleted-then-readded.ts` | Elements added (or re-added) during a `forEach` are not visited — forEach walks a snapshot of the order ledger taken at call time. |
| `cases/Set/prototype/intersection/result-order.ts` | When `this.size > other.size`, the intersection result is ordered as in the receiver; the standard orders it as in the argument (the smaller side drives iteration). |

Found while porting, but not pinned by any portable vector: `add(-0)` stores
the element with its sign (JS normalizes to `+0`). `+0`/`-0` *equality* works
(dedupe and delete behave per SameValueZero), so only iteration plus
`Object.is` observes it — and the test262 vectors that do require set-like
arguments.

## Not ported (portable in principle, below the curation bar)

| test262 path / pattern | Reason |
|:--|:--|
| `set-no-iterable.js`, `set.js` | `new Set()` size-0 behavior covered by the ported size case. |
| `prototype/add/add.js`, `will-not-add-duplicate-entry-initial-iterable.js` | Descriptor-only / same dedupe pattern as the ported add cases. |
| `prototype/delete/delete-entry.js`, `delete-entry-initial-iterable.js`, `returns-true-when-delete-operation-occurs.js`, `returns-false-when-delete-is-noop.js`, `delete.js` | Covered by the ported `delete-entry-normalizes-zero.ts`, `size/…-add-delete.ts`, and the Map delete ports. |
| `prototype/has/returns-true|false-when-value-*-number|string|boolean.js`, `returns-false-when-value-not-present-null|undefined.js`, `has.js` | Trivial present/absent variants covered by the ported cases; the null/undefined-negative variants add nothing over the pinned gaps. |
| `prototype/clear/clears-an-empty-set.js`, `clears-all-contents-from-iterable.js`, `returns-undefined.js`, `clear.js` | Same pattern as the ported `clears-all-contents.ts`; `clear` returns void. |
| `prototype/size/returns-count-of-present-values-by-insertion.js`, `by-iterable.js` | Elements are `0, undefined, false, NaN, null, '', Symbol()` — undefined/Symbol rejected by design; the null portion is the gap pinned by `has/returns-true-when-value-present-null.ts`. |
| `prototype/forEach/iterates-values-added-after-foreach-begins.js`, `iterates-values-revisits-after-delete-re-add.js`, `iterates-values-not-deleted.js`, `iterates-in-iterable-entry-order.js`, `throws-when-callback-throws.js`, `returns-undefined.js`, `forEach.js` | Snapshot gap already pinned by the ported `iterates-values-deleted-then-readded.ts` (and the Map-side `iterates-values-added-after-foreach-begins.ts`); the rest are order/throw variants of ported cases. |
| `prototype/entries/**`, `prototype/keys/keys.js`, `prototype/values/returns-iterator-empty.js`, `values.js` | `keys` aliases `values` and `entries` yields `[v, v]` — parity is covered by the interpreter fixture `set_iteration_parity.subm` and the ported `values/returns-iterator.ts`. |
| `union/combines-sets.js`, `combines-empty-sets.js`, `combines-same-sets.js`, `combines-itself.js`, `appends-new-values.js`; `intersection|difference|symmetricDifference/combines-empty-sets.js`, `combines-itself.js`, `combines-same-sets.js`; `isSubsetOf|isSupersetOf|isDisjointFrom/compares-empty-sets.js`, `compares-itself.js`, `compares-same-sets.js`; `*/union.js` etc. | Covered by the ported result-order/combines-sets cases plus the interpreter fixtures `set_algebra.subm` / `set_relations.subm` (empty/self/equal-set edges). |

## Per-case adaptations

- `[...set]` spread → a `toArray` for-of collect; `expects.shift()` → index
  access (no `Array#shift`).
- `instanceof Set` assertions dropped — the type system fixes the return
  type as `Set<T>`.
- `add`/`clear` return-value identity asserts rewritten against observable
  mutation (`add/returns-this.ts`) or dropped (`clear` returns void).
- Exhausted iterator results carry no `value` field; ports assert
  `!("value" in result)`, and yield values are copied to a local because
  calls invalidate `in`-narrowing.
