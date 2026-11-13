# SUB-2110: several index signatures on one object type

## Problem

An object type holds at most one index signature. So Submilli rejects
TypeScript programs that tsc accepts:

- `Object.assign(a, b)` with `a: Record<`a${string}`, number>` and
  `b: Record<`b${string}`, string>` ("Object.assign cannot combine index
  signatures with different key and value types").
- `{ [k: string]: string | number; [i: number]: number }` ("duplicate string
  index signature").
- `interface B extends A { [k: `b${string}`]: string }` when `A` has another
  domain ("incompatible inherited string index signatures").
- Spreads and computed literals that mix domains ("combining index signatures
  with different key and value types is not supported").

## Decisions

- **Representation**: ts-rust contract row CD-013 (approved 2026-11-13, comment
  on SUB-2110). `Type::Object` keeps `index: Option<IndexSignature>` and gains
  `#[serde(default, skip_serializing_if = "Vec::is_empty")] more_indexes:
  Vec<IndexSignature>`. `index` is the `string` signature if there is one,
  otherwise the first. `more_indexes` holds the rest, sorted by `IndexSignature`'s
  `Ord`. An object with one signature serializes exactly as it does today.
  Interface declarations (`TypeKind::Interface`, `TypedInterfaceDecl`,
  `InterfaceNarrowingTest`, `Shape::Object`) get the same field with the same
  serde attributes.
- **Spread matches tsc** (Doron, 2026-11-13). This replaces the documented
  retention rule. A spread result keeps a signature only when every source
  (the earlier result and the spread value, own properties counting as a
  source without signatures) has a signature of the same key type. Its value
  is the union of their values. Otherwise the signature is dropped, so
  `{ ...a, x: 1 }` is `{ x: number }` and `{ ...a, ...b }` is `{}`.
- Every other choice follows tsc 6 (`--strict`), as directed in the subset
  TS-compat decisions.

## Rules (tsc 6, checked with probes)

- **Applicable signature** (`findApplicableIndexInfo`): for a read key `K`, take
  every signature other than `string` whose key `K` is assignable to. One gives
  its value. Several give the intersection of their values, `readonly` only if
  all are. The `string` signature applies only when no other does.
  Example: `{ [k: a*]: number; [k: ab*]: 1 | 2 }["ab1"]` is `1 | 2`.
- **Write**: the value must fit the value of the applicable signature. A
  `readonly` applicable signature rejects the write.
- **Declaration consistency** (TS2413): when signature `S`'s key is assignable to
  signature `T`'s key (`number` to `string`, `id-*` to `string`, `ab*` to `a*`),
  `S`'s value must be assignable to `T`'s value. This includes inherited
  signatures. Named fields keep today's rule against every signature whose key
  matches their name.
- **Assignability** (Submilli's existing rule, extended): a source fits a
  target when, for every target signature, the source's fields whose names match
  it and the source signatures whose domains relate to it all fit its value.
- **Runtime test** (cast, interface test): each property is checked against
  every signature whose key matches its name, `string` included.
- **`Object.assign`**: the result has every source's signatures. Two signatures
  with the same key merge into one whose value is the intersection.
- **Computed literal** (`{ [k]: v }` with a non-literal key): as tsc, a `string`
  signature over all values (template keys count as `string`), plus a `number`
  signature over the values under numeric keys.
- **Inheritance**: an interface has its own signatures and its bases'. An own
  signature with the same key as an inherited one replaces it, and its value
  must fit the inherited value.
- **`keyof`**: the union of all signature keys and field names.
- **Utility types**: `Partial` and `Readonly` map every signature. `Pick` and
  `Omit` keep the signatures whose keys the picked keys cover.
- **`Object.values`/`entries`**: the union of every signature's value and the
  field types.
- **JSON schema**: a `string` signature is `additionalProperties`, each pattern is
  a `patternProperties` entry. TS2413 consistency makes that equivalent. Number
  domains stay rejected.

## Steps

1. Representation: the new field, `Type` helpers (`index_signatures()`),
   `TypeResolver::index_signatures` and `applicable_index`, walkers
   (`map_types`, `type_size`, substitution, variance, `Display`, `.d.ts`
   emission, definition printing), artifact version bump.
2. Declarations: the parser keeps every signature of an object type literal, the
   interface binder keeps every own signature, consistency checks.
3. Reads and writes through the applicable signature.
4. Assignability and comparability over every signature.
5. Composition: `Object.assign`, spread (tsc rule), computed literals,
   inheritance, utility types.
6. Codegen: a conformance test per signature, `has_string_index` users, JSON
   schema.
7. Spec (Records), fixtures, and the TypeScript conformance suite.

## Tests

- The issue's program prints `1 x`.
- Disjoint `a*: number` / `b*: string`, through assign, declaration, cast, read,
  write and getters.
- `string` + `number` signatures, compatible and incompatible (TS2413).
- Overlapping templates (`a*` and `ab*`): the read is the intersection.
- Spread alternatives under the tsc rule.
- Computed property domains.
- Interface inheritance that adds a domain.

## As built

- Every reader goes through `TypeResolver::index_signatures` (all signatures,
  an interface's substituted), `has_index_signature`, or the applicable-signature
  helpers `index_for_name`, `index_for_number_name` and `applicable_index`
  (`typechecker/infer/records.rs`). Relations (assignability, comparability) use
  `source_indexes_fit`, which doesn't intersect values.
- Signatures whose keys overlap without one lying within the other (`a${string}`
  beside `${string}z`) are rejected by this front end, in declarations
  (`report_overlapping_signatures`, beside the TS2413 check), through
  inheritance and by `Object.assign` (Doron, 2026-11-13), unless their
  intersection is one of the values exactly (equal values, or one without
  object types that fits the other; `values_intersect_exactly`). Overlap is
  decided from the keys' literal text and number holes (`may_share_name`),
  conservatively: a pair it can't rule out counts as overlapping. `Object.assign` also refuses nested signatures whose values
  don't nest (TS2413). The typed tree still holds
  them for ts-rust, which has intersection types; codegen checks each
  signature a name matches either way. So `intersect_values` only meets nested
  values, where one fits all the others; anything else (from ts-rust) stays a
  plain intersection. `TypeLimits::enter_intersection` bounds re-entry through
  a bound's `T[K]` (`MAX_OPEN_INTERSECTIONS`).
- `split_index_signatures` is the one way to store a list of signatures, so a
  non-empty `more_indexes` always comes with an `index`; `index.is_some()` still
  answers "has a signature".
- Spread signatures are writable, as in tsc. Object rest keeps its source's
  signatures.
- Inherited signatures: the first base's wins for a key the bases share (the
  rule before this change); a disagreement is reported as before.
- Artifact schema version 19.

### Deliberate deviations from tsc

- A spread still widens an earlier named field by a later source's matching
  signature value (`objectSpreadIndexSignature` line 9, triaged by design): the
  source may overwrite the field at run time.
- A computed literal's run of non-literal keys gives one signature over the
  union of their key types. tsc gives a `string` signature over every value and a
  `number` one over the numeric keys. Ours is sound and accepts the same reads,
  with a wider value for a number key.
- A write with a non-literal key must also fit every narrower signature its key
  can name (`o[k] = "s"` with `k: string` beside `[k: `a${string}`]: number`),
  as it already had to fit every declared field it can name. tsc checks only the
  applicable signature and accepts the write, which would break later reads.
- Signatures whose keys overlap without nesting are rejected (see As built).
- A source relates to a target signature when one of the source signatures
  that apply to its key fits on its own (`source_indexes_fit`); tsc relates
  their intersection. Stricter, and needs no intersection inside a relation.

### Conformance

`keyofAndIndexedAccess2`: the `{ [key: string]: E; [key: number]: E }` rows
(89, 90, 94, 100) and the duplicate-signature error (84) now match tsc.
`objectSpreadIndexSignature` line 6 now matches tsc.
