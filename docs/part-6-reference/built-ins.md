---
title: "Built-ins"
description: "The globals every Submilli program has without an import: types, error classes, namespaces, and global functions, with each one's members."
slug: reference/built-ins
sidebar:
  order: 5
authorship:
  label: ai-assisted
  confirmed: true
  contentHash: "1ffbf950967f6b738f16118daa96eb4e605f94e23f050545a7241b3f2c900b87"
  confirmedAt: "2026-10-05T17:33:06.426Z"
---

Built-ins are the globals in scope in every program without an `import`. They
are a subset of the ECMA-262 globals, `TextEncoder` and `TextDecoder`, and
`PermissionDeniedError` and `QuotaExceededError`. This page lists them, what is
absent and what to use instead, and each built-in's members. The language
itself is on [Language](/docs/reference/language). The importable `submilli:`
modules are on [Standard library](/docs/reference/standard-library).

## Catalog

`submilli builtins` prints the catalog:

```text
Types: Array, BigInt, Boolean, Error, Map, Number, Object, PermissionDeniedError, QuotaExceededError, RangeError, Record, RegExp, Set, String, SyntaxError, TextDecoder, TextEncoder, TypeError, Uint8Array
Namespaces: JSON, Math, Temporal
```

| Built-in | Kind | Describes |
| --- | --- | --- |
| `Array<T>` | Type | A homogeneous array |
| `Map<K, V>` | Type | A key-value collection whose keys are compared by structural equality |
| `Set<T>` | Type | A unique-value collection whose elements are compared by structural equality |
| `String` | Type | UTF-16 strings |
| `Number` | Type | IEEE 754 doubles |
| `BigInt` | Type | Arbitrary-precision integers. Literals use the `n` suffix |
| `Boolean` | Type | `true` and `false` |
| `Object` | Type | The base of every object type, with `Object.keys`, `values`, `entries`, `hasOwn`, `is` |
| `Record<K, V>` | Type | String-keyed objects ([Language](/docs/reference/language#recordstring-v-for-runtime-keys)) |
| `RegExp` | Type | Regular expressions, without lookaround or backreferences |
| `Uint8Array` | Type | Byte arrays, with hex and base64 conversion |
| `TextEncoder`, `TextDecoder` | Type | UTF-8 encoding and decoding |
| `Error` | Error class | The base error class. `throw` takes `Error` and its subclasses |
| `TypeError` | Error class | Failed runtime type checks: a failed `as` cast, `x!` on `null`, invalid UTF-8, an invalid URL |
| `RangeError` | Error class | Out-of-range values: an array index, bigint division by zero, invalid Temporal values, and argument-size caps |
| `SyntaxError` | Error class | Text that fails to parse: `JSON.parse`, `BigInt()`, `Uint8Array.fromHex`, `new RegExp()` |
| `PermissionDeniedError` | Error class | A denied capability, with fields `caller`, `capability`, `reason` ([Permissions](/docs/reference/permissions)) |
| `QuotaExceededError` | Error class | A budget refusal: filesystem space, model tokens, or session state ([Errors and limits](/docs/reference/errors-and-limits)) |
| `JSON` | Namespace | `JSON.parse`, returning `unknown`, and `JSON.stringify` |
| `Math` | Namespace | Constants and functions, including `Math.random()` |
| `Temporal` | Namespace | Dates, times, time zones, and durations |

Every error class extends `Error` and can be named in a typed `catch`
(`catch (e: PermissionDeniedError)`).

### Temporal types

| Type | Represents |
| --- | --- |
| `Temporal.Instant` | An exact moment, with no time zone |
| `Temporal.ZonedDateTime` | A moment in an IANA time zone |
| `Temporal.PlainDate` | A calendar date, with no time or time zone |
| `Temporal.PlainTime` | A wall-clock time, with no date or time zone |
| `Temporal.PlainDateTime` | A calendar date and wall-clock time, with no time zone |
| `Temporal.PlainYearMonth` | A year and month, such as `2026-06` |
| `Temporal.PlainMonthDay` | A month and day, such as `06-07` |
| `Temporal.Duration` | A span of time |
| `Temporal.Now` | The clock: `instant()`, `plainDateISO()`, `plainTimeISO()`, `plainDateTimeISO()`, `zonedDateTimeISO()`, `timeZoneId()` |

Temporal values are immutable. Arithmetic returns a new value. The calendar is
ISO 8601. `Temporal.Now` functions take an optional IANA time-zone id and
default to the system zone. A Temporal value in a template literal is written
with an explicit `.toString()`.

```typescript title="ticket-age.ts"
function main(): string {
  const opened = Temporal.Instant.from("2026-09-15T08:30:00Z");
  const closed = Temporal.Instant.from("2026-09-18T10:00:00Z");
  const hoursOpen = closed.since(opened).total("hours");

  const local = closed.toZonedDateTimeISO("Asia/Jerusalem");
  const followUp = local.toPlainDate().add({ days: 3 });

  return `open ${hoursOpen} hours, closed ${local.toPlainTime().toString()} local, follow up ${followUp.toString()}`;
}
```

```text
open 73.5 hours, closed 13:00:00 local, follow up 2026-09-21
```

## Global functions and values

These globals are in scope and are not in the `submilli builtins` catalog.

| Global | Behavior |
| --- | --- |
| `console.log(first, ...rest)` | Writes the values, separated by spaces, to the log stream |
| `assert(condition, message?)` | Throws `Error(message)` when `condition` is `false` |
| `parseInt(string, radix = 10)` | Parses an integer prefix, or returns `NaN` when there are no digits |
| `parseFloat(string)` | Parses a decimal number, or returns `NaN` when the text is not one |
| `isNaN(value)`, `isFinite(value)` | Number tests |
| `encodeURIComponent(uri)`, `encodeURI(uri)` | Percent-encoding |
| `decodeURIComponent(uri)`, `decodeURI(uri)` | Percent-decoding. Throws `Error` (`"URI malformed"`) on an invalid escape |
| `NaN`, `Infinity` | Number constants |

## Not available

| Global | Use instead |
| --- | --- |
| `Date` | `Temporal`, with `Temporal.Now.instant()` for the current time |
| `Symbol` | None |
| `Proxy`, `Reflect` | None |
| Prototype reflection: `Object.getPrototypeOf`, `Object.defineProperty`, `obj.hasOwnProperty`, `.prototype` | `Object.hasOwn(obj, key)` or `key in obj` for presence |
| `Object.assign`, `Object.freeze` | Object spread `{ ...a, ...b }`, and `readonly` types |
| `WeakMap`, `WeakSet` | `Map`, `Set` |
| `Promise`, `queueMicrotask`, `setTimeout`, `setInterval` | None. Calls are synchronous |
| Typed arrays other than `Uint8Array`, `ArrayBuffer`, `DataView` | `Uint8Array` |
| `Intl` | None. `localeCompare` compares UTF-16 code units |
| `globalThis`, `window` | None |
| `fetch` | `submilli:http` |
| `URL`, `URLSearchParams` | `submilli:url` |
| `crypto` | `submilli:crypto`, `submilli:uuid` |
| `atob`, `btoa`, `Buffer` | `Uint8Array.fromBase64`, `toBase64`, `TextEncoder`, `TextDecoder` |
| `process`, `require`, and other Node.js globals | The [standard library](/docs/reference/standard-library) and Submilli Packages |

An absent global is a compile error. `Date` has its own message:

```typescript title="now.ts"
function main(): string {
  const now = new Date();
  return now.toISOString();
}
```

```text
error: `Date` is not supported
 --> now.ts:2:19
  |
1 | function main(): string {
2 |   const now = new Date();
  |                   ^^^^
3 |   return now.toISOString();
  |
help: use `Temporal.Now.instant()` for wall-clock time, or `Temporal.ZonedDateTime` / `Temporal.Instant` for time values. `Date` is intentionally out of scope — see Temporal for a correct, immutable, timezone-aware time API.
```

Other absent globals are unresolved identifiers, such as
``error: unresolved identifier `Symbol` ``, and absent members are missing
methods, such as ``error: no method `getPrototypeOf` on `ObjectConstructor` ``
followed by the type's declaration.

## Looking a built-in up

The live declarations, with doc comments, come from the CLI, which reads
the same source as the compiler. `submilli builtins` lists the catalog.
`submilli builtins <name>…` prints declarations, such as
`submilli builtins Map Temporal`.

A dotted path prints one member, such as `submilli builtins Temporal.Instant`
or `submilli builtins Map.get`. A name that is not a built-in fails with a
suggestion:

```text
$ submilli builtins Date
unknown built-in: Date. Did you mean `Temporal`?

Run `submilli builtins` to list available built-ins.
```

A module name is redirected to `submilli docs`:

```text
$ submilli builtins submilli:fs
`submilli:fs` is a package, not a language built-in. Run `submilli docs submilli:fs` for its declarations, and write `import ... from "submilli:fs"` to use it.

Run `submilli builtins` to list available built-ins.
```

`submilli docs Temporal` prints a built-in's declarations too, after a line
saying it is always in scope and takes no `import`.

<!-- generated:builtins -->

## `Array`

A homogeneous array of `T`.

| Member | Description |
| --- | --- |
| `readonly length: number` | The number of elements in the array. |
| `at(index: number): T \| null` | Returns the element at `index`, or `null` if out of range. |
| `concat(...others: (readonly T[])[]): T[]` | Returns a new array containing this array's elements followed by every element of each `others` array, in order. |
| `copyWithin(target: number, start?: number, end?: number): T[]` | Copies the slots `[start, end)` to `target`, in place (overlap-safe), and returns the array. |
| `entries(): Iterator<[number, T]>` | Returns a live `Iterator<[number, T]>` of index/element pairs. |
| `every(predicate: (arg0: T, arg1: number, arg2: T[]) => boolean): boolean` | Returns `true` if `predicate` returned `true` for every element. |
| `fill(value: T, start?: number, end?: number): T[]` | Sets every slot in `[start, end)` to `value`, in place, and returns the array. |
| `filter(predicate: (arg0: T, arg1: number, arg2: T[]) => boolean): T[]` | Returns a new array of the elements for which `predicate` returned `true`. |
| `find(callback: (arg0: T, arg1: number, arg2: T[]) => boolean): T \| null` | Returns the first element for which `callback` returns `true`, or `null` if none match. |
| `findIndex(callback: (arg0: T, arg1: number, arg2: T[]) => boolean): number` | Returns the index of the first element for which `callback` returns `true`, or `-1` if none match. |
| `findLast(callback: (arg0: T, arg1: number, arg2: T[]) => boolean): T \| null` | Returns the last element for which `callback` returns `true`, or `null` if none match. |
| `findLastIndex(callback: (arg0: T, arg1: number, arg2: T[]) => boolean): number` | Returns the index of the last element for which `callback` returns `true`, or `-1` if none match. |
| `flat(depth?: number): T[]` | Flattens nested arrays up to `depth` levels into a new array. |
| `flatMap<U>(callback: (arg0: T, arg1: number, arg2: T[]) => U[]): U[]` | Maps each element to an array via `callback`, then flattens the results one level. |
| `forEach(callback: (arg0: T, arg1: number, arg2: T[]) => void): void` | Calls `callback(value, index, array)` once for each element in order. |
| `includes(elem: T, fromIndex?: number): boolean` | Returns `true` if some element at or after `fromIndex` equals `elem`. |
| `indexOf(elem: T, fromIndex?: number): number` | Returns the index of the first element equal to `elem` at or after `fromIndex`, or `-1` if not present. |
| `join(separator?: string): string` | Joins the elements using `separator`. |
| `keys(): Iterator<number>` | Returns a live `Iterator<number>` of the indices — length is re-read on every step. |
| `lastIndexOf(elem: T, fromIndex?: number): number` | Returns the index of the last element equal to `elem`, or `-1`. |
| `map<U>(callback: (arg0: T, arg1: number, arg2: T[]) => U): U[]` | Returns a new array produced by applying `callback` to each element. |
| `pop(): T \| null` | Removes the last element and returns it, or `null` if the array is empty. |
| `push(elem: T): number` | Appends `elem` to the end of the array. |
| `reduce<U>(callback: (arg0: U, arg1: T, arg2: number, arg3: T[]) => U, initial: U): U` | Reduces the array to a single value. |
| `reduceRight<U>(callback: (arg0: U, arg1: T, arg2: number, arg3: T[]) => U, initial: U): U` | Reduces the array right-to-left. |
| `reverse(): T[]` | Reverses the array in place and returns it. |
| `shift(): T \| null` | Removes and returns the first element, shifting the rest forward. |
| `slice(start?: number, end?: number): T[]` | Returns a new array of the elements from `start` (inclusive) to `end` (exclusive). |
| `some(predicate: (arg0: T, arg1: number, arg2: T[]) => boolean): boolean` | Returns `true` if `predicate` returned `true` for at least one element. |
| `sort(compareFn?: ((arg0: T, arg1: T) => number) \| null): T[]` | Sorts the array in place and returns it. |
| `splice(start: number, deleteCount?: number, ...items: T[]): T[]` | Removes `deleteCount` elements at `start`, inserts `items` there (in place), and returns the removed elements. |
| `toJson(): string` | Returns the JSON representation of this array — `"["` + elements' `toJson()` joined with `","` + `"]"`. |
| `toReversed(): T[]` | Returns a new array with the elements in reverse order; the receiver is not modified. |
| `toSorted(compareFn?: ((arg0: T, arg1: T) => number) \| null): T[]` | Returns a new sorted array; the receiver is not modified. |
| `toSpliced(start: number, deleteCount?: number, ...items: T[]): T[]` | Returns a new array with `deleteCount` elements removed at `start` and `items` inserted there; the receiver is not modified. |
| `toString(): string` | Returns the elements joined with commas. |
| `unshift(...items: T[]): number` | Prepends `items` (keeping their argument order) and returns the new length. |
| `values(): Iterator<T>` | Returns a live `Iterator<T>` over the elements — unlike Map/Set cursors it sees pushes made during iteration. |
| `with(index: number, value: T): T[]` | Returns a copy of the array with the slot at `index` replaced by `value`; the receiver is not modified. |

| Constant | Description |
| --- | --- |
| `Array: ArrayConstructor` |  |

### `ArrayConstructor`

Constructor object for `Array`.

| Member | Description |
| --- | --- |
| `from<T, U>(src: readonly T[] \| Iterable<T> \| Iterator<T>, mapFn?: null \| ((arg0: T, arg1: number) => U)): U[]` | Materializes any iterable — an array, string (code points), `Iterator<T>`, or `Iterable<T>` — into a fresh array. |
| `isArray<T>(value: T): boolean` | Returns `true` when `value` is an array. |
| `of<T>(...items: T[]): T[]` | Builds an array from its arguments — `Array.of(1, 2, 3)` is `[1, 2, 3]`. |

## `BigInt`

Arbitrary-precision integer.

| Member | Description |
| --- | --- |
| `toJson(): string` | Returns the canonical decimal representation — JSON has no native bigint literal, so this matches `toString`. |
| `toString(radix?: number): string` | Returns this bigint formatted in the given base. |

| Constant | Description |
| --- | --- |
| `BigInt: BigIntConstructor` |  |

### `BigIntConstructor`

Constructor object for `bigint`.

## `Boolean`

The boolean type — `true` or `false`.

| Member | Description |
| --- | --- |
| `toJson(): string` | Returns `"true"` or `"false"` — same as `toString` (booleans are JSON-native). |
| `toString(): string` | Returns `"true"` or `"false"`. |

## `Error`

The built-in error class.

| Member | Description |
| --- | --- |
| `static isError(value: unknown): boolean` | Returns `true` when `value` is an `Error` instance (including subclasses). |
| `message: string` | The human-readable message passed to `new Error(message)`. |
| `name: string` | The error class name. |
| `constructor(message?: string)` |  |

## `Map`

A hash-backed key-value collection.

| Member | Description |
| --- | --- |
| `readonly size: number` | The number of entries currently in the map. |
| `clear(): void` | Removes every entry. |
| `delete(key: K): boolean` | Removes `key` from the map. |
| `entries(): Iterator<[K, V]>` | Returns a lazy `Iterator<[K, V]>` over the entries — the same cursor `for-of` uses. |
| `forEach(callback: (arg0: V, arg1: K, arg2: Map<K, V>) => void): void` | Calls `callback(value, key, map)` once for each entry in insertion order. |
| `get(key: K): V \| null` | Returns the value associated with `key`, or `null` if the key is not present. |
| `has(key: K): boolean` | Returns `true` when `key` is present. |
| `iterator(): Iterator<[K, V]>` | Returns a fresh `Iterator<[K, V]>` over the entries. |
| `keys(): Iterator<K>` | Returns a lazy `Iterator<K>` over the keys in insertion order (snapshots the buckets at creation). |
| `set(key: K, value: V): Map<K, V>` | Associates `value` with `key`, overwriting any prior value. |
| `values(): Iterator<V>` | Returns a lazy `Iterator<V>` over the values in insertion order (snapshots the buckets at creation). |

| Constant | Description |
| --- | --- |
| `Map: MapConstructor` |  |

### `MapConstructor`

Constructor object for `Map`.

| Member | Description |
| --- | --- |
| `new<K, V>(entries?: null \| readonly [K, V][] \| Iterable<[K, V]> \| Iterator<[K, V]>): Map<K, V>` | Construct a `Map<K, V>`, optionally from an iterable of `[K, V]` entries: `new Map([["a", 1]])`. |

## `Number`

The IEEE-754 double-precision number type.

| Member | Description |
| --- | --- |
| `toExponential(fractionDigits?: number): string` | Exponential notation — `(1234.5).toExponential(2)` is `"1.23e+3"`. |
| `toFixed(digits?: number): string` | Fixed-point notation with exactly `digits` fraction digits — `(3.14159).toFixed(2)` is `"3.14"`. |
| `toJson(): string` | Returns the JSON representation of this number. |
| `toPrecision(precision?: number): string` | Formats to `precision` significant digits, switching to exponential form for very large or small values. |
| `toString(radix?: number): string` | Returns this number formatted in the given base. |

| Constant | Description |
| --- | --- |
| `Number: NumberConstructor` |  |

### `NumberConstructor`

Constructor object for `number`.

| Member | Description |
| --- | --- |
| `readonly EPSILON: number` | The gap between 1 and the next representable double (2^-52). |
| `readonly MAX_SAFE_INTEGER: number` | 2^53 − 1 — the largest exactly-representable integer. |
| `readonly MAX_VALUE: number` | The largest finite double (≈1.7976931348623157e308). |
| `readonly MIN_SAFE_INTEGER: number` | −(2^53 − 1). |
| `readonly MIN_VALUE: number` | The smallest positive double (5e-324, subnormal). |
| `readonly NEGATIVE_INFINITY: number` | Same value as `-Infinity`. |
| `readonly NaN: number` | Same value as the global `NaN`. |
| `readonly POSITIVE_INFINITY: number` | Same value as the global `Infinity`. |

## `Object`

Universal base type for every object shape.

| Member | Description |
| --- | --- |
| `toJson(): string` | Returns the JSON representation of this object — `"{"` + `"key":value-json` pairs joined with `","` + `"}"`. |
| `toString(): string` | Returns `"[object Object]"`. |

| Constant | Description |
| --- | --- |
| `Object: ObjectConstructor` |  |

### `ObjectConstructor`

Constructor object for `Object` — the enumeration statics (`keys`/`values`/`entries`/`hasOwn`) and `is`.

| Member | Description |
| --- | --- |
| `entries(obj: unknown): [string, unknown][]` | Returns `[name, value]` pairs in canonical sorted key order. |
| `hasOwn(obj: unknown, key: string): boolean` | Returns `true` when `obj` declares a field named `key`. |
| `is(a: unknown, b: unknown): boolean` | SameValue comparison: like `===` but `Object.is(NaN, NaN)` is `true` and `Object.is(0, -0)` is `false`. |
| `keys(obj: unknown): string[]` | Returns the object's field names in canonical sorted order (the same order JSON output uses). |
| `values(obj: unknown): unknown[]` | Returns the object's field values, aligned with `Object.keys` order. |

## `PermissionDeniedError`

The built-in permission-denial class (`extends Error`, `name` = `"PermissionDeniedError"`).

| Member | Description |
| --- | --- |
| `caller: string` | The Package the denied call was attributed to (e.g. `"main"`). |
| `capability: string` | The denied capability name (e.g. `"fs.read"`, `"http.get"`). |
| `reason: string` | The policy-supplied denial reason. |
| `constructor(message: string, caller: string, capability: string, reason: string)` |  |

## `QuotaExceededError`

A budget refusal (`extends Error`): filesystem space, model tokens, or session state.

| Member | Description |
| --- | --- |
| `constructor(message?: string)` |  |

## `RangeError`

The built-in range-error class (`extends Error`, `name` = `"RangeError"`).

| Member | Description |
| --- | --- |
| `constructor(message?: string)` |  |

## `Record`

### `Record`

Record<K, V> accepts string keys.

```typescript
type Record<K extends string, V> = { [P in K]: V };
```

## `RegExp`

Compiled regular expression.

| Member | Description |
| --- | --- |
| `readonly dotAll: boolean` | `true` if the regex was constructed with the `s` flag (`.` matches newlines). |
| `readonly flags: string` | The flag string in source order — any subset of `gimsuy`. |
| `readonly global: boolean` | `true` if the regex was constructed with the `g` flag. |
| `readonly ignoreCase: boolean` | `true` if the regex was constructed with the `i` flag (case-insensitive matching). |
| `readonly lastIndex: number` | Read-only in v1 — the wrapper writes it back internally on `g`/`y` matches. |
| `readonly multiline: boolean` | `true` if the regex was constructed with the `m` flag (`^` / `$` match line boundaries). |
| `readonly source: string` | The original JS-source pattern (without the leading/trailing `/`). |
| `readonly sticky: boolean` | `true` if the regex was constructed with the `y` flag (sticky / anchored at `lastIndex`). |
| `readonly unicode: boolean` | `true` if the regex was constructed with the `u` flag (Unicode classes for `\d` / `\w` / `\s`). |
| `exec(s: string): RegExpMatch \| null` | Find the next match in `s`. |
| `test(s: string): boolean` | Returns `true` if the pattern matches anywhere in `s`. |

| Constant | Description |
| --- | --- |
| `RegExp: RegExpConstructor` |  |

### `RegExpConstructor`

Constructor object for `RegExp`.

| Member | Description |
| --- | --- |
| `new(source: string, flags: string): RegExp` | Construct a new `RegExp` from `source` and `flags`. |

## `Set`

A hash-backed unique-value collection.

| Member | Description |
| --- | --- |
| `readonly size: number` | The number of elements currently in the set. |
| `add(value: T): Set<T>` | Adds `value` to the set. |
| `clear(): void` | Removes every element. |
| `delete(value: T): boolean` | Removes `value` from the set. |
| `difference(other: Set<T>): Set<T>` | Returns a new set with this set's elements that are not in `other`. |
| `entries(): Iterator<[T, T]>` | Returns a lazy `Iterator<[T, T]>` of `[value, value]` pairs (the element repeats, mirroring `Map#entries`). |
| `forEach(callback: (arg0: T, arg1: T, arg2: Set<T>) => void): void` | Calls `callback(value, value, set)` once for each element in insertion order. |
| `has(value: T): boolean` | Returns `true` when `value` is present. |
| `intersection(other: Set<T>): Set<T>` | Returns a new set with the elements present in both this set and `other`. |
| `isDisjointFrom(other: Set<T>): boolean` | Returns `true` when this set and `other` share no elements. |
| `isSubsetOf(other: Set<T>): boolean` | Returns `true` when every element of this set is in `other`. |
| `isSupersetOf(other: Set<T>): boolean` | Returns `true` when every element of `other` is in this set. |
| `iterator(): Iterator<T>` | Returns a fresh `Iterator<T>` over the elements. |
| `keys(): Iterator<T>` | Alias of `values()` — sets have no separate keys. |
| `symmetricDifference(other: Set<T>): Set<T>` | Returns a new set with the elements in exactly one of this set and `other` (the overlap is dropped). |
| `union(other: Set<T>): Set<T>` | Returns a new set with every element of this set and `other`. |
| `values(): Iterator<T>` | Returns a fresh array of every element in insertion order. |

| Constant | Description |
| --- | --- |
| `Set: SetConstructor` |  |

### `SetConstructor`

Constructor object for `Set`.

| Member | Description |
| --- | --- |
| `new<T>(values?: null \| readonly T[] \| Iterable<T> \| Iterator<T>): Set<T>` | Construct a `Set<T>`, optionally from an iterable of values: `new Set([1, 2, 2])` dedups to size 2. |

## `String`

The UTF-16 string type.

| Member | Description |
| --- | --- |
| `readonly length: number` | The number of UTF-16 code units in the string. |
| `at(index: number): string \| null` | Returns the code unit at `index` as a single-character string. |
| `charAt(index: number): string` | Returns the UTF-16 code unit at `index` as a single-character string. |
| `charCodeAt(index: number): number` | Returns the UTF-16 code unit at `index` as an integer (0..65535), or `NaN` if out of range. |
| `codePointAt(index: number): number` | Returns the Unicode code point starting at `index`, decoding surrogate pairs into values up to 0x10FFFF. |
| `concat(other: string): string` | Returns a new string with `other` appended. |
| `endsWith(search: string, endPosition?: number): boolean` | Returns `true` if the substring ending at `endPosition` ends with `search`. |
| `equals(other: string): boolean` | Returns `true` when both strings have identical code units. |
| `includes(search: string, fromIndex?: number): boolean` | Returns `true` if `search` occurs at or after `fromIndex`. |
| `indexOf(search: string, fromIndex?: number): number` | Returns the index of the first occurrence of `search` at or after `fromIndex`, or `-1` if none. |
| `isWellFormed(): boolean` | Returns `true` when the string contains no lone surrogates (it round-trips losslessly through UTF-8). |
| `iterator(): Iterator<string>` | Returns an `Iterator<string>` of CODE POINTS — surrogate pairs arrive as one two-unit string. |
| `lastIndexOf(search: string, fromIndex?: number): number` | Returns the index of the last occurrence of `search` at or before `fromIndex`, or `-1` if none. |
| `localeCompare(other: string): number` | Compares this string with `other` in UTF-16 code-unit order. |
| `match(re: RegExp): RegExpMatch \| null` | Find the next match of `re` in this string. |
| `matchAll(re: RegExp): RegExpMatch[]` | Find every non-overlapping match of `re` in this string. |
| `normalize(form?: string): string` | Returns a new string in the specified Unicode normalization form. |
| `padEnd(targetLength: number, padString?: string): string` | Pads this string with `padString` on the right until the result reaches `targetLength` code units. |
| `padStart(targetLength: number, padString?: string): string` | Pads this string with `padString` on the left until the result reaches `targetLength` code units. |
| `repeat(count: number): string` | Returns a new string containing `count` copies of this string concatenated. |
| `replace(search: string \| RegExp, replacement: string): string` | Replace the first match of `search` with `replacement`. |
| `replaceAll(search: string \| RegExp, replacement: string): string` | Replace every non-overlapping match of `search` with `replacement`. |
| `search(re: RegExp): number` | Index of the first match of `re` in this string, or `-1` if no match. |
| `slice(start?: number, end?: number): string` | Returns a new string containing the code units from `start` (inclusive) to `end` (exclusive). |
| `split(separator: string \| RegExp, limit?: number): string[]` | Split this string into pieces by matches of `separator`. |
| `startsWith(search: string, position?: number): boolean` | Returns `true` if the substring starting at `position` begins with `search`. |
| `substring(start?: number, end?: number): string` | Like `slice`, but negative values clamp to `0`, and `start` and `end` are swapped when `start > end`. |
| `toJson(): string` | Returns this string wrapped in `"…"` with JSON escapes applied (`\"`, `\\`, `\b`, `\f`, `\n`, `\r`, `\t`, and `\u00XX` for control codepoints). |
| `toLowerCase(): string` | Returns a new string with every character converted to its Unicode lowercase form. |
| `toString(): string` | Returns this string unchanged (identity). |
| `toUpperCase(): string` | Returns a new string with every character converted to its Unicode uppercase form. |
| `toWellFormed(): string` | Returns a copy with every lone surrogate replaced by U+FFFD (`�`); well-formed strings come back unchanged. |
| `trim(): string` | Returns a new string with Unicode whitespace removed from both ends. |
| `trimEnd(): string` | Returns a new string with Unicode whitespace removed from the end. |
| `trimStart(): string` | Returns a new string with Unicode whitespace removed from the start. |

| Constant | Description |
| --- | --- |
| `String: StringConstructor` |  |

### `StringConstructor`

Constructor object for `string`.

## `SyntaxError`

The built-in syntax-error class (`extends Error`, `name` = `"SyntaxError"`).

| Member | Description |
| --- | --- |
| `constructor(message?: string)` |  |

## `TextDecoder`

Decodes UTF-8 byte arrays into UTF-16 strings.

| Member | Description |
| --- | --- |
| `decode(bytes: Uint8Array): string` | Decode `bytes` as UTF-8 into a UTF-16 string. |

| Constant | Description |
| --- | --- |
| `TextDecoder: TextDecoderConstructor` |  |

### `TextDecoderConstructor`

Constructor object for `TextDecoder`.

| Member | Description |
| --- | --- |
| `new(): TextDecoder` | Construct a new `TextDecoder` instance. |

## `TextEncoder`

Encodes UTF-16 strings to UTF-8 byte arrays.

| Member | Description |
| --- | --- |
| `encode(s: string): Uint8Array` | UTF-8 encode `s` into a `Uint8Array`. |

| Constant | Description |
| --- | --- |
| `TextEncoder: TextEncoderConstructor` |  |

### `TextEncoderConstructor`

Constructor object for `TextEncoder`.

| Member | Description |
| --- | --- |
| `new(): TextEncoder` | Construct a new `TextEncoder` instance. |

## `TypeError`

The built-in type-error class (`extends Error`, `name` = `"TypeError"`).

| Member | Description |
| --- | --- |
| `constructor(message?: string)` |  |

## `Uint8Array`

Packed byte array.

| Member | Description |
| --- | --- |
| `readonly byteLength: number` | Equivalent to `length`. |
| `readonly length: number` | The number of bytes in this array. |
| `at(index: number): number \| null` | Returns the byte at `index`, or `null` if out of range. |
| `copyWithin(target: number, start?: number, end?: number): Uint8Array` | Copies `bytes[start..end)` to `bytes[target..]` in place. |
| `equals(other: Uint8Array): boolean` | Byte-by-byte equality with `other`. |
| `every(predicate: (arg0: number, arg1: number, arg2: Uint8Array) => boolean): boolean` | Returns `true` iff `predicate(byte, index, array)` returns `true` for every byte. |
| `fill(value: number, start?: number, end?: number): Uint8Array` | Writes `value & 0xff` to every byte in `[start, end)`. |
| `filter(predicate: (arg0: number, arg1: number, arg2: Uint8Array) => boolean): Uint8Array` | Returns a new `Uint8Array` containing every byte for which `predicate(byte, index, array)` returns `true`. |
| `find(predicate: (arg0: number, arg1: number, arg2: Uint8Array) => boolean): number \| null` | Returns the first byte for which `predicate(byte, index, array)` returns `true`, or `null`. |
| `findIndex(predicate: (arg0: number, arg1: number, arg2: Uint8Array) => boolean): number` | Returns the index of the first matching byte, or `-1`. |
| `findLast(predicate: (arg0: number, arg1: number, arg2: Uint8Array) => boolean): number \| null` | Returns the last byte for which `predicate(byte, index, array)` returns `true`, or `null`. |
| `findLastIndex(predicate: (arg0: number, arg1: number, arg2: Uint8Array) => boolean): number` | Returns the index of the last matching byte, or `-1`. |
| `forEach(callback: (arg0: number, arg1: number, arg2: Uint8Array) => void): void` | Invokes `callback(byte, index, array)` for every byte in order. |
| `includes(target: number, fromIndex?: number): boolean` | Returns `true` if `target` appears at or after `fromIndex`. |
| `indexOf(target: number, fromIndex?: number): number` | Returns the first index of `target`, or `-1`. |
| `join(separator?: string): string` | Returns this array's bytes formatted in decimal and concatenated with `separator` between adjacent pairs. |
| `lastIndexOf(target: number, fromIndex?: number): number` | Returns the last index of `target`, or `-1`. |
| `map(callback: (arg0: number, arg1: number, arg2: Uint8Array) => number): Uint8Array` | Returns a new `Uint8Array` whose i-th byte is `callback(this[i], i, this) & 0xff`. |
| `reduce<U>(callback: (arg0: U, arg1: number, arg2: number, arg3: Uint8Array) => U, initial: U): U` | Folds bytes left-to-right with `callback(acc, byte, index, array)`. |
| `reduceRight<U>(callback: (arg0: U, arg1: number, arg2: number, arg3: Uint8Array) => U, initial: U): U` | Folds bytes right-to-left with `callback(acc, byte, index, array)`. |
| `reverse(): Uint8Array` | Reverses this array's bytes in place and returns `this`. |
| `set(values: Uint8Array, offset?: number): void` | Copies `values` into this array starting at `offset`. |
| `slice(start?: number, end?: number): Uint8Array` | Returns a fresh copy of the bytes in `[start, end)`. |
| `some(predicate: (arg0: number, arg1: number, arg2: Uint8Array) => boolean): boolean` | Returns `true` if `predicate` returns `true` for any byte. |
| `sort(compareFn?: ((arg0: number, arg1: number) => number) \| null): Uint8Array` | Sorts bytes in place via `compareFn(a, b)`. |
| `subarray(start?: number, end?: number): Uint8Array` | Deep-copy alias for `slice` under v1 (no `ArrayBuffer` view sharing). |
| `toBase64(options?: Base64Options \| null): string` | Returns the bytes as a base64 string. |
| `toHex(): string` | Returns the bytes as a lowercase hex string. |
| `toJson(): string` | Returns this array's bytes as a standard (padded) base64 string wrapped in `"…"` (the JSON string form). |
| `toReversed(): Uint8Array` | Returns a fresh copy with bytes reversed. |
| `toSorted(compareFn?: ((arg0: number, arg1: number) => number) \| null): Uint8Array` | Returns a fresh copy sorted via `compareFn`. |
| `toString(): string` | Returns this array's bytes joined as a comma-separated decimal string — e.g. `Uint8Array.new([1, 2, 3]).toString() === "1,2,3"`. |
| `with(index: number, value: number): Uint8Array` | Returns a clone of this array with `value & 0xff` written at `index`. |

| Constant | Description |
| --- | --- |
| `Uint8Array: Uint8ArrayConstructor` |  |

### `Uint8ArrayConstructor`

Constructor object for `Uint8Array`.

| Member | Description |
| --- | --- |
| `alloc(n: number): Uint8Array` | Allocate a zero-filled `Uint8Array` of length `n`. |
| `fromArray(values: number[]): Uint8Array` | Build a new `Uint8Array` from `values` — canonical name for `Uint8Array.new(values)`. |
| `fromBase64(s: string, options?: Base64Options \| null): Uint8Array` | Decode `s` as base64; `options.alphabet` picks standard vs URL-safe. |
| `fromBytes(other: Uint8Array): Uint8Array` | Returns a deep copy of `other`. |
| `fromHex(s: string): Uint8Array` | Decode `s` as a hex string. |
| `new(values: number[] \| number): Uint8Array` | Build a new `Uint8Array`. |
| `of(...values: number[]): Uint8Array` | Build a `Uint8Array` from the supplied byte values — `Uint8Array.of(1, 2, 3)`. |

## `JSON`

| Function | Capability | Description |
| --- | --- | --- |
| `parse(text: string): unknown` |  | Parse a JSON string as unknown; use `JSON.parse(s) as T` to validate a target type. |
| `stringify<T>(value: T, replacer?: null, space?: number \| string \| null): string` |  | Serialize a value to a JSON string. |

## `Math`

| Function | Capability | Description |
| --- | --- | --- |
| `abs(x: number): number` |  |  |
| `acos(x: number): number` |  |  |
| `acosh(x: number): number` |  |  |
| `asin(x: number): number` |  |  |
| `asinh(x: number): number` |  |  |
| `atan(x: number): number` |  |  |
| `atan2(y: number, x: number): number` |  |  |
| `atanh(x: number): number` |  |  |
| `cbrt(x: number): number` |  |  |
| `ceil(x: number): number` |  |  |
| `clz32(x: number): number` |  |  |
| `cos(x: number): number` |  |  |
| `cosh(x: number): number` |  |  |
| `exp(x: number): number` |  |  |
| `expm1(x: number): number` |  |  |
| `floor(x: number): number` |  |  |
| `fround(x: number): number` |  |  |
| `hypot(...values: number[]): number` |  |  |
| `imul(a: number, b: number): number` |  |  |
| `log(x: number): number` |  |  |
| `log10(x: number): number` |  |  |
| `log1p(x: number): number` |  |  |
| `log2(x: number): number` |  |  |
| `max(...values: number[]): number` |  |  |
| `min(...values: number[]): number` |  |  |
| `pow(base: number, exponent: number): number` |  |  |
| `random(): number` |  | Pseudo-random number in `[0, 1)`. |
| `round(x: number): number` |  |  |
| `sign(x: number): number` |  |  |
| `sin(x: number): number` |  |  |
| `sinh(x: number): number` |  |  |
| `sqrt(x: number): number` |  |  |
| `tan(x: number): number` |  |  |
| `tanh(x: number): number` |  |  |
| `trunc(x: number): number` |  |  |

| Constant | Description |
| --- | --- |
| `E: number` |  |
| `LN10: number` |  |
| `LN2: number` |  |
| `LOG10E: number` |  |
| `LOG2E: number` |  |
| `PI: number` |  |
| `SQRT1_2: number` |  |
| `SQRT2: number` |  |

## `Temporal`

| Constant | Description |
| --- | --- |
| `Duration: Temporal.DurationConstructor` | The `Temporal.Duration` constructor. |
| `Instant: Temporal.InstantConstructor` | The `Temporal.Instant` constructor. |
| `PlainDate: Temporal.PlainDateConstructor` | A calendar date (year, month, day) with no time or time zone. |
| `PlainDateTime: Temporal.PlainDateTimeConstructor` | A calendar date and wall-clock time with no time zone. |
| `PlainMonthDay: Temporal.PlainMonthDayConstructor` | A calendar month and day with no year or time zone (e.g. `06-07`). |
| `PlainTime: Temporal.PlainTimeConstructor` | A wall-clock time (hour, minute, second, nanosecond) with no date or time zone. |
| `PlainYearMonth: Temporal.PlainYearMonthConstructor` | A calendar year and month with no day or time zone (e.g. `2026-06`). |
| `ZonedDateTime: Temporal.ZonedDateTimeConstructor` | The `Temporal.ZonedDateTime` constructor. |

### `Temporal.Duration`

A span of time, expressed as separate calendar (`years`/`months`/`weeks`/`days`) and time (`hours`/…/`nanoseconds`) unit slots.

| Member | Description |
| --- | --- |
| `readonly blank: boolean` | `true` when every slot is `0` (the Duration is empty / `sign === 0`). |
| `readonly days: number` | Calendar-days slot. |
| `readonly hours: number` | Hours slot. |
| `readonly microseconds: number` | Microseconds slot. |
| `readonly milliseconds: number` | Milliseconds slot. |
| `readonly minutes: number` | Minutes slot. |
| `readonly months: number` | Calendar-months slot. |
| `readonly nanoseconds: number` | Nanoseconds slot. |
| `readonly seconds: number` | Seconds slot. |
| `readonly sign: number` | `-1`, `0`, or `1`: the Duration's overall direction (every slot shares this sign). |
| `readonly weeks: number` | Calendar-weeks slot. |
| `readonly years: number` | Calendar-years slot. |
| `abs(): Temporal.Duration` | Returns a Duration with every slot's absolute value (a non-negative Duration). |
| `add(d: Temporal.Duration \| Temporal.DurationFields): Temporal.Duration` | Returns `this + d` as a new Duration. |
| `negated(): Temporal.Duration` | Returns a Duration with every field's sign flipped. |
| `round(roundTo: string \| Temporal.DurationRoundOptions): Temporal.Duration` | Rounds this Duration — `round("hour")` (smallestUnit shorthand) or `round({ smallestUnit, largestUnit, roundingMode, roundingIncrement, relativeTo })`. |
| `subtract(d: Temporal.Duration \| Temporal.DurationFields): Temporal.Duration` | Returns `this - d` as a new Duration. |
| `toJSON(): string` | Returns the ISO string form used by JSON.stringify. |
| `toString(options?: Temporal.DurationToStringOptions): string` | Returns the ISO 8601 duration form (e.g. `"PT1H30M"`), optionally rounded/formatted. |
| `total(totalOf: string \| Temporal.DurationTotalOptions): number` | Total length of this Duration in a unit — `total("hours")` or `total({ unit, relativeTo })`. |
| `with(fields: Temporal.Duration \| Temporal.DurationFields): Temporal.Duration` | Returns a copy of this Duration with the given slots replaced (e.g. `d.with({ hours: 5 })`); unlisted slots are kept. |

### `Temporal.DurationCompareOptions`

Options for `Temporal.Duration.compare(...)`.

| Member | Description |
| --- | --- |
| `readonly relativeTo?: Temporal.PlainDate \| Temporal.ZonedDateTime` | Anchor (PlainDate or ZonedDateTime) — required to compare calendar durations. |

### `Temporal.DurationConstructor`

Constructor object for `Temporal.Duration`.

| Member | Description |
| --- | --- |
| `compare(a: Temporal.Duration \| Temporal.DurationFields, b: Temporal.Duration \| Temporal.DurationFields, options?: Temporal.DurationCompareOptions): number` | Compares two Durations: `-1`, `0`, or `1`. |
| `from(item: string \| Temporal.Duration \| Temporal.DurationFields): Temporal.Duration` | Build a Duration from an ISO 8601 string (e.g. `"PT1H30M"`), an existing Duration, or a fields bag (`{ hours: 1 }`). |
| `new(fields: Temporal.DurationFields): Temporal.Duration` | Construct a Duration from a fields bag (e.g. `{ hours: 1, minutes: 30 }`). |

### `Temporal.DurationFields`

Options bag for `new Temporal.Duration({...})`.

| Member | Description |
| --- | --- |
| `readonly days?: number` | Calendar days. |
| `readonly hours?: number` | Hours. |
| `readonly microseconds?: number` | Microseconds. |
| `readonly milliseconds?: number` | Milliseconds. |
| `readonly minutes?: number` | Minutes. |
| `readonly months?: number` | Calendar months. |
| `readonly nanoseconds?: number` | Nanoseconds. |
| `readonly seconds?: number` | Seconds. |
| `readonly weeks?: number` | Calendar weeks (7 days each). |
| `readonly years?: number` | Calendar years. |

### `Temporal.DurationRoundOptions`

Options for `Temporal.Duration.round(...)`.

| Member | Description |
| --- | --- |
| `readonly largestUnit?: string` | Largest unit to balance into (e.g. `"hour"`). |
| `readonly relativeTo?: Temporal.PlainDate \| Temporal.ZonedDateTime` | Anchor (PlainDate or ZonedDateTime) — required to round calendar units. |
| `readonly roundingIncrement?: number` | Round to a multiple of this increment (default 1). |
| `readonly roundingMode?: string` | `"halfExpand"` (default), `"ceil"`, `"floor"`, `"trunc"`, `"halfEven"`, … |
| `readonly smallestUnit?: string` | Smallest unit to keep (e.g. `"second"`). |

### `Temporal.DurationToStringOptions`

Options for `Temporal.Duration.prototype.toString(...)`.

| Member | Description |
| --- | --- |
| `readonly fractionalSecondDigits?: number` | Exact fractional second digits to print, from 0 through 9. |
| `readonly roundingMode?: string` | `"trunc"` (default), `"ceil"`, `"floor"`, `"halfExpand"`, … |
| `readonly smallestUnit?: string` | Smallest unit to keep while formatting. |

### `Temporal.DurationTotalOptions`

Options for `Temporal.Duration.prototype.total(...)`.

| Member | Description |
| --- | --- |
| `readonly relativeTo?: Temporal.PlainDate \| Temporal.ZonedDateTime` | Anchor (PlainDate or ZonedDateTime) — required for calendar units. |
| `readonly unit: string` | The unit to total into (e.g. `"hours"`, `"days"`). |

### `Temporal.Instant`

A point in time, to nanosecond precision.

| Member | Description |
| --- | --- |
| `readonly epochMilliseconds: number` | Milliseconds since the Unix epoch (1970-01-01T00:00:00Z). |
| `readonly epochNanoseconds: bigint` | Nanoseconds since the Unix epoch as a `bigint` (full precision). |
| `add(d: Temporal.Duration \| Temporal.DurationFields): Temporal.Instant` | Returns a new Instant `d` later. |
| `equals(other: Temporal.Instant): boolean` | `true` if `this` and `other` refer to the same instant. |
| `round(roundTo: string \| Temporal.InstantRoundOptions): Temporal.Instant` | Rounds this Instant to the requested time unit and increment. |
| `since(other: Temporal.Instant, options?: Temporal.SinceUntilOptions): Temporal.Duration` | Returns the Duration from `other` to `this` (positive if `this` is later). |
| `subtract(d: Temporal.Duration \| Temporal.DurationFields): Temporal.Instant` | Returns a new Instant `d` earlier. |
| `toJSON(): string` | Returns the ISO string form used by JSON.stringify. |
| `toString(): string` | Returns the ISO 8601 representation (e.g. `"2024-03-09T15:30:45.123Z"`). |
| `toZonedDateTimeISO(tz: string): Temporal.ZonedDateTime` | Project this Instant onto the IANA time zone `tz`. |
| `until(other: Temporal.Instant, options?: Temporal.SinceUntilOptions): Temporal.Duration` | Returns the Duration from `this` to `other` (positive if `other` is later). |

### `Temporal.InstantConstructor`

Constructor object for `Temporal.Instant`.

| Member | Description |
| --- | --- |
| `compare(a: Temporal.Instant, b: Temporal.Instant): number` | Orders two Instants: -1, 0, or 1. |
| `from(iso: string): Temporal.Instant` | Parse an ISO 8601 instant string (e.g. `"2024-03-09T15:30:45.123Z"`). |
| `fromEpochMilliseconds(ms: number): Temporal.Instant` | Construct an Instant from milliseconds since the Unix epoch. |
| `fromEpochNanoseconds(ns: bigint): Temporal.Instant` | Construct an Instant from nanoseconds since the Unix epoch (full precision). |

### `Temporal.InstantRoundOptions`

Options for `Temporal.Instant.round(...)`.

| Member | Description |
| --- | --- |
| `readonly roundingIncrement?: number` | Positive increment of `smallestUnit`; defaults to 1. |
| `readonly roundingMode?: string` | Rounding mode; defaults to `"halfExpand"`. |
| `readonly smallestUnit: string` | Smallest time unit to retain, from hour through nanosecond. |

### `Temporal.PlainDate`

A calendar date (year, month, day) with no time or time zone.

| Member | Description |
| --- | --- |
| `readonly day: number` | Day of the month (1–31). |
| `readonly dayOfWeek: number` | Day of the week, 1 (Monday) through 7 (Sunday). |
| `readonly dayOfYear: number` | Day of the year (1–365 or 366). |
| `readonly daysInMonth: number` | Days in this month. |
| `readonly daysInWeek: number` | Days in this ISO week. |
| `readonly daysInYear: number` | Days in this year. |
| `readonly inLeapYear: boolean` | True when the ISO calendar year is a leap year. |
| `readonly month: number` | Calendar month (1–12). |
| `readonly monthCode: string` | ISO month code, e.g. `"M06"`. |
| `readonly monthsInYear: number` | Months in this year. |
| `readonly weekOfYear: number` | ISO week number. |
| `readonly year: number` | Calendar year. |
| `readonly yearOfWeek: number` | ISO week-numbering year. |
| `add(duration: Temporal.Duration \| Temporal.DurationFields): Temporal.PlainDate` | Returns a new value with `duration` added. |
| `equals(other: Temporal.PlainDate): boolean` | `true` if `this` and `other` denote the same value. |
| `since(other: Temporal.PlainDate, options?: Temporal.SinceUntilOptions): Temporal.Duration` | The `Duration` from `other` since which `this` occurs. |
| `subtract(duration: Temporal.Duration \| Temporal.DurationFields): Temporal.PlainDate` | Returns a new value with `duration` subtracted. |
| `toJSON(): string` | Returns the ISO string form used by JSON.stringify. |
| `toPlainDateTime(time?: Temporal.PlainTime): Temporal.PlainDateTime` | Combines this date with `time` (default midnight) into a `PlainDateTime`. |
| `toPlainMonthDay(): Temporal.PlainMonthDay` | The month-and-day part as a `PlainMonthDay`. |
| `toPlainYearMonth(): Temporal.PlainYearMonth` | The year-and-month part as a `PlainYearMonth`. |
| `toString(): string` | The ISO 8601 string form. |
| `toZonedDateTime(timeZoneOrOptions: string \| Temporal.PlainDateToZonedOptions): Temporal.ZonedDateTime` | Interprets this date in a time zone — pass a zone id (midnight) or `{ timeZone, plainTime? |
| `until(other: Temporal.PlainDate, options?: Temporal.SinceUntilOptions): Temporal.Duration` | The `Duration` from `this` until `other`. |
| `with(fields: Temporal.PlainDateFields): Temporal.PlainDate` | Returns a copy with the listed fields replaced; unlisted fields are kept. |

### `Temporal.PlainDateConstructor`

Constructor object, accessed via the `Temporal` binding.

| Member | Description |
| --- | --- |
| `compare(a: Temporal.PlainDate, b: Temporal.PlainDate): number` | Orders two values: -1, 0, or 1. |
| `from(iso: string): Temporal.PlainDate` | Parse an ISO 8601 string into this Plain type. |

### `Temporal.PlainDateFields`

Partial fields bag for `with(...)`.

| Member | Description |
| --- | --- |
| `readonly day?: number` | Optional field; omitted fields keep the receiver's value. |
| `readonly month?: number` | Optional field; omitted fields keep the receiver's value. |
| `readonly year?: number` | Optional field; omitted fields keep the receiver's value. |

### `Temporal.PlainDateTime`

A calendar date and wall-clock time with no time zone.

| Member | Description |
| --- | --- |
| `readonly day: number` | Day of the month (1–31). |
| `readonly dayOfWeek: number` | Day of the week, 1 (Monday) through 7 (Sunday). |
| `readonly dayOfYear: number` | Day of the year (1–365 or 366). |
| `readonly daysInMonth: number` | Days in this month. |
| `readonly daysInWeek: number` | Days in this ISO week. |
| `readonly daysInYear: number` | Days in this year. |
| `readonly hour: number` | Hour of the day (0–23). |
| `readonly inLeapYear: boolean` | True when the ISO calendar year is a leap year. |
| `readonly microsecond: number` | Microsecond within the millisecond (0–999). |
| `readonly millisecond: number` | Millisecond within the second (0–999). |
| `readonly minute: number` | Minute of the hour (0–59). |
| `readonly month: number` | Calendar month (1–12). |
| `readonly monthCode: string` | ISO month code, e.g. `"M06"`. |
| `readonly monthsInYear: number` | Months in this year. |
| `readonly nanosecond: number` | Nanoseconds within the second (0–999_999_999). |
| `readonly second: number` | Second of the minute (0–59). |
| `readonly weekOfYear: number` | ISO week number. |
| `readonly year: number` | Calendar year. |
| `readonly yearOfWeek: number` | ISO week-numbering year. |
| `add(duration: Temporal.Duration \| Temporal.DurationFields): Temporal.PlainDateTime` | Returns a new value with `duration` added. |
| `equals(other: Temporal.PlainDateTime): boolean` | `true` if `this` and `other` denote the same value. |
| `since(other: Temporal.PlainDateTime, options?: Temporal.SinceUntilOptions): Temporal.Duration` | The `Duration` from `other` since which `this` occurs. |
| `subtract(duration: Temporal.Duration \| Temporal.DurationFields): Temporal.PlainDateTime` | Returns a new value with `duration` subtracted. |
| `toJSON(): string` | Returns the ISO string form used by JSON.stringify. |
| `toPlainDate(): Temporal.PlainDate` | The calendar-date part as a `PlainDate`. |
| `toPlainMonthDay(): Temporal.PlainMonthDay` | The month-and-day part as a `PlainMonthDay`. |
| `toPlainTime(): Temporal.PlainTime` | The wall-clock-time part as a `PlainTime`. |
| `toPlainYearMonth(): Temporal.PlainYearMonth` | The year-and-month part as a `PlainYearMonth`. |
| `toString(): string` | The ISO 8601 string form. |
| `toZonedDateTime(timeZone: string): Temporal.ZonedDateTime` | Interprets this wall clock in `timeZone`, resolving DST gaps with `compatible` disambiguation. |
| `until(other: Temporal.PlainDateTime, options?: Temporal.SinceUntilOptions): Temporal.Duration` | The `Duration` from `this` until `other`. |
| `with(fields: Temporal.PlainDateTimeFields): Temporal.PlainDateTime` | Returns a copy with the listed fields replaced; unlisted fields are kept. |

### `Temporal.PlainDateTimeConstructor`

Constructor object, accessed via the `Temporal` binding.

| Member | Description |
| --- | --- |
| `compare(a: Temporal.PlainDateTime, b: Temporal.PlainDateTime): number` | Orders two values: -1, 0, or 1. |
| `from(iso: string): Temporal.PlainDateTime` | Parse an ISO 8601 string into this Plain type. |

### `Temporal.PlainDateTimeFields`

Partial fields bag for `with(...)`.

| Member | Description |
| --- | --- |
| `readonly day?: number` | Optional field; omitted fields keep the receiver's value. |
| `readonly hour?: number` | Optional field; omitted fields keep the receiver's value. |
| `readonly microsecond?: number` | Optional field; omitted fields keep the receiver's value. |
| `readonly millisecond?: number` | Optional field; omitted fields keep the receiver's value. |
| `readonly minute?: number` | Optional field; omitted fields keep the receiver's value. |
| `readonly month?: number` | Optional field; omitted fields keep the receiver's value. |
| `readonly nanosecond?: number` | Optional field; omitted fields keep the receiver's value. |
| `readonly second?: number` | Optional field; omitted fields keep the receiver's value. |
| `readonly year?: number` | Optional field; omitted fields keep the receiver's value. |

### `Temporal.PlainDateToZonedOptions`

Options for `PlainDate.toZonedDateTime`.

| Member | Description |
| --- | --- |
| `readonly plainTime?: Temporal.PlainTime` | Wall-clock time; defaults to midnight when omitted. |
| `readonly timeZone: string` | IANA time-zone id (e.g. `"America/New_York"`). |

### `Temporal.PlainMonthDay`

A calendar month and day with no year or time zone (e.g. `06-07`).

| Member | Description |
| --- | --- |
| `readonly day: number` | Day of the month (1–31). |
| `readonly month: number` | Calendar month (1–12). |
| `readonly monthCode: string` | ISO month code, e.g. `"M06"`. |
| `equals(other: Temporal.PlainMonthDay): boolean` | `true` if `this` and `other` denote the same value. |
| `toJSON(): string` | Returns the ISO string form used by JSON.stringify. |
| `toPlainDate(fields: Temporal.PlainMonthDayToDateFields): Temporal.PlainDate` | Adds a `year` to make a `PlainDate`; a Feb-29 month-day constrains to Feb-28 in a non-leap year. |
| `toString(): string` | The ISO 8601 string form. |
| `with(fields: Temporal.PlainMonthDayFields): Temporal.PlainMonthDay` | Returns a copy with the listed fields replaced; unlisted fields are kept. |

### `Temporal.PlainMonthDayConstructor`

Constructor object, accessed via the `Temporal` binding.

| Member | Description |
| --- | --- |
| `from(iso: string): Temporal.PlainMonthDay` | Parse an ISO 8601 string into this Plain type. |

### `Temporal.PlainMonthDayFields`

Partial fields bag for `with(...)`.

| Member | Description |
| --- | --- |
| `readonly day?: number` | Optional field; omitted fields keep the receiver's value. |
| `readonly month?: number` | Optional field; omitted fields keep the receiver's value. |

### `Temporal.PlainMonthDayToDateFields`

The field that completes this value into a `PlainDate`.

| Member | Description |
| --- | --- |
| `readonly year: number` | Calendar year; a Feb-29 month-day constrains to Feb-28 in a non-leap year. |

### `Temporal.PlainTime`

A wall-clock time (hour, minute, second, nanosecond) with no date or time zone.

| Member | Description |
| --- | --- |
| `readonly hour: number` | Hour of the day (0–23). |
| `readonly microsecond: number` | Microsecond within the millisecond (0–999). |
| `readonly millisecond: number` | Millisecond within the second (0–999). |
| `readonly minute: number` | Minute of the hour (0–59). |
| `readonly nanosecond: number` | Nanoseconds within the second (0–999_999_999). |
| `readonly second: number` | Second of the minute (0–59). |
| `add(duration: Temporal.Duration \| Temporal.DurationFields): Temporal.PlainTime` | Returns a new value with `duration` added. |
| `equals(other: Temporal.PlainTime): boolean` | `true` if `this` and `other` denote the same value. |
| `since(other: Temporal.PlainTime, options?: Temporal.SinceUntilOptions): Temporal.Duration` | The `Duration` from `other` since which `this` occurs. |
| `subtract(duration: Temporal.Duration \| Temporal.DurationFields): Temporal.PlainTime` | Returns a new value with `duration` subtracted. |
| `toJSON(): string` | Returns the ISO string form used by JSON.stringify. |
| `toString(): string` | The ISO 8601 string form. |
| `until(other: Temporal.PlainTime, options?: Temporal.SinceUntilOptions): Temporal.Duration` | The `Duration` from `this` until `other`. |
| `with(fields: Temporal.PlainTimeFields): Temporal.PlainTime` | Returns a copy with the listed fields replaced; unlisted fields are kept. |

### `Temporal.PlainTimeConstructor`

Constructor object, accessed via the `Temporal` binding.

| Member | Description |
| --- | --- |
| `compare(a: Temporal.PlainTime, b: Temporal.PlainTime): number` | Orders two values: -1, 0, or 1. |
| `from(iso: string): Temporal.PlainTime` | Parse an ISO 8601 string into this Plain type. |

### `Temporal.PlainTimeFields`

Partial fields bag for `with(...)`.

| Member | Description |
| --- | --- |
| `readonly hour?: number` | Optional field; omitted fields keep the receiver's value. |
| `readonly microsecond?: number` | Optional field; omitted fields keep the receiver's value. |
| `readonly millisecond?: number` | Optional field; omitted fields keep the receiver's value. |
| `readonly minute?: number` | Optional field; omitted fields keep the receiver's value. |
| `readonly nanosecond?: number` | Optional field; omitted fields keep the receiver's value. |
| `readonly second?: number` | Optional field; omitted fields keep the receiver's value. |

### `Temporal.PlainYearMonth`

A calendar year and month with no day or time zone (e.g. `2026-06`).

| Member | Description |
| --- | --- |
| `readonly daysInMonth: number` | Days in this month. |
| `readonly daysInYear: number` | Days in this year. |
| `readonly inLeapYear: boolean` | True when the ISO calendar year is a leap year. |
| `readonly month: number` | Calendar month (1–12). |
| `readonly monthCode: string` | ISO month code, e.g. `"M06"`. |
| `readonly monthsInYear: number` | Months in this year. |
| `readonly year: number` | Calendar year. |
| `add(duration: Temporal.Duration \| Temporal.DurationFields): Temporal.PlainYearMonth` | Returns a new value with `duration` added. |
| `equals(other: Temporal.PlainYearMonth): boolean` | `true` if `this` and `other` denote the same value. |
| `since(other: Temporal.PlainYearMonth, options?: Temporal.SinceUntilOptions): Temporal.Duration` | The `Duration` from `other` since which `this` occurs. |
| `subtract(duration: Temporal.Duration \| Temporal.DurationFields): Temporal.PlainYearMonth` | Returns a new value with `duration` subtracted. |
| `toJSON(): string` | Returns the ISO string form used by JSON.stringify. |
| `toPlainDate(fields: Temporal.PlainYearMonthToDateFields): Temporal.PlainDate` | Adds a `day` to make a `PlainDate`; the day constrains to the month length. |
| `toString(): string` | The ISO 8601 string form. |
| `until(other: Temporal.PlainYearMonth, options?: Temporal.SinceUntilOptions): Temporal.Duration` | The `Duration` from `this` until `other`. |
| `with(fields: Temporal.PlainYearMonthFields): Temporal.PlainYearMonth` | Returns a copy with the listed fields replaced; unlisted fields are kept. |

### `Temporal.PlainYearMonthConstructor`

Constructor object, accessed via the `Temporal` binding.

| Member | Description |
| --- | --- |
| `compare(a: Temporal.PlainYearMonth, b: Temporal.PlainYearMonth): number` | Orders two values: -1, 0, or 1. |
| `from(iso: string): Temporal.PlainYearMonth` | Parse an ISO 8601 string into this Plain type. |

### `Temporal.PlainYearMonthFields`

Partial fields bag for `with(...)`.

| Member | Description |
| --- | --- |
| `readonly month?: number` | Optional field; omitted fields keep the receiver's value. |
| `readonly year?: number` | Optional field; omitted fields keep the receiver's value. |

### `Temporal.PlainYearMonthToDateFields`

The field that completes this value into a `PlainDate`.

| Member | Description |
| --- | --- |
| `readonly day: number` | Day of the month (1–31); constrained to the month length. |

### `Temporal.SinceUntilOptions`

Options for `until`/`since` on every Temporal type.

| Member | Description |
| --- | --- |
| `readonly largestUnit?: string` | Largest unit to balance into (e.g. `"hours"`, `"months"`). |
| `readonly roundingIncrement?: number` | Round to a multiple of this increment of `smallestUnit` (default 1). |
| `readonly roundingMode?: string` | `"halfExpand"` (default), `"ceil"`, `"floor"`, `"trunc"`, `"halfEven"`, … |
| `readonly smallestUnit?: string` | Smallest unit to keep; the result is rounded to it. |

### `Temporal.ZonedDateTime`

An Instant paired with an IANA time zone.

| Member | Description |
| --- | --- |
| `readonly day: number` | Day of the month (1–31). |
| `readonly dayOfWeek: number` | Day of the week, 1 (Monday) through 7 (Sunday). |
| `readonly dayOfYear: number` | Day of the year (1–365 or 366). |
| `readonly daysInMonth: number` | Days in this month. |
| `readonly daysInWeek: number` | Days in this ISO week. |
| `readonly daysInYear: number` | Days in this year. |
| `readonly epochMilliseconds: number` | Milliseconds since the Unix epoch (timezone-independent). |
| `readonly epochNanoseconds: bigint` | Nanoseconds since the Unix epoch. |
| `readonly hour: number` | Hour of the day (0–23). |
| `readonly hoursInDay: number` | Elapsed hours in this local day, accounting for time-zone transitions. |
| `readonly inLeapYear: boolean` | True when the ISO calendar year is a leap year. |
| `readonly microsecond: number` | Microsecond within the millisecond (0–999). |
| `readonly millisecond: number` | Millisecond within the second (0–999). |
| `readonly minute: number` | Minute of the hour (0–59). |
| `readonly month: number` | Calendar month (1–12). |
| `readonly monthCode: string` | ISO month code, e.g. `"M06"`. |
| `readonly monthsInYear: number` | Months in this year. |
| `readonly nanosecond: number` | Nanosecond within the microsecond (0–999). |
| `readonly offset: string` | UTC offset at this instant, such as `"-05:00"`. |
| `readonly offsetNanoseconds: number` | UTC offset at this instant in nanoseconds. |
| `readonly second: number` | Second of the minute (0–59 or 60 on a leap-second). |
| `readonly timeZoneId: string` | The IANA time-zone id (e.g. `"America/New_York"`). |
| `readonly weekOfYear: number` | ISO week number. |
| `readonly year: number` | Calendar year in this time zone. |
| `readonly yearOfWeek: number` | ISO week-numbering year. |
| `add(d: Temporal.Duration \| Temporal.DurationFields): Temporal.ZonedDateTime` | Returns a new ZonedDateTime `d` later. |
| `equals(other: Temporal.ZonedDateTime): boolean` | `true` if `this` and `other` have the same instant **and** the same time-zone id. |
| `round(roundTo: string \| Temporal.ZonedDateTimeRoundOptions): Temporal.ZonedDateTime` | Rounds this ZonedDateTime in its time zone, from day through nanosecond precision. |
| `since(other: Temporal.ZonedDateTime, options?: Temporal.SinceUntilOptions): Temporal.Duration` | Returns the calendar-aware Duration from `other` to `this`. |
| `startOfDay(): Temporal.ZonedDateTime` | Returns the first valid instant on this date in the current time zone. |
| `subtract(d: Temporal.Duration \| Temporal.DurationFields): Temporal.ZonedDateTime` | Returns a new ZonedDateTime `d` earlier. |
| `toInstant(): Temporal.Instant` | Drops the time-zone, returns the underlying Instant. |
| `toJSON(): string` | Returns the ISO string form used by JSON.stringify. |
| `toPlainDate(): Temporal.PlainDate` | The wall-clock calendar date in this time zone, as a tz-free `PlainDate`. |
| `toPlainDateTime(): Temporal.PlainDateTime` | The wall-clock date and time in this time zone, as a tz-free `PlainDateTime`. |
| `toPlainTime(): Temporal.PlainTime` | The wall-clock time in this time zone, as a tz-free `PlainTime` (with nanosecond precision). |
| `toString(): string` | Returns the ISO 8601 form with bracketed tz (e.g. `"2024-03-09T12:00:00+00:00[UTC]"`). |
| `until(other: Temporal.ZonedDateTime, options?: Temporal.SinceUntilOptions): Temporal.Duration` | Returns the calendar-aware Duration from `this` to `other`. |
| `with(fields: Temporal.ZonedDateTimeFields): Temporal.ZonedDateTime` | Returns a copy with the listed wall-clock fields replaced (keeping the time zone); unlisted fields are kept. |
| `withTimeZone(tz: string): Temporal.ZonedDateTime` | Same instant, different IANA time zone. |

### `Temporal.ZonedDateTimeConstructor`

Constructor object for `Temporal.ZonedDateTime`.

| Member | Description |
| --- | --- |
| `compare(a: Temporal.ZonedDateTime, b: Temporal.ZonedDateTime): number` | Orders two ZonedDateTimes by their exact instant: -1, 0, or 1. |
| `from(iso: string): Temporal.ZonedDateTime` | Parse an ISO 8601 zoned datetime with bracketed tz (e.g. `"2026-01-15T10:00:00-05:00[America/New_York]"`). |

### `Temporal.ZonedDateTimeFields`

Partial fields bag for `with(...)`.

| Member | Description |
| --- | --- |
| `readonly day?: number` | Optional field; omitted fields keep the receiver's value. |
| `readonly hour?: number` | Optional field; omitted fields keep the receiver's value. |
| `readonly microsecond?: number` | Optional field; omitted fields keep the receiver's value. |
| `readonly millisecond?: number` | Optional field; omitted fields keep the receiver's value. |
| `readonly minute?: number` | Optional field; omitted fields keep the receiver's value. |
| `readonly month?: number` | Optional field; omitted fields keep the receiver's value. |
| `readonly nanosecond?: number` | Optional field; omitted fields keep the receiver's value. |
| `readonly second?: number` | Optional field; omitted fields keep the receiver's value. |
| `readonly year?: number` | Optional field; omitted fields keep the receiver's value. |

### `Temporal.ZonedDateTimeRoundOptions`

Options for `Temporal.ZonedDateTime.round(...)`.

| Member | Description |
| --- | --- |
| `readonly roundingIncrement?: number` | Positive increment of `smallestUnit`; defaults to 1. |
| `readonly roundingMode?: string` | Rounding mode; defaults to `"halfExpand"`. |
| `readonly smallestUnit: string` | Smallest wall-clock unit to retain, from day through nanosecond. |

| Function | Capability | Description |
| --- | --- | --- |
| `instant(): Temporal.Instant` |  | The current wall-clock instant. |
| `plainDateISO(tz?: string \| null): Temporal.PlainDate` |  | Today's date in `tz`, or the system (local) zone if `tz` is omitted / null. |
| `plainDateTimeISO(tz?: string \| null): Temporal.PlainDateTime` |  | The current date and wall-clock time in `tz`, or the system (local) zone if `tz` is omitted / null. |
| `plainTimeISO(tz?: string \| null): Temporal.PlainTime` |  | The current wall-clock time in `tz`, or the system (local) zone if `tz` is omitted / null. |
| `timeZoneId(): string` |  | The system IANA time-zone id (or `"UTC"` if the system zone can't be detected). |
| `zonedDateTime(tz?: string \| null): Temporal.ZonedDateTime` |  | The current ZonedDateTime in `tz`, or the system zone if `tz` is null. |
| `zonedDateTimeISO(tz?: string \| null): Temporal.ZonedDateTime` |  | The current ZonedDateTime in `tz`, or the system zone if `tz` is omitted / null. |

<!-- /generated:builtins -->
