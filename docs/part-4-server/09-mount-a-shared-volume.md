---
title: "Mount a shared volume"
description: "How to give programs a directory that outlives sessions and is shared across blueprints: declare a named volume on the server, mount it in a blueprint read-only or read-write, use it from a program, and know where its files live."
slug: server/mount-a-shared-volume
sidebar:
  order: 9
---

A session's files end with the session. Some things an agent works with
must outlive it, such as the notes it keeps between conversations, a
handbook every agent should be able to read, or a workspace two blueprints
share. For those there is the named volume. The server owns and declares
it, a blueprint mounts it at a path it chooses, read-only or read-write,
and every session and blueprint that mounts it sees it.

This guide shows you how to mount a shared volume. The example gives
Acme's support agent a memory it keeps across conversations and a
read-only company handbook. Substitute your volumes.

## Declare it on the server

Volumes are declared in the server's config file, under `volumes:`, and
nowhere else. A blueprint can only name them. Each has a kind, a size
limit, and the most access any blueprint may have:

```yaml title="server.yaml (fragment)"
volumes:
  project-memory:
    kind: managed-local
    size_limit: 100MB
  company-handbook:
    kind: local-path
    path: /srv/company-handbook
    access: read_only
    size_limit: unlimited
```

| Kind | Where its files live |
| --- | --- |
| `managed-local` | A directory the server creates the first time a program uses the volume, under `volume_dir` (`$SUBMILLI_HOME/server/volumes/<name>` by default) |
| `local-path` | The directory at `path`, which you created and the server never creates or deletes |

`size_limit` is required, a size such as `100MB` or `10GB`, or
`unlimited`. One limit covers the volume however many sessions and
blueprints use it, so a blueprint can't get a second allowance by
mounting the same volume twice. `access` is `read_write` unless you
write `read_only`. The server reads the declarations at startup, so
restart it after a change.

## Mount it in a blueprint

A blueprint mounts a volume at a path under `vfs.mounts`, beside its
root, which stays `ephemeral` or `per_session` as [Keep files and
state](/docs/blueprints/keep-files-and-state) describes. The
support agent keeps its session files at `/`, its memory at `/memory`,
and reads the handbook at `/handbook`:

```yaml title="blueprint.yaml (fragment)"
vfs:
  mode: per_session
  mounts:
    /memory:
      mode: named
      volume: project-memory
      access: read_write
    /handbook:
      mode: named
      volume: company-handbook
      access: read_only
```

`access` can only narrow what the server declared. Leave it out to take
the server's setting. A volume can also be the root itself, `vfs:
{mode: named, volume: project-memory}`, for a blueprint whose
filesystem should outlive the session. The blueprint's filesystem rules
apply under a mount as anywhere else. The example allows `fs.read`,
`fs.write`, `fs.stat`, and `fs.list` to `main`.

Registration checks the mounts against the declarations:

```sh
submilli server blueprint apply blueprint.yaml
```

```text
Added blueprint 'support'
```

A volume the server doesn't declare, or more access than it allows, is
refused with the fix:

```text
error: volume 'team-memory' is not declared on this server; declared volumes: company-handbook, project-memory
```

```text
error: volume 'company-handbook' is read_only on this server; drop `access: read_write` (or write `access: read_only`), or ask the operator to declare it read_write
```

A blueprint written for the earlier `persistent` mode is refused too,
with the edit that replaces it:

```text
error: blueprint parse error: vfs.mode: vfs mode `persistent` was removed: write `mode: named` and keep the `volume:` line (`vfs: {mode: named, volume: <name>}`). A named volume keeps its files across calls, sessions and restarts as before; leave `access:` out to keep the access the server declares for it at line 4 column 9
```

## Use it from a program

A program reads and writes under a mount with the ordinary `submilli:fs`
calls. This one reads the refund policy and adds a line to the agent's
notes:

```typescript title="remember.ts"
import fs from "submilli:fs";

function main(): string {
    const policy = fs.readText("/handbook/refunds.md") ?? "";
    fs.appendText("/memory/notes.md", "Northwind asked about refunds; pointed them to the policy.\n");
    const notes = fs.readText("/memory/notes.md") ?? "";
    return `policy: ${policy.split("\n")[2]}\nnotes so far: ${notes.split("\n").length - 1}`;
}
```

Run it twice. Each `run-code` opens a new session, so the session's
files start empty each time, and the notes under `/memory` don't:

```sh
submilli server run-code remember.ts --blueprint support
submilli server run-code remember.ts --blueprint support
```

```text
policy: A goodwill credit needs a ticket number and may not exceed $50 without a lead's approval.
notes so far: 1
policy: A goodwill credit needs a ticket number and may not exceed $50 without a lead's approval.
notes so far: 2
```

A write under a read-only mount is refused with a `PermissionDeniedError`
the program can catch, before the file is touched:

```typescript title="scribble.ts"
import fs from "submilli:fs";

function main(): string {
    fs.writeText("/handbook/refunds.md", "No refunds.");
    return "rewrote the handbook";
}
```

```text
error: PermissionDeniedError: permission denied: caller=main capability=fs.write: /handbook/refunds.md is in the volume mounted read-only at /handbook. This volume cannot be written from this blueprint; write under a writable path instead (fs.info() lists each mount and its access).
  fields: caller = "main", capability = "fs.write", reason = "/handbook/refunds.md is in the volume mounted read-only at /handbook"
  at main (<execute>:4:42)  [thrown here]
3 | function main(): string {
4 |     fs.writeText("/handbook/refunds.md", "No refunds.");
  |                                          ^
5 |     return "rewrote the handbook";
```

`fs.info()` reports the root's mode, access, and
size limit, and each mount's path, volume, access, and limit, with `-1`
where no limit applies:

```typescript title="info.ts"
import fs from "submilli:fs";

function main(): string {
    const info = fs.info();
    const lines = [`/ ${info.mode} ${info.access} limit=${info.sizeLimit.toString()}`];
    for (const mount of info.mounts) {
        lines.push(`${mount.path} ${mount.volume} ${mount.access} limit=${mount.sizeLimit.toString()}`);
    }
    return lines.join("\n");
}
```

```text
/ per_session read_write limit=-1
/handbook company-handbook read_only limit=-1
/memory project-memory read_write limit=104857600
```

Mount points can't be moved or removed, and one mount can't sit inside
another. The same volume may be mounted at two paths, under one limit. A move
between the root and a mount, or between two mounts, copies and then
removes, so it isn't atomic. A named volume needs a server to resolve
it, so `submilli run` refuses a blueprint that mounts one and says what
to do instead:

```text
error: blueprint.yaml: blueprint 'support' uses named volume 'company-handbook' at `/handbook`; named volumes are declared in a server config, so run it on `submilli-server`, or drop the volume for local runs (use `--vfs <dir>` to give the program a directory)
```

## Give each user their own directory

One memory every session shares suits a handbook but not notes about
customers. The agent serving Northwind shouldn't read what it noted
about Initech. Mount only that customer's directory of the volume,
chosen by the variable the application binds for the session:

```yaml title="blueprint.yaml (fragment)"
variables:
  customerId:
    required: true

vfs:
  mode: per_session
  cwd: /memory
  mounts:
    /memory:
      mode: named
      volume: project-memory
      subPath: customers/${vars.customerId}
```

`subPath` names the directory inside the volume to mount. The program sees it as `/memory` whichever customer the session is
for, and nothing above it, so neither the program nor a package it
calls can reach another customer's notes, and no rule has to name a
customer. `${vars.customerId}` must be a whole part of the path, and a
writable mount creates the directory the first time it is used. `cwd`
is where relative paths start, so this program's `notes.md` is
`/memory/notes.md`:

```typescript title="remember.ts"
import fs from "submilli:fs";

function main(): string {
    fs.appendText("notes.md", "Asked about refunds; pointed them to the policy.\n");
    const notes = fs.readText("notes.md") ?? "";
    return `${fs.cwd()}/notes.md: ${notes.split("\n").length - 1} lines`;
}
```

Run it twice for Northwind, then once for Initech:

```sh
submilli server run-code remember.ts --blueprint support --var customerId=cus_northwind
submilli server run-code remember.ts --blueprint support --var customerId=cus_northwind
submilli server run-code remember.ts --blueprint support --var customerId=cus_initech
```

```text
/memory/notes.md: 1 lines
/memory/notes.md: 2 lines
/memory/notes.md: 1 lines
```

Notice the third run. The path is the same, and Initech's notes start
at one line. The volume holds a directory per customer:

```text
project-memory/customers/cus_initech/notes.md
project-memory/customers/cus_northwind/notes.md
```

A session bound to a value that isn't one directory name, such as `..`,
is refused before any program runs:

```text
invalid vfs config: each path component must be nonempty and contain no separator, NUL, '.' or '..' component
```

`cwd` is a convenience. `..` and absolute paths still
reach the rest of what the blueprint mounts. The boundary is `subPath`.

## Where the files live

A managed volume's files are under `volume_dir`, in a directory named
after the volume, and a `local-path` volume's are where you put them:

```text
~/.submilli/server/volumes/project-memory/notes.md
```

Nothing the server does deletes a volume's files. Ending a session or
removing a blueprint leaves them, and removing the declaration from the
config only stops blueprints from naming the volume. Declare it again
and its files are still there. To remove a managed volume for good,
delete its directory under `volume_dir` while the server is stopped.
Back up `volume_dir` with the sessions, as [Run the
server](/docs/server/run-the-server) says. As with a session's files,
nothing can rebuild what programs kept there.
