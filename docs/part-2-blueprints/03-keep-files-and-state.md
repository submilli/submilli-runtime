---
title: "Keep files and state"
description: "How to give the programs in one session a filesystem and a key-value store that last between them: choose the filesystem, grant its operations, run a program, cap the files, grant session state."
slug: blueprints/keep-files-and-state
sidebar:
  order: 3
authorship:
  label: ai-assisted
  confirmed: true
  contentHash: "5f9a811e5b23b4ed98d6f7dc628e6244fd9808c02132fb09a27f25add805e8a8"
  confirmedAt: "2026-10-05T13:01:53.009Z"
---

An agent often needs more than one program to finish a job. It downloads a
report and works through it over several turns, edits a file in place, or
remembers how far it got. Without somewhere to keep that, each program
starts over, and whatever it must carry forward passes through the
model's context window.

This guide shows you how to let the programs in one session keep files
and state between them. The example keeps notes under `/notes`.
Substitute your paths.

## Start from an empty Blueprint

```sh
submilli blueprint init notes
```

```text
✓ created blueprint.yaml (name: notes)
```

## The filesystem

Each program gets its own filesystem, rooted at `/`. The Blueprint's
`vfs` line says what backs it and how long it lasts. It has no command, so
add it by hand, with `idle_timeout`:

```yaml title="blueprint.yaml (fragment)"
vfs: per_session
idle_timeout: 1h
```

Pick the mode by how long the files should last:

| Mode | The program's `/` is |
| --- | --- |
| `none` | Nothing. Every `submilli:fs` call fails |
| `ephemeral` (the default) | A scratch directory created for the run and deleted after it |
| `per_session` | A directory that lasts as long as the session |
| `named` | A volume the server's operator declared, kept across sessions and restarts, and shared with every Blueprint that names it |

Under `per_session`, files and session state both last as long as the
session. `idle_timeout` closes a session nobody has used for that long. The
default is 24 hours, and the CLI writes `1h` back as `'3600s'`.

A named volume can also sit beside the session's files, mounted at its own
path under `vfs.mounts`. The mount is read-only or read-write, and covers
the volume or the directory of the user the session is for. [Mount a shared
volume](/docs/server/mount-a-shared-volume) declares one on the
server and mounts it.

## What programs can do with it

Programs use the filesystem through `submilli:fs`. `submilli docs` prints
what the module can do:

```sh
submilli docs submilli:fs
```

```text
submilli:fs — Sandbox filesystem: read/write/list/stat/remove/exists/info.
…
function appendText(path: string, content: string): void;
function exists(path: string): boolean;
function list(path: string, recursive: boolean): Iterator<DirEntry>;
function mkdir(path: string, recursive: boolean): void;
function readText(path: string): string | null;
function remove(path: string, recursive: boolean): void;
function stat(path: string): Stat | null;
function writeText(path: string, content: string): void;
…
```

Under a Blueprint that grants nothing, a program can call none of it. The
module's operations fall under eight capabilities. List them with the
fields a filter can test:

```sh
submilli blueprint capability list submilli:fs
```

```text
submilli:fs
  fs.read — Read files and code workspace content (including search and ignore rules)
      fields: path: string, length: number, chunkSize: number, recursive: boolean
      example filter: path glob "*.csv"
  fs.write — Create, write, append, or apply code edits to files
      fields: path: string, length: number, max_bytes: number, diff: string
      example filter: path glob "/out/*"
  fs.stat — Inspect metadata (including code workspace discovery)
      fields: path: string, recursive: boolean
      example filter: path glob "/data/*"
  fs.list — List directory entries (including code search, glob and tree)
      fields: path: string, recursive: boolean
      example filter: path glob "/data/*"
  fs.mkdir — Create directories
      fields: path: string, recursive: boolean
      example filter: path glob "/tmp/*"
  fs.remove — Delete files or directories
      fields: path: string, recursive: boolean
      example filter: path glob "/tmp/*"
  fs.move — Move or rename a path
      fields: from: string, to: string
      example filter: to glob "/archive/*"
  fs.copy — Copy a path
      fields: from: string, to: string, recursive: boolean
      example filter: to glob "/backup/*"
```

## Grant the operations

The notes live under `/notes`. Let a program create that directory, write
and read inside it, and ask whether a path exists:

```sh
submilli blueprint capability add fs.mkdir --filter 'path == "/notes"'
submilli blueprint capability add fs.write --filter 'path glob "/notes/*"'
submilli blueprint capability add fs.read --filter 'path glob "/notes/*"'
submilli blueprint capability add fs.stat
```

```text
✓ added allow fs.mkdir (filter: path == "/notes") to caller 'main' in blueprint.yaml
  Create directories
  filter fields: path: string, recursive: boolean
✓ added allow fs.write (filter: path glob "/notes/*") to caller 'main' in blueprint.yaml
  Create, write, append, or apply code edits to files
  filter fields: path: string, length: number, max_bytes: number, diff: string
✓ added allow fs.read (filter: path glob "/notes/*") to caller 'main' in blueprint.yaml
  Read files and code workspace content (including search and ignore rules)
  filter fields: path: string, length: number, chunkSize: number, recursive: boolean
✓ added allow fs.stat to caller 'main' in blueprint.yaml
  Inspect metadata (including code workspace discovery)
  filter fields: path: string, recursive: boolean
```

Paths are checked after `.` and `..` are resolved, so a write to
`/notes/../secrets.md` is checked as `/secrets.md` and refused. In a
`glob`, `*` crosses `/`, so `path glob "/notes/*"` also matches
`/notes/2026/a.md`. Each operation has its own capability, so `fs.remove`,
`fs.move`, and `fs.copy` stay denied here.

## Run a program

Create `note.ts`. It adds a line to today's notes and returns the file:

```typescript title="note.ts"
import * as fs from "submilli:fs";

function main(): string {
    if (!fs.exists("/notes")) {
        fs.mkdir("/notes", false);
    }
    fs.appendText("/notes/today.md", "- call Northwind about the credit\n");
    return fs.readText("/notes/today.md") ?? "";
}
```

```sh
submilli run --blueprint blueprint.yaml note.ts
```

```text
- call Northwind about the credit
```

Run it again and the file still has one line. `submilli run` has no
session, so each run gets a fresh directory whatever `vfs` says. To stand
in for a session, point `--vfs` at a directory, and the runs share it:

:::note[Sessions live on the server]
Your application opens a session on `submilli-server` for one
conversation, and then runs each program inside it. There, `vfs: per_session`
gives all programs of that conversation the same directory, with no
flag. [Session state](#session-state) below opens one by hand.
:::

```sh
mkdir -p workspace
submilli run --blueprint blueprint.yaml --vfs ./workspace note.ts
submilli run --blueprint blueprint.yaml --vfs ./workspace note.ts
```

The second run returns both lines:

```text
- call Northwind about the credit
- call Northwind about the credit
```

A program that writes `/notes/../secrets.md` under this Blueprint gets a
`PermissionDeniedError` for `fs.write` before anything is written.

## Cap the size

If the filesystem is `ephemeral` or `per_session`, cap how much space its
files take:

```yaml title="blueprint.yaml (fragment)"
vfs:
  mode: per_session
  size_limit: 100MB
```

`size_limit` takes a byte count or a size such as `500KB`, `100MB`, or
`1GB`, and the CLI writes it back as bytes. Under `per_session` it covers
all the session's files. A write that would pass it is refused with a
`QuotaExceededError` the program can catch. With a `1KB` limit, writing 2,000
bytes gets:

```text
error: QuotaExceededError: fs.writeText /notes/big.md: the filesystem's size limit of 1024 bytes would be exceeded: 0 bytes are in use and this needs 2000 more
```

Deleting files frees the space. A named volume takes no `size_limit`
here. The operator sets one where the server declares the volume, and
that limit covers all sessions and Blueprints using it.

## Session state

Files are one way for a program to leave something for the next.
`submilli:session` is the other. It is a key-value store that lasts the
session, kept in memory, and checked against a type when read:

```sh
submilli docs submilli:session
```

```text
submilli:session — Session-scoped key-value state: get/has/set/remove/list.
…
function get<T>(key: string): T;
function has(key: string): boolean;
function set(key: string, value: unknown): void;
…
```

```sh
submilli blueprint capability list submilli:session
```

```text
submilli:session
  session.read — Read session state (get, has), and decide which keys a list may reveal
      fields: key: string
      example filter: key glob "triage/*"
  session.write — Store or overwrite a session value (set)
      fields: key: string
      example filter: key glob "triage/*"
  session.remove — Delete a session key
      fields: key: string
      example filter: key glob "triage/*"
  session.list — Enumerate session keys under a prefix
      fields: prefix: string
      example filter: prefix == "triage/"
```

Grant reading and writing:

```sh
submilli blueprint capability add session.read
submilli blueprint capability add session.write
```

```text
✓ added allow session.read to caller 'main' in blueprint.yaml
  Read session state (get, has), and decide which keys a list may reveal
  filter fields: key: string
✓ added allow session.write to caller 'main' in blueprint.yaml
  Store or overwrite a session value (set)
  filter fields: key: string
```

Create two programs. `remember.ts` saves where the agent got to, and
`recall.ts` reads it back:

```typescript title="remember.ts"
import * as session from "submilli:session";

function main(): string {
    session.set("progress", { reviewed: 3, next: "cus_initech" });
    return "saved";
}
```

```typescript title="recall.ts"
import * as session from "submilli:session";

interface Progress {
    reviewed: number;
    next: string;
}

function main(): string {
    const progress = session.get<Progress | null>("progress");
    if (progress === null) {
        return "nothing saved yet";
    }
    return `reviewed ${progress.reviewed}, next up ${progress.next}`;
}
```

The store exists only inside a session, which the application opens.
`submilli run` has none, so use `submilli-server`. [Run the
server](/docs/server/run-the-server) starts one and [Connect the
CLI](/docs/server/connect-the-cli) points the commands at it.
Register the Blueprint:

```sh
submilli server blueprint apply blueprint.yaml
```

```text
Added blueprint 'notes'
```

Open a session for it, as your application does when a conversation
starts. The command prints the session's id:

```sh
SESSION=$(submilli server session open --blueprint notes)
```

Run the first program inside it:

```sh
submilli server run-code --session "$SESSION" remember.ts
```

```text
saved
```

Then the second:

```sh
submilli server run-code --session "$SESSION" recall.ts
```

```text
reviewed 3, next up cus_initech
```

The second program found what the first saved. Files behave the same
here, and `note.ts` run twice in this session returns both lines with no
`--vfs`. Without `--session`, `run-code` opens a fresh session for each
run, and the second program would find nothing. Close the session when the
conversation ends:

```sh
submilli server session close "$SESSION"
```

```text
closed session 468eb254-bb7c-43df-bbb6-032d58896acc
```

[Connect a harness](/docs/tutorials/connect-a-harness) shows your
application opening sessions over HTTP the same way.

## The result

```yaml title="blueprint.yaml"
kind: blueprint
name: notes
idle_timeout: '3600s'
vfs:
  mode: per_session
  size_limit: 104857600
default: deny
permissions:
  main:
  - capability: fs.mkdir
    filter: path == "/notes"
    action: allow
  - capability: fs.write
    filter: path glob "/notes/*"
    action: allow
  - capability: fs.read
    filter: path glob "/notes/*"
    action: allow
  - capability: fs.stat
    action: allow
  - capability: session.read
    action: allow
  - capability: session.write
    action: allow
```
