---
title: "The language"
description: Write TypeScript in Submilli, return a result from main, and use HTTP and the filesystem.
slug: the-language
sidebar:
  order: 4
---

You write Submilli programs in TypeScript. Submilli makes deliberate choices
about which TypeScript features it supports and how some of them behave.
Without TypeScript's need to accommodate existing JavaScript code, Submilli
can enforce stricter type checks. For example, it excludes `any`, and `as` is
a runtime type assertion that checks whether a value matches the stated type.
This chapter shows how to use TypeScript in Submilli and explains the differences
you'll encounter along the way.

## Return the answer from `main`

Every program needs a parameterless `main` function. Submilli calls it for you:

```typescript title="hello.ts"
function main(): string {
  console.log("Starting the program");
  return "Hello from Submilli";
}
```

Save this as `hello.ts` and run `submilli run hello.ts`. The returned result is
`Hello from Submilli`.

Unlike a regular Node.js script, a Submilli program returns its answer from
`main`; `console.log` is for logging. When an agent framework such as LangChain
calls Submilli, a successful run returns only that answer, so the agent doesn't
have to extract it from console output. The framework can retrieve the logs
through another tool call.

Strings are returned verbatim, numbers and booleans become text, and objects and
arrays serialize to JSON. Return an object directly for a structured answer;
you don't need `JSON.stringify`.

Function declarations and class methods require return types, such as `: string`
above. Use `: void` for no result. Arrow functions can infer their return type;
local variables can infer their type from the assigned value.

## Fetch tasks and save a report

This program fetches a todo list and saves the unfinished titles, one per line.
It uses JSONPlaceholder, a public API with fake data for testing. No account or
API key is needed. Save it as `todos.ts`:

```typescript title="todos.ts"
import * as http from "submilli:http";
import * as fs from "submilli:fs";

interface Todo {
  id: number;
  title: string;
  completed: boolean;
}

interface Report {
  path: string;
  unfinished: number;
}

function unfinishedTitles(todos: Todo[]): string[] {
  const titles: string[] = [];
  for (const todo of todos) {
    if (!todo.completed) {
      titles.push(todo.title);
    }
  }
  return titles;
}

function main(): Report {
  const response = http.get(
    "https://jsonplaceholder.typicode.com/todos?userId=1"
  );
  response.throwForStatus();

  const todos = response.json() as Todo[];
  const titles = unfinishedTitles(todos);
  const path = "/unfinished.txt";
  fs.writeText(path, titles.join("\n"));

  return { path, unfinished: titles.length };
}
```

For nine unfinished tasks, the returned result is:

```json
{"path":"/unfinished.txt","unfinished":9}
```

`http.get` returns its response directly. I/O is synchronous from the program's
point of view; Submilli has no `async` or `await`. `throwForStatus()` throws for
a non-2xx response, and connection failures also raise errors. An uncaught error
stops the program before it writes the report.

For file operations, Submilli provides a virtual filesystem: an area for the
program's files. Its root `/` refers to that area,
not the root of your machine. `/unfinished.txt` is a file inside it.

`fs.writeText` writes UTF-8 text, replacing any existing file. The parent
directory must exist; here we write directly under `/`. Run the example:

```sh
mkdir -p todo-files
submilli check todos.ts
submilli run todos.ts --vfs ./todo-files
cat todo-files/unfinished.txt
```

`check` typechecks without executing. `--vfs` makes `todo-files` the program's
filesystem root, so `/unfinished.txt` lands there. Without it, the CLI uses a
temporary directory for the run.

A blueprint is a configuration file that defines what a program may do. Each
operation it controls, such as an HTTP GET or a file write, is a **capability**.
Local runs without a blueprint allow all capabilities. Under a blueprint, this
example needs an enabled filesystem and the `http.get` and `fs.write`
capabilities, restricted to the example's host and file path. Imports don't
grant capabilities. See the [blueprint chapter](/docs/blueprints) to configure them.

## Check external data with `unknown` and `as`

The `Todo` interface describes the fields we need; `Todo[]` is an array of those
objects. But `response.json()` and `JSON.parse(text)` return `unknown`.
Submilli deliberately doesn't support `any`, so unchecked data can't bypass
type checking and cause errors later in the program. You must check an unknown
value before using it.

In the example, `as Todo[]` checks the response at runtime. Every element must
have the required fields with the declared types. Extra fields are allowed;
a wrong type throws `TypeError`. Unlike
[TypeScript's type assertions](https://www.typescriptlang.org/docs/handbook/2/everyday-types.html#type-assertions),
Submilli's casts perform runtime validation. They don't convert strings into
numbers or otherwise repair the data.

A type annotation alone won't check the response. Replacing the cast with
`const todos: Todo[] = response.json();` produces this compile error:

```text
error: expected `Todo[]`, got `unknown`
  --> todos.ts:31:25
   |
30 |
31 |   const todos: Todo[] = response.json();
   |                         ^^^^^^^^^^^^^^^
32 |   const titles = unfinishedTitles(todos);
```

The caret identifies the expression whose type doesn't fit. Restore
`const todos = response.json() as Todo[];` to fix it. The program then compiles,
and the cast checks the actual data when it runs.

For JSON text, use `JSON.parse(text) as Todo[]`; `JSON.parse<Todo[]>(text)` isn't
supported. You can also narrow `unknown` with a condition such as
`typeof value === "string"` before calling string methods.

## Handle missing values with `null`

Submilli has no `undefined`. Nullable values use types such as `string | null`.
An omitted optional property reads as `null`:

```typescript
function assignee(task: { assignee?: string }): string {
  if (task.assignee !== null) {
    return task.assignee.toUpperCase();
  }
  return "unassigned";
}

function main(): string[] {
  return [assignee({ assignee: "dana" }), assignee({})];
}
```

The result is `["DANA","unassigned"]`. Inside the non-null branch, the compiler
knows the assignee is a string. An omitted assignee takes the fallback.

The function body can also be written as:

```typescript
return task.assignee?.toUpperCase() ?? "unassigned";
```

Here `?.` produces `null` for an absent assignee, and `??` supplies the fallback.
Use `||` to also replace values such as `0`, `false`, and an empty string.
The non-null assertion `value!` checks at runtime and throws `TypeError` if
the value is null.

## String-keyed records

Use `Record<string, V>` for an object whose property names are known at runtime.
The equivalent index-signature syntax is `{ [key: string]: V }`.

```typescript
function main(): number {
  const counts: Record<string, number> = {};
  const word = "hello";
  counts[word] = (counts[word] ?? 0) + 1;
  return counts[word]!;
}
```

Open-record reads return `V | null`; a missing key returns `null`. Writes require
`V`. A present property can also contain `null` when `V` allows it; use
`key in record` or `Object.hasOwn(record, key)` to distinguish presence.
Computed literals (`{ [key]: value }`), object spread, enumeration, and JSON
serialization use the object's actual properties. Casts such as
`JSON.parse(text) as Record<string, number>` validate the present values.

Finite records such as `Record<"name" | "email", string>` require both properties
and read them as `string`. Aliases, generic value types, string index signatures
in interfaces, and interface inheritance are supported. A `readonly` index
signature prohibits writes through that view.

Only string keys are supported. `keyof` an open string-indexed object is
`string`; numeric keys, symbol keys, general mapped types, unresolved generic
key parameters, and `delete` are unsupported. Dynamic `in` checks return a
boolean without introducing new narrowing facts.

## Other TypeScript differences

| Feature | Submilli behavior |
| --- | --- |
| `==` and `!=` | Aliases for strict equality. `1 == "1"` fails at compile time. |
| `items[index]` | Out-of-range array reads and writes throw at runtime. Use `push` to append. |
| `object[key]` | Arbitrary string keys require `Record<string, V>` or a string index signature. Missing keys return `null`. |
| Runtime APIs | Use Submilli's standard library and installed Submilli packages. Node.js APIs, browser globals, and npm packages aren't available. |
| `Date` | Use the built-in `Temporal` API. |
| `Symbol`, `Proxy`, prototype reflection | Not available. |

Look up exact API declarations before relying on a method:

```sh
submilli docs submilli:http
submilli docs submilli:fs
submilli builtins Map
```

Agents use `packages.docs` and `builtins.docs` for the same information.
The [standard-library chapter](/docs/standard-library) is still being written;
these commands provide the current reference.
