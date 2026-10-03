---
title: "Allow Git"
description: "How to let a program clone a repository, find and change what it needs, and commit: set the identity and its token, grant the Git and file operations, and run it."
slug: blueprints/allow-git
sidebar:
  order: 4
---

Coding assistants write the code. A Submilli agent more often needs to
read it: an agent on the support line hears about a bug or a feature
request, and the fastest way to triage it is the repository itself, to
find where the feature lives and say how deep the change goes. And a
repository is not only for code. Notes and memory kept in Git get
history, diffs, and review for free, which is why some memory
frameworks store an agent's memory that way. In a program,
`submilli:git` does the repository work and `submilli:code` the finding
and editing, both inside the program's own filesystem, and the blueprint
decides which repository, which directory, and whether the program may
commit at all.

This guide shows you how to let a program clone a repository, search and
edit it, and commit. It starts from an empty blueprint: set the Git
identity and its token, grant the Git and file operations, and run a
program that does the work. The example clones GitHub's
Hello-World repository into `/repo`; substitute your remote and paths.

## Start from an empty blueprint

```sh
submilli blueprint init coder
```

```text
✓ created blueprint.yaml (name: coder)
```

## Set the identity

`submilli:git` is off until the blueprint gives it an identity, because
every commit it makes is authored by the blueprint, not the program. The
module speaks HTTPS only, not SSH, so the identity also carries the
username a remote will ask for:

```sh
submilli blueprint git set --name "Support Agent" --email agent@acme.example --username agent
```

```text
Configured Git in blueprint.yaml. Configure grants with `submilli blueprint capability add`.
```

```yaml title="blueprint.yaml (fragment)"
git:
  identity:
    name: Support Agent
    email: agent@acme.example
  username: agent
```

When a remote asks for credentials, Submilli sends the secret named
`GIT_TOKEN` with that username, and the program never sees it. Declare it
and put the value in the store; a public repository never asks, and the
token goes unused:

```sh
submilli blueprint secret add GIT_TOKEN --store git_token
submilli secret put git_token
```

```text
✓ declared secret 'GIT_TOKEN' (store: git_token) in blueprint.yaml
Value for 'git_token': [hidden]
Stored secret 'git_token'
```

If you want the session recorded in the author, a value may include a
variable: `--name 'Support Agent (${vars.customerId})'`, single-quoted so
the shell leaves it alone. `submilli blueprint git show` prints the
identity and whether `GIT_TOKEN` is declared, without the value; `git
remove` drops the block and leaves the secret and the rules in place.

## What programs can do with it

A program works on a repository through the `Repository` class of
`submilli:git`:

```sh
submilli docs submilli:git
```

```text
submilli:git — Capability-controlled VFS repositories: history, staging, commits, branches and HTTPS fetch.
…
class Repository {
  static clone(url: string, path: string, options?: null | { branch?: string }): Repository;
  static init(path: string, options?: null | { branch?: string }): Repository;
  static open(path: string): Repository;
  add(paths: string[]): void;
  commit(message: string): string;
  status(): { branch: string | null; clean: boolean; entries: { … }[] };
  log(options?: null | { limit?: number; offset?: number }): { commits: { … }[]; nextOffset: number | null };
  diff(options?: null | { from?: string; mode?: string; to?: string }): { binaryPaths: string[]; patch: string };
  fetch(remote?: string, branch?: string): { branches: string[] };
  pull(remote?: string, branch?: string): { current: string; previous: string };
  …
}
```

Git has four capabilities:

```sh
submilli blueprint capability list submilli:git
```

```text
submilli:git
  git.init — Create a local repository and its VFS directory
      fields: path: string
      example filter: path == "/repo"
  git.clone — Clone an HTTPS repository into a VFS directory
      fields: path: string, remoteName: string, remote: string, branch: string
      example filter: path == "/repo" and remote == "https://github.com/acme/project.git"
  git.fetch — Fetch or pull HTTPS remote branches into an existing repository
      fields: path: string, remoteName: string, remote: string, branch: string
      example filter: path == "/repo" and remote == "https://github.com/acme/project.git"
  git.commit — Commit staged changes with blueprint identity
      fields: path: string, branch: string
      example filter: path == "/repo" and branch == "main"
```

## The workspace tools

Finding and changing code is `submilli:code`, the workspace tools a
coding agent expects: numbered reads, search, tree, and anchored edits
that return a diff:

```sh
submilli docs submilli:code
```

```text
submilli:code — Workspace tools: numbered reads, search, glob, tree, anchored edits and unified diffs.
…
function read(path: string, offset?: number, limit?: number): { lines: { line: number; text: string }[]; path: string; truncated: boolean };
function search(pattern: string, options?: null | { caseSensitive?: boolean; context?: number; exclude?: string[]; include?: string[]; limit?: number; mode?: string; path?: string }): { … };
function tree(path: string, depth?: number): { entries: { depth: number; kind: string; modifiedAt: number; path: string }[]; truncated: boolean };
function edit(path: string, oldString: string, newString: string, replaceAll?: boolean, nearLine?: number): { changed: boolean; diagnostics: { … }[]; diff: string; success: boolean };
…
```

It has no capabilities of its own: reads use `fs.read`, navigation uses
`fs.list` and `fs.stat`, and edits use `fs.read` and `fs.write`.

## Grant the operations

Let the program clone one repository into `/repo` and commit there, and
read, list, and edit under it:

```sh
submilli blueprint capability add git.clone \
  --filter 'path == "/repo" and remote == "https://github.com/octocat/Hello-World.git"'
submilli blueprint capability add git.commit --filter 'path == "/repo"'
submilli blueprint capability add fs.read --filter 'path == "/repo" or path glob "/repo/*"'
submilli blueprint capability add fs.list --filter 'path == "/repo" or path glob "/repo/*"'
submilli blueprint capability add fs.write --filter 'path glob "/repo/*"'
submilli blueprint capability add fs.stat
```

```text
✓ added allow git.clone (filter: path == "/repo" and remote == "https://github.com/octocat/Hello-World.git") to caller 'main' in blueprint.yaml
  Clone an HTTPS repository into a VFS directory
  filter fields: path: string, remoteName: string, remote: string, branch: string
✓ added allow git.commit (filter: path == "/repo") to caller 'main' in blueprint.yaml
  Commit staged changes with blueprint identity
  filter fields: path: string, branch: string
✓ added allow fs.read (filter: path == "/repo" or path glob "/repo/*") to caller 'main' in blueprint.yaml
  Read files and code workspace content (including search and ignore rules)
  filter fields: path: string, length: number, chunkSize: number, recursive: boolean
✓ added allow fs.list (filter: path == "/repo" or path glob "/repo/*") to caller 'main' in blueprint.yaml
  List directory entries (including code search, glob and tree)
  filter fields: path: string, recursive: boolean
✓ added allow fs.write (filter: path glob "/repo/*") to caller 'main' in blueprint.yaml
  Create, write, append, or apply code edits to files
  filter fields: path: string, length: number, max_bytes: number, diff: string
✓ added allow fs.stat to caller 'main' in blueprint.yaml
  Inspect metadata (including code workspace discovery)
  filter fields: path: string, recursive: boolean
```

Each operation needs only its own rule: `git.clone` creates and fills
`/repo` with no `fs` rule, and reading history, staging, creating or
switching branches, and adding remotes need no rule at all. `fs.stat` is
left unfiltered: before a search, the workspace tools look for ignore
files up to the root, and `stat` reveals only metadata. `remote` is the
full HTTPS URL; refer to [permissions](/docs/reference/permissions)
for how it is normalized and for filtering on `branch`.

If the program should fetch or pull later, grant `git.fetch` for the same
path:

```sh
submilli blueprint capability add git.fetch --filter 'path == "/repo"'
```

```text
✓ added allow git.fetch (filter: path == "/repo") to caller 'main' in blueprint.yaml
  Fetch or pull HTTPS remote branches into an existing repository
  filter fields: path: string, remoteName: string, remote: string, branch: string
```

## Run a program

Create `review.ts`. It clones the repository, searches it for the text a
customer mentioned, and records what it found in a commit:

```typescript title="review.ts"
import { Repository } from "submilli:git";
import * as code from "submilli:code";

function main(): string {
    const repo = Repository.clone("https://github.com/octocat/Hello-World.git", "/repo", { branch: "master" });
    const hits = code.search("Hello", { path: "/repo" }).matches.length;
    const edit = code.edit("/repo/README", "Hello World!", "Hello World! Reviewed by the support agent.");
    repo.add(["."]);
    const commit = repo.commit("Note the review");
    return `${hits} match, committed ${commit.slice(0, 7)}\n${edit.diff}`;
}
```

```sh
submilli run --blueprint blueprint.yaml review.ts
```

```text
1 match, committed 3324dc2
--- a
+++ b
@@ -1,1 +1,1 @@
-Hello World!
+Hello World! Reviewed by the support agent.
```

The repository lives in the program's filesystem, so how long the commit
lasts is the `vfs` mode's decision: refer to [Keep files and
state](/docs/blueprints/keep-files-and-state).

## The result

```yaml title="blueprint.yaml"
kind: blueprint
name: coder
secrets:
  GIT_TOKEN:
    store: git_token
git:
  identity:
    name: Support Agent
    email: agent@acme.example
  username: agent
default: deny
permissions:
  main:
  - capability: git.clone
    filter: path == "/repo" and remote == "https://github.com/octocat/Hello-World.git"
    action: allow
  - capability: git.commit
    filter: path == "/repo"
    action: allow
  - capability: fs.read
    filter: path == "/repo" or path glob "/repo/*"
    action: allow
  - capability: fs.list
    filter: path == "/repo" or path glob "/repo/*"
    action: allow
  - capability: fs.write
    filter: path glob "/repo/*"
    action: allow
  - capability: fs.stat
    action: allow
```
