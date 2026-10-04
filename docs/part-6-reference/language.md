---
title: "Language"
description: "The TypeScript Submilli programs are written in: the shape of a program, the stricter checks Submilli makes and why, and how to look declarations up."
slug: reference/language
sidebar:
  order: 4
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
| `void` | Nothing |

`console.log` writes to a separate log, not to the result. `submilli run
<file>` runs a program. `submilli check <file>` only compiles it.

Modules are imported by name: `submilli:<name>` for the standard library,
`@<org>/<name>` for installed packages, and `@mcp/<server>` for MCP servers
a blueprint declares. Namespace (`import * as fs`), default
(`import fs`), and named (`import { sha256 }`) imports all work. Built-ins
need no import. Importing a module grants nothing. Every gated call is
checked against the blueprint.

## Stricter by design

### `unknown` instead of `any`

`JSON.parse` and `response.json()` return `unknown`, and Submilli has no
`any`, so data from outside can't bypass type checking and fail later in the
program. Narrow an `unknown` value with `typeof`, `Array.isArray`,
`instanceof`, `in`, or `=== null`, or cast it with `as`:

```typescript
const tickets = JSON.parse(text) as Ticket[];
```

### `as` checks the value

A cast checks the value when it runs: every required field present with its
type, every array element matching, extra fields allowed. Data of the wrong
shape throws `TypeError` at the cast, where it arrived. A cast doesn't
convert, so the string `"3"` stays a string.

### One absent value: `null`

Submilli has no `undefined`, so a missing value is always `null`, typed
`T | null`. An omitted optional property, a missing `Map` key, and a missing
record key all read as `null`.

| Write | Means |
| --- | --- |
| `if (x !== null)` | `x` is `T` inside the branch |
| `x?.field`, `x?.method()` | `null` when `x` is `null` |
| `x ?? fallback` | `fallback` when `x` is `null` |
| `x!` | `x` as `T`. Throws `TypeError` when it is `null` |

### Calls return their result

Every call into the standard library or a package returns its result
directly, with no `async`, `await`, or `Promise`, so a program reads top to
bottom:

```typescript
const page = jina.read(url);
fs.writeText("notes.md", summarize(page));
```

### `Record<string, V>` for runtime keys

An object whose keys are known only at runtime is a `Record<string, V>`, and
reading a missing key gives `null`:

```typescript
const counts: Record<string, number> = {};
for (const word of words) {
  counts[word] = (counts[word] ?? 0) + 1;
}
```

### Other differences

| Feature | In Submilli |
| --- | --- |
| `==` and `!=` | Strict, like `===` and `!==`, so `1 == "1"` is a compile error |
| Return types | Declared on functions and methods; arrow functions infer them |
| `items[i]` | Out-of-range reads and writes throw `RangeError`. `push` appends |
| Runtime APIs | The standard library and Submilli packages, in place of Node.js APIs, browser globals, and npm |
| Dates and times | `Temporal`, in place of `Date` |
| `Symbol`, `Proxy`, prototype reflection | Not available |

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
threw. A call the blueprint doesn't allow throws `PermissionDeniedError`
([Permissions](/docs/reference/permissions#denials-at-run-time)).

## Looking declarations up

| Command | Prints |
| --- | --- |
| `submilli search <query>` | Modules and packages whose name, description, or exports match, such as `submilli search writeText` |
| `submilli docs <module>` | A module's declarations, such as `submilli docs submilli:http`. With `--blueprint <file>`, only what that blueprint's programs can import |
| `submilli builtins` | The built-in types and namespaces |
| `submilli builtins <name>` | A built-in's declarations, or one member, such as `submilli builtins Map.get` |

```text
$ submilli builtins Map.get
interface Map<K, V> {
  /**
   * Returns the value associated with `key`, or `null` if the key is not present.
   * @param key The key to look up.
   */
  get(key: K): V | null;
}
```
