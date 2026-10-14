# Object statics — keys / values / entries / hasOwn / is

Triage of `test/built-ins/Object/{keys,values,entries,hasOwn,is}` (185 files).
26 ported (+3 divergence pins under `cases/Object/divergence/`), 6 rejected
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

Undefined equality is covered by `is/same-value-x-y-undefined.ts`, including an
omitted second argument, and by the restored primitive mixed-type cases.
Undefined receivers throw like null ones: `keys/15.2.3.14-1-5.ts`,
`hasOwn/toobject_undefined.ts`, and both halves of
`values|entries/exception-not-object-coercible.ts`.

## Object.fromEntries (SUB-1708)

The original Object inventory above selected only enumeration and comparison
statics. SUB-450 explicitly deferred `fromEntries` while objects had fixed
shapes; it was absent from the curated suite, rather than passing upstream tests.
The 25 upstream `fromEntries` files were inventoried separately for SUB-1708.

Ported: `empty-iterable.js`, `iterator-not-closed-for-throwing-next.js`,
`iterator-not-closed-for-throwing-done-accessor.js`,
`iterator-closed-for-throwing-entry-key-tostring.js`, and
`key-order.js` (using `Object.keys` instead of `Object.getOwnPropertyNames`). Iterator protocol names
are adapted from `Symbol.iterator`/`return` to `iterator`/`close`.
Interpreter fixtures additionally cover Maps, arrays, readonly tuples, explicit
value type arguments, duplicate keys, numeric keys, `__proto__`, undefined/null
values, lone UTF-16 surrogates, and original-error precedence when cleanup throws.
These implementation regression fixtures are separate from upstream conformance ports.

| Remaining upstream cases | Reason |
|:--|:--|
| `length.js`, `name.js`, `not-a-constructor.js`, `prototype.js`, `simple-properties.js` | Reflection and property descriptors follow the blanket exclusions. |
| `supports-symbols.js`, `to-property-key.js` | Symbols and `Symbol.toPrimitive` are unsupported. |
| `string-entry-object-succeeds.js`, `string-entry-string-object-succeeds.js` | Boxed strings and `Object(...)` construction are unsupported. |
| `uses-define-semantics.js` | Prototype descriptors are unsupported; the fixture checks `__proto__` as an ordinary own field. |
| `uses-keys-not-iterator.js` | Original uses numeric object fields and built-in iterator tampering; entry arrays are read by index in the implementation. |
| `requires-argument.js`, `string-entry-primitive-throws.js`, `iterator-closed-for-null-entry.js`, `iterator-closed-for-string-entry.js` | Statically invalid arguments/entries are excluded by the typed entry signature. |
| `evaluation-order.js`, `iterator-closed-for-throwing-entry-key-accessor.js`, `iterator-closed-for-throwing-entry-value-accessor.js` | Original relies on numeric entry accessors or dynamic coercion hooks outside the directly typed entry surface. |
| `iterator-not-closed-for-next-returning-non-object.js`, `iterator-not-closed-for-uncallable-next.js` | Invalid iterator protocol implementations are rejected statically. |

## Object.assign and Object.groupBy

The 38 upstream `assign` files and 14 upstream `groupBy` files were inventoried
from `tc39/test262/test/built-ins/Object/`. Ports retain upstream basenames and
provenance headers. These cases join the existing nightly conformance runner;
the repository's nightly-only suite gate is unchanged.

`assign` ports: `Target-Object`, `Source-Null-Undefined`, `Source-String`,
`ObjectOverride-sameproperty`, `Override`, and `target-set-user-error`.
`{}` replaces `new Object()`, string indexes replace numeric index spelling,
and `Object.keys` replaces `getOwnPropertyNames` when all properties involved
are enumerable. A class setter replaces descriptor installation in the setter
error case; it preserves the assignment failure being tested.

Remaining `assign` material: descriptor/name/length/constructor reflection;
primitive target wrappers; symbols and proxies; frozen/sealed/non-extensible
objects and readonly descriptors; descriptor-driven source getters and changed
own-key metadata; the array-target original's sparse arrays, arbitrary named
array properties, and Proxy-driven length/index reordering. Dense array copying,
resize/extension, target aliasing, partial mutation on failure, private fields,
and UTF-16 preservation are covered separately by interpreter regression fixtures.
`Source-Number-Boolen-Symbol` combines supported primitive sources with rejected
symbols, so its original is not ported.

`groupBy` ports: `evenOdd`, `groupLength`, `string`, `emptyList`, `callback-arg`,
`callback-throws`, `iterator-next-throws`, `invalid-property-key`, and
`toPropertyKey`. Rest parameters represent `arguments`; iterator()/close()
represent Symbol.iterator/return. Typed Error subclasses retain thrown-error
identity. `toPropertyKey` uses string-valued `toString` (required by the existing
conversion-hook contract), retaining heterogeneous key coercion and group values.

Remaining `groupBy` material: `length`/`name` reflection; `null-prototype`
(prototype introspection is unavailable); `invalid-callback` and
`invalid-iterable` (statically invalid call/protocol shapes). The original
number-valued `toString` variant is outside the existing conversion-hook contract.
