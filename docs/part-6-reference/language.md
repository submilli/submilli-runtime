---
title: "Language"
description: "The TypeScript Submilli programs are written in: the shape of a program, the stricter checks Submilli makes and why, and how to look declarations up."
slug: reference/language
sidebar:
  order: 4
authorship:
  label: ai-assisted
  confirmed: true
  contentHash: "cdf3fdb1d810a1bc20c4f3cc6014e43b0f8a457bdf23369787ad5c3b38d97929"
  confirmedAt: "2026-10-05T13:01:53.009Z"
---

You write Submilli programs in TypeScript. Submilli makes deliberate choices
about which TypeScript features it supports and how some of them behave.
Without TypeScript's need to accommodate existing JavaScript code, it can
enforce stricter checks. It excludes `any`, and `as` checks that a value
matches its type. This page describes the shape of a program and those
choices. The globals a program has are on
[Built-ins](/docs/reference/built-ins). The `submilli:` modules are on
[Standard library](/docs/reference/standard-library).

## A program

A program is one `.ts` file with a function named `main`, which takes no
parameters and declares its return type. Top-level statements run first,
then `main`. What `main` returns is the program's result.

```typescript title="tickets.ts"
import * as fs from "submilli:fs";

interface Ticket {
  id: string;
  hours: number;
}

function main(): { count: number; hours: number } {
  const tickets = JSON.parse(fs.readText("/tickets.json") ?? "[]") as Ticket[];
  console.log(`read ${tickets.length} tickets`);
  return { count: tickets.length, hours: tickets.reduce((sum, t) => sum + t.hours, 0) };
}
```

| `main` returns | The result is |
| --- | --- |
| `string` | The string |
| `number`, `boolean` | Its text, such as `5` or `true` |
| An object or array | JSON, with object keys sorted |
| `void` or `undefined` | Nothing |
| `null` | The JSON text `null` |

`console.log` writes to a separate log, not to the result. `submilli run
<file>` runs a program. `submilli check <file>` only compiles it.

Modules are imported by name: `submilli:<name>` for the standard library,
`@<org>/<name>` for installed Packages, and `@mcp/<server>` for MCP servers
a Blueprint declares. Namespace (`import * as fs`), default
(`import fs`), and named (`import { sha256 }`) imports all work. Built-ins
need no import. Importing a module grants nothing. Every gated call is
checked against the Blueprint.

## Stricter by design

### `unknown` instead of `any`

`JSON.parse` and `response.json()` return `unknown`, and Submilli has no
`any`, so data from outside can't bypass type checking and fail later in the
program. Narrow an `unknown` value with `typeof`, `Array.isArray`,
`instanceof`, `in`, `=== null`, or `=== undefined`, or cast it with `as`:

```typescript
const tickets = JSON.parse(text) as Ticket[];
```

### `as` checks the value

A cast checks the value when it runs: every required field present with its
type, every array element matching, extra fields allowed. Data of the wrong
shape throws `TypeError` at the cast, where it arrived. A cast doesn't
convert, so the string `"3"` stays a string. An optional field `a?: T` may
be missing but rejects JSON `null`. Declare a field an API may send as
`null` with `a?: T | null` or `a: T | null`.

### Missing values: `undefined`

`undefined` is a primitive type with one value. Omitted optional properties,
missing `Map` and record keys, and omitted optional parameters produce
`undefined`. Explicit `null` remains a separate value and must be declared
when it is accepted.

| Declaration | May omit the property? | Read type |
| --- | --- | --- |
| `a?: string` | Yes | `string \| undefined` |
| `a: string \| undefined` | No | `string \| undefined` |
| `a?: string \| null` | Yes | `string \| null \| undefined` |

An optional field permits explicit `undefined`. An absent field is different
from a present field whose value is undefined: `"a" in obj` and `Object.keys`
check presence. An `in` check alone cannot prove an optional field's value is
defined.

| Write | Means |
| --- | --- |
| `if (x !== undefined)` | Remove `undefined` from `x`'s type in the branch |
| `if (x !== null)` | Remove `null`, leaving any `undefined` member |
| `x?.field`, `x?.method()` | Produce `undefined` when `x` is null or undefined |
| `x ?? fallback` | Use `fallback` when `x` is null or undefined |
| `x!` | Remove both nullish types; throw `TypeError` for either value |

`==` and `!=` against `null` or `undefined` are compile errors, because `==`
compares like `===` and would not match the other value. To test for either
value, use `x === null || x === undefined`.

### Optional parameters, tuples, and methods

A parameter declared `name?: string` accepts omission or explicit undefined.
A required parameter declared `name: string | undefined` still requires an
argument. Default parameter and destructuring initializers run for omission
or undefined, never null. Defaults run once, in order, in the callee's scope.
A callback created in a default may capture a later parameter. Reading or
assigning that parameter before its initialization throws `ReferenceError`.

These declarations show the supported forms:

```typescript
function greeting(name: string = "guest"): string {
  return `Hello, ${name}`;
}

type PagePosition = [offset: number, cursor?: string];
interface Reporter {
  report?(message: string): void;
}
```

An omitted optional tuple element does not extend the array; reading that
position returns undefined. Required tuple elements must precede optional
ones. Call an optional method with `reporter.report?.("done")`, or narrow it
before calling. Optional calls preserve the method's receiver.

Bare returns and falling off a function produce undefined when its return
contract permits it. `void expression` evaluates the expression once and
produces undefined. `void` remains a distinct type: viewing a value-returning
callback as `() => void` does not change its runtime result.

### JSON and stored values

JSON has no undefined value. `JSON.stringify` omits undefined object
properties, writes undefined array elements as JSON null, and returns
undefined for top-level undefined. Parsing JSON null produces null. HTTP calls use undefined for no body and
explicit null for the JSON body `null`.

Generated input schemas include only JSON-representable members of a union.
An `a: string | undefined` property stays required; `a?: string` is optional.
An undefined-only value type cannot be represented by a JSON Schema.

Session reads have type `T | undefined`: `session.get<T>(key)` returns
undefined for a missing key and preserves a stored null. Session writes use
the same nested undefined conversion as JSON and reject top-level undefined
before changing the stored value. See
[Keep files and state](/docs/blueprints/keep-files-and-state).

### Calls return their result

Every call into the standard library or a Package returns its result
directly, with no `async`, `await`, or `Promise`, so a program reads top to
bottom:

```typescript
const page = jina.read(url);
fs.writeText("notes.md", summarize(page));
```

### `Record<string, V>` for runtime keys

An object whose keys are known only at runtime is a `Record<string, V>`, and
reading a missing key gives `undefined` (the read type is `V | undefined`):

```typescript
const counts: Record<string, number> = {};
for (const word of words) {
  counts[word] = (counts[word] ?? 0) + 1;
}
```

### Other differences

| Feature | In Submilli |
| --- | --- |
| `==` and `!=` | Strict, like `===` and `!==`, so `1 == "1"` and `x == null` are compile errors |
| Return types | Declared on functions and methods; arrow functions infer them |
| `items[i]` | Out-of-range reads and writes throw `RangeError`. `items.at(i)` returns `undefined`. `push` appends |
| `let x: T;` | Needs an initializer unless `T` includes `undefined` |
| Runtime APIs | The standard library and Submilli Packages, in place of Node.js APIs, browser globals, and npm |
| Dates and times | `Temporal`, in place of `Date` |
| `Symbol`, `Proxy`, prototype reflection | Not available |

## Migrating from null-only missing values

Update guards for optional properties and missing collection results from
`=== null` to `=== undefined`. Where a value can contain both, check both or
use `??` for a fallback. Keep null checks for APIs that explicitly declare
null, including a failed `RegExp.exec` and nullable package results.

Replace null used to omit a built-in argument with undefined, or omit the
argument. Null remains valid only when its declared type includes it. An
optional `a?: T` no longer accepts null unless written `a?: T | null`.
Review JSON output that previously included null for missing optional fields;
undefined fields are now omitted. Rebuild compiled packages against the new
runtime before using them.

## Errors

A compile error shows the line, marks the position, and says how to fix it.
For a misspelled field it names the closest one and prints the type's
declaration:

```text
error: field `amout` does not exist on `Credit`
  --> credit.ts:15:29
   |
14 |   const credit = applyCredit("cus_northwind", 1500);
15 |   return `credited ${credit.amout} cents`;
   |                             ^^^^^
16 | }
   |
help: did you mean `amount`?
   |
help: /** A credit added to a customer's account. */
interface Credit {
  /** Amount in cents. */
  amount: number;
  /** The customer who received the credit. */
  customerId: string;
}
```

A runtime error names the error and shows the call stack and the line that
threw. A call the Blueprint doesn't allow throws `PermissionDeniedError`
([Permissions](/docs/reference/permissions#denials-at-run-time)).

## Looking declarations up

| Command | Prints |
| --- | --- |
| `submilli search <query>` | Modules and Packages whose name, description, or exports match, such as `submilli search writeText` |
| `submilli docs <module>` | A module's declarations, such as `submilli docs submilli:http`. With `--blueprint <file>`, only what that Blueprint's programs can import |
| `submilli builtins` | The built-in types and namespaces |
| `submilli builtins <name>` | A built-in's declarations, or one member, such as `submilli builtins Map.get` |

```text
$ submilli builtins Map.get
interface Map<K, V> {
  /**
   * Returns the value associated with `key`, or `undefined` if the key is not present.
   * @param key The key to look up.
   */
  get(key: K): V | undefined;
}
```
