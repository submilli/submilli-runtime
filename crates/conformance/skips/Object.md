# Object statics — keys / values / entries / hasOwn / is

Triage of `test/built-ins/Object/{keys,values,entries,hasOwn,is}` (185 files).
21 ported (+3 divergence pins under `cases/Object/divergence/`), 7 rejected
originals copied under `rejected/Object/`. Blanket rules (SKIPS.md) cover
`length.js`, `name.js`, `not-a-constructor.js`, `prop-desc`-style descriptor
verification (`object-is.js`, `function-property-descriptor.js`,
`descriptor.js`, `prototype.js`), `Symbol.*` cases (`*symbol*`), and
coercion-trap cases (`toobject_before_topropertykey.js`) without further note.

## Rejected (design decisions)

| Pattern | Reason |
|:--|:--|
| `keys/return-order.js`, `values/return-order.js`, `entries/return-order.js`, `keys/order-after-define-property*.js`, `entries/order-after-define-property*.js`, `keys/15.2.3.14-6-*.js` | Expect integer-keys-then-insertion enumeration order; our objects have no insertion order — enumeration is canonical sorted key order (divergence pinned in `cases/Object/divergence/keys-canonical-sorted-order.ts`). Copied: `rejected/Object/keys/return-order.js`. |
| `is/not-same-value-x-y-object.js` | Expects JS reference identity; our `Object.is` follows the structural `===` (divergence pinned in `cases/Object/divergence/is-structural-objects.ts`). Copied. |
| `*undefined*` (`keys/15.2.3.14-1-5.js`, `is/same-value-x-y-empty.js`, `is/same-value-x-y-undefined.js`, `is/not-same-value-x-y-undefined.js`, `is/not-same-value-x-y-null.js` (one-arg form), `hasOwn/toobject_undefined.js`) | About `undefined` semantics (or absent-argument-as-`undefined`); the language has `null` only and `Object.is` requires both arguments. Copied: `rejected/Object/is/same-value-x-y-undefined.js`. |
| Descriptor-driven cases (`keys/15.2.3.14-2-7.js`, `-2-8`, `-3-7`, `-4-1`, `-5-1..5-16`, `values|entries/getter-*.js`, `observable-operations.js`, `exception-during-enumeration.js`, `hasOwn/hasown_{own,inherited}_{getter,setter,writable,nonwritable}*.js`, `hasOwn/descriptor.js`) | `Object.defineProperty`, enumerability attributes, getters/setters — no property-descriptor model. Copied: `rejected/Object/keys/15.2.3.14-2-7.js`. |
| Prototype-chain cases (`hasOwn/hasown_inherited_exists.js` + the inherited-descriptor family, `values|entries/inherited-properties-omitted.js`) | `Object.create` / constructor-function prototypes — no prototypes; objects have only declared own fields. Copied: `rejected/Object/hasOwn/hasown_inherited_exists.js`. |
| String-receiver enumeration (`values|entries/primitive-strings.js`, `keys/15.2.3.14-5-15.js`, `-5-16`, `-6-3`) | JS enumerates a string's index keys; non-object receivers yield `[]` here (pinned in `cases/Object/divergence/values-erased-to-unknown.ts`). The no-throw intent survives as `keys/15.2.3.14-1-3.ts`. Copied: `rejected/Object/values/primitive-strings.js`. |
| Array/arguments receivers (`keys/15.2.3.14-3-3.js`, `-3-4`, `-5-13`, `-5-14`, `-6-1`, `-6-2`) | JS yields index keys for arrays (plus sparse-array holes, `arguments` object); arrays are not structural objects here and yield `[]`. |
| Built-in tampering (`values|entries/tamper-with-object-keys.js`, `tamper-with-global-object.js`, `keys/15.2.3.14-2-3.js`) | Reassigning built-ins (`Object.keys = …`, shadowing `Array`); built-ins are not mutable bindings. Copied: `rejected/Object/values/tamper-with-object-keys.js`. |
| Proxy cases (`keys/proxy-*.js`, `keys/property-traps-order-with-proxied-array.js`) | No `Proxy`. |
| Reflection on the result array (`keys/15.2.3.14-2-2.js` ([[Class]]), `-2-4` (isExtensible), `-2-5` (isSealed), `-2-6` (isFrozen), `-3-6` (instanceof)) | `Object.prototype.toString.call`, seal/freeze, `instanceof` — none exist. |
| Function-object cases (`keys/15.2.3.14-0-1.js`, `-0-2`, `-3-2`, `hasOwn/hasown.js`, `values|entries/function-length.js`, `function-name.js`) | Functions are not property-bearing objects; `typeof` is a narrowing guard only. |

## Not ported (below the curation bar)

| Pattern | Reason |
|:--|:--|
| `keys/15.2.3.14-1-2.js` (boolean receiver doesn't throw) | Redundant with the ported `-1-1` (number) and `-1-3` (string). |
| `keys/15.2.3.14-2-1.js` (result is an Array) | Redundant with the ported `values|entries/primitive-booleans.ts`, which pin `Array.isArray` on the result. |
| `values|entries/exception-not-object-coercible.js` (null/undefined throw) | The `null` half is covered by the ported `keys/15.2.3.14-1-4.ts` and `hasOwn/toobject_null.ts`; the `undefined` half is rejected. |
| `values|entries/symbols-omitted.js`, `primitive-symbols.js`, `is/*symbol*` | Blanket `Symbol` rejection. |

## Ported notes

- `assertThrows` matches the base `Error`; the original `TypeError`
  distinction in `keys/15.2.3.14-1-4.js` and `hasOwn/toobject_null.js` is
  erased (noted in each case).
- `is/same-value-x-y-string.js`: the `String('foo')` wrapper became a
  computed string concatenation (same code-unit-sequence intent).
- `is/same-value-x-y-object.js`: self-comparison still holds, but under
  structural equality rather than identity; the identity-negative
  counterpart is rejected (see above).
