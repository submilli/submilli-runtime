---
title: "Standard library"
description: "Every submilli: module: what gates it, who may import it, the rules its functions share, Git's operations and limits, and each module's functions and types."
slug: reference/standard-library
sidebar:
  order: 6
authorship:
  label: ai-assisted
  confirmed: true
  contentHash: "2c3eb285d02288236f6512c2086fdaac71865f5b49c5179de7b54b8c291758fd"
  confirmedAt: "2026-10-05T17:36:35.000Z"
---

This page describes what each `submilli:` module does, the capabilities
that gate it, and the rules its functions share, followed by every module's
functions and types. The globals that need no import, such as `Map`, `JSON`,
and `Temporal`, are on [Built-ins](/docs/reference/built-ins).

## Modules

| Module | What it does | Gated by |
| --- | --- | --- |
| `submilli:fs` | The program's filesystem: read, write, list, stat, move, copy, remove | `fs.*` |
| `submilli:http` | HTTP requests, and downloads into the filesystem | `http.<method>`, `http.download` |
| `submilli:llm` | Model calls: `call`, `batch`, `models` | `llm.call` |
| `submilli:embedding` | Text embeddings: `embed`, `models` | `embedding.embed` |
| `submilli:session` | Key-value state that lasts for the session | `session.*` |
| `submilli:git` | Repositories in the filesystem: history, commits, branches, clone, fetch, pull | `git.init`, `git.clone`, `git.fetch`, `git.commit` |
| `submilli:code` | Numbered reads, search, glob, tree, anchored edits, and diffs over the program's files | `fs.read`, `fs.list`, `fs.stat`, `fs.write` |
| `submilli:crypto` | SHA-256, SHA-512, HMAC-SHA-256, random bytes | Nothing |
| `submilli:url` | URL and query-string parsing and building | Nothing |
| `submilli:uuid` | UUID v4 and v7 | Nothing |
| `submilli:secrets` | A declared secret's value, for Packages | `secrets.get` |
| `submilli:security` | `check`, for Packages | The capability it names |
| `submilli:test` | `label` and `expectException`, for test files | Nothing |

`submilli:git` is available under a Blueprint with a `git` block. Each
capability's fields are on [Permissions](/docs/reference/permissions).

A module is imported by its name, as a namespace, a default, or by name:

```typescript
import * as http from "submilli:http";
import fs from "submilli:fs";
import { sha256 } from "submilli:crypto";
```

`submilli search` and `submilli docs` list the modules a program may import.
With `--blueprint`, they list only those the Blueprint lets it use.

## Rules for every module

- Calls are synchronous. Every function returns its result directly.
  `llm.batch` sends its prompts concurrently and returns when all are done.
- Paths are the program's own. `submilli:fs`, `submilli:code`,
  `submilli:git`, and `http.download` take paths in the program's
  filesystem, never the host's. A relative path resolves against the working
  directory, `fs.cwd()`.
- A denied call throws a catchable `PermissionDeniedError`. A filesystem
  or token budget a call would pass throws `QuotaExceededError`
  ([Errors and limits](/docs/reference/errors-and-limits)).

## HTTP requests

- HTTPS, unless the Blueprint sets `allow_insecure_http: true`.
- Redirects, up to 10, are each checked against the caller's rules before
  they are sent. A redirect to another origin drops the credential headers.
- A request times out after 30 seconds and a download after 60. A
  response body may be up to 50 MB, and a download up to its `maxBytes`.
- On a server, hosts that resolve to private addresses are refused unless the
  server allows them ([Server settings](/docs/reference/server-settings)).

## Workspace tools

`submilli:code` works through the filesystem capabilities:

| Functions | Capabilities checked |
| --- | --- |
| `read`, `diffFiles` | `fs.read` on each file |
| `search`, `glob`, `tree` | `fs.list`, `fs.stat`, and `fs.read` for files searched and for ignore files |
| `edit`, `insertAt`, `applyPatch` | `fs.read`, then `fs.write` with `path`, `length` (the new size) and `diff` (the change as a unified diff) |
| `diffText` | None |


Traversal honors `.gitignore` and `.ignore` files and skips hidden entries.
An edit that fails writes nothing. One that succeeds replaces the file
atomically and returns the change as a unified diff.

## Git repositories

`submilli:git` works on Git repositories in the program's filesystem. The
Blueprint's `git` block sets the identity that authors every commit and the
`username` that pairs with the `GIT_TOKEN` secret for private remotes
([Allow Git](/docs/blueprints/allow-git)). `Repository.init`,
`Repository.clone`, and `Repository.open` return a `Repository`:

| Method | Capability | Behavior |
| --- | --- | --- |
| `Repository.init` | `git.init` | Creates the directory and refuses an existing repository |
| `Repository.clone` | `git.clone` | Needs no `git.init`, `git.fetch` or `fs.*` rule |
| `Repository.open`, constructor | None | Opens `path`'s `.git` directory |
| `status`, `log`, `diff`, `show`, `branches`, `remotes` | None | Read only |
| `add` | None | At most 10,000 paths per call, `.` selects the working tree, and deletions are staged |
| `commit` | `git.commit` (`branch` is the current branch) | Refuses an empty message and an empty commit |
| `createBranch` | None | Never overwrites an existing branch |
| `switchBranch` | None | Requires a clean working tree, untracked files included |
| `addRemote`, `setRemoteUrl` | None | HTTPS URLs only |
| `fetch` | `git.fetch` | Defaults to `origin`. With no branch, fetches every remote branch and checks each |
| `pull` | `git.fetch` | Fetches and fast-forwards the current branch. Refuses divergence and a dirty working tree |


A revision is `HEAD`, a branch or tag name, a full ref, or a full commit ID.
Remotes are `https://` URLs. Repositories created by standard Git work as
they are. Advanced layouts, such as linked worktrees, submodules, partial
clones, and SHA-256 object format, aren't supported.

| Limit | Value |
| --- | --- |
| Time per operation | 60 seconds |
| Directory nesting | 64 levels |
| `log` page | Up to 1,000 commits, 50 by default |
| Memory | Counts against `max_execution_memory` |
| Files | Count against the filesystem's `size_limit` |

An operation that passes a limit throws and leaves the repository as it was.

## Model calls

`submilli:llm` sends prompts to the models the Blueprint declares
([Allow model calls](/docs/blueprints/allow-model-calls)):

| Form | Returns |
| --- | --- |
| `call(model, prompt)` | One `Completion` |
| `call<T>(model, prompt)` | A `T`, checked field by field against the response |
| `batch(model, prompts)` | A `Completion[]`, `result[i]` for `prompts[i]` |
| `batch<T[]>(model, prompts)` | A `T[]`. One non-conforming element throws for the whole batch |
| `models()` | The `Model`s this caller may call |


- A `Completion` has `ok`, `true` when the model stopped naturally, and
  `text`. A truncated completion is `ok: false` with a `reason`.
- The typed form sends a JSON Schema for `T` with the request and checks the
  response against it. A response that doesn't match throws `TypeError`.
- A batch takes up to 128 prompts, each up to 256 KB. A call's tokens are
  reserved against the run's and the server's budgets before it is sent.

## Embeddings

`submilli:embedding` turns texts into vectors through the aliases the
Blueprint declares ([Blueprint file](/docs/reference/blueprint-file#embedding)):

| Form | Returns |
| --- | --- |
| `embed(model, texts, purpose)` | One `Embeddings`: `vector(i)` is the vector of `texts[i]` |
| `models()` | The `EmbeddingModel`s this caller may use |

- `purpose` is `"query"` for text you search with and `"document"` for text
  you search over. A provider or model with no notion of purpose ignores it:
  OpenAI always, and Hugging Face unless the alias sets
  `query_prompt_name` or `document_prompt_name`. Elsewhere it is sent as
  Voyage `input_type`; Jina `task` (`retrieval.query`, `retrieval.passage`);
  Google `taskType`, or for `gemini-embedding-2` a text prefix the model reads.
- A call returns every vector or throws; nothing is truncated, and there are
  no per-text outcomes. An empty `texts`, more than 128 texts, more than
  2 MiB of text, or a text longer than the alias's `maxInputBytes` throws a
  `RangeError` before anything is sent.
- Every vector has unit length. `vector(i)` returns a `number[]` and `bytes(i)`
  returns `dimensions * 4` bytes of little-endian 32-bit floats; an `i` that
  isn't an integer below `count` throws a `RangeError`.
- The vectors live in the runtime at 4 bytes per number and count against
  `max_execution_memory` until the program drops the result.
- `identity` is opaque: compare the whole string, and don't mix vectors whose
  identities differ. It covers the Blueprint's configuration of the alias and
  can't see a vendor changing a model behind an unchanged name (a risk for
  OpenAI and shared Hugging Face hosting), or a model redeployed at the same
  dedicated Hugging Face endpoint unless the alias's `model` changes. A
  `"query"` and a `"document"` result from one alias share an identity.
  A different `base_url` doesn't change it, except a dedicated Hugging Face
  endpoint's host.
- Failures: `QuotaExceededError` for a budget or the request limit,
  `TypeError` for an unusable provider response, and an `Error` with a reason
  of `rate-limited`, `request-rejected`, `provider-unavailable`, `transport`,
  `timeout`, or `blocked-by-network-policy` for other provider failures. No
  error quotes a text or a vector.

## Modules for Packages and tests

`secrets.get(name)` returns a declared secret's value to a Package. From the
program itself, `main`, it always throws, so a secret's value never reaches
generated code.

`check(capability, context)` from `submilli:security` asks the Blueprint
whether the Package's caller may perform `capability` with the fields in
`context`, and throws `PermissionDeniedError` if not
([Export a function](/docs/packages/export-a-function)).

`label(description)` from `submilli:test` starts a named test, and
`expectException(fn, errorType?)` checks that `fn` throws
([Write tests](/docs/packages/write-tests)).

<!-- generated:stdlib -->

## `submilli:code`

Workspace tools: numbered reads, search, glob, tree, anchored edits and unified diffs.

| Function | Capability | Description |
| --- | --- | --- |
| `applyPatch(path: string, patch: string): { changed: boolean; diagnostics: { hunk: number; line: number; message: string }[]; diff: string; success: boolean }` |  | Apply a single-file unified patch by unique context, ignoring header positions. |
| `diffFiles(a: string, b: string): string` |  | Unified diff of two strict UTF-8 workspace files. |
| `diffText(a: string, b: string): string` |  | Pure UTF-16 text comparison; unified diff with three context lines. |
| `edit(path: string, oldString: string, newString: string, replaceAll?: boolean, nearLine?: number): { changed: boolean; diagnostics: { hunk: number; line: number; message: string }[]; diff: string; success: boolean }` |  | Replace a unique exact anchor; replaceAll selects every occurrence. |
| `glob(pattern: string): { entries: { depth: number; kind: string; modifiedAt: number; path: string }[]; truncated: boolean }` |  | Match file paths relative to /; newest modification first, path breaks ties. |
| `insertAt(path: string, line: number, text: string): { changed: boolean; diagnostics: { hunk: number; line: number; message: string }[]; diff: string; success: boolean }` |  | Insert text literally before a one-based line; one past the last line appends. |
| `read(path: string, offset?: number, limit?: number): { lines: { line: number; text: string }[]; path: string; truncated: boolean }` |  | Numbered UTF-8 lines; one-based offset, default 200 lines. |
| `search(pattern: string, options?: { caseSensitive?: boolean; context?: number; exclude?: string[]; include?: string[]; limit?: number; mode?: string; path?: string }): { counts: { count: number; path: string }[]; files: string[]; matches: { after: { line: number; text: string }[]; before: { line: number; text: string }[]; line: number; path: string; text: string }[]; truncated: boolean }` |  | Regex search, ignoring hidden and ignored paths. |
| `tree(path: string, depth?: number): { entries: { depth: number; kind: string; modifiedAt: number; path: string }[]; truncated: boolean }` |  | Ignored/hidden paths omitted; symlinks listed but never traversed. |

## `submilli:crypto`

Hashing, HMAC, and random bytes.

| Function | Capability | Description |
| --- | --- | --- |
| `hmacSha256(key: Uint8Array, message: string \| Uint8Array): Uint8Array` |  | HMAC-SHA-256 of `message` under `key`. |
| `randomBytes(length: number): Uint8Array` |  | Cryptographically secure random bytes from the OS entropy source. |
| `sha256(input: string \| Uint8Array): Uint8Array` |  | SHA-256 digest of `input`. |
| `sha512(input: string \| Uint8Array): Uint8Array` |  | SHA-512 digest of `input`. |
| `timingSafeEqual(a: Uint8Array, b: Uint8Array): boolean` |  | Constant-time byte-array equality, suitable for comparing HMAC tags. |

## `submilli:embedding`

Remote text embeddings: embed batches into sealed vectors, and models() to discover aliases.

| Function | Capability | Description |
| --- | --- | --- |
| `embed(model: string, texts: string[], purpose: "document" \| "query"): Embeddings` | `embedding.embed { model, input_count: $texts.length }` | Embed `texts` with the alias `model` and return one sealed `Embeddings`: `result.vector(i)` is the vector of `texts[i]`, in input order. |
| `models(): EmbeddingModel[]` | `embedding.embed { model, input_count: 0 }` | The embedding aliases this runtime serves and this caller may use. |

### `EmbeddingModel`

One alias this caller may embed with.

| Member | Description |
| --- | --- |
| `readonly description: string \| undefined` | Operator-authored deployment intent, or `undefined` when none was declared. |
| `readonly dimensions: number` | The length of every vector this alias returns. |
| `readonly identity: string` | The embedding-space identity results from this alias carry. |
| `readonly maxInputBytes: number` | The UTF-8 byte length above which one text is refused before sending. |
| `readonly maxInputTokens: number \| undefined` | The alias's input limit in tokens, or `undefined` when none is known. |
| `readonly name: string` | The alias name, exactly as `embed` expects it. |

### `Embeddings`

The sealed result of `embed`: vectors held by the runtime at 4 bytes per number, in input order, labeled with the embedding-space `identity`.

| Member | Description |
| --- | --- |
| `readonly count: number` | The number of vectors, equal to the number of texts embedded. |
| `readonly dimensions: number` | The length of every vector. |
| `readonly identity: string` | The embedding-space identity of every vector here. |
| `readonly inputTokens: number \| undefined` | Input tokens the provider reported for the whole call, or `undefined` when it reported none. |
| `readonly model: string` | The alias these vectors were embedded with. |
| `bytes(index: number): Uint8Array` | The vector of `texts[index]` as little-endian 32-bit floats: `dimensions * 4` bytes, the compact form for storing a vector. |
| `vector(index: number): number[]` | The vector of `texts[index]` as a `number[]` of `dimensions` numbers. |

## `submilli:fs`

Sandbox filesystem: read/write/list/stat/remove/exists/info.

| Function | Capability | Description |
| --- | --- | --- |
| `append(path: string, content: Uint8Array): void` | `fs.write { path, length: number }` | Append `content` to `path`. |
| `appendText(path: string, content: string): void` | `fs.write { path, length: number }` | UTF-8 append. |
| `bytes(path: string, chunkSize: number): Iterator<Uint8Array>` | `fs.read { path, chunkSize }` | Stream the file at `path` as a constant-memory byte iterator. |
| `copy(from: string, to: string, recursive: boolean): void` | `fs.copy { from, to, recursive }` | Copy `from` to `to`. |
| `cwd(): string` |  | The absolute guest working directory used by relative paths. |
| `exists(path: string): boolean` | `fs.stat { path }` | Returns `true` iff `path` resolves to a filesystem entry under the VFS root. |
| `info(): Info` |  | The active VFS configuration: the root's `mode` (`"none"` / `"ephemeral"` / `"per_session"` / `"named"`), `access`, `volume` and `sizeLimit` (the cap on the bytes its files may hold, or `-1` when none applies), plus `mounts`, the named volumes grafted below the root. |
| `lines(path: string): Iterator<string>` | `fs.read { path }` | Stream the file at `path` as a constant-memory line iterator. |
| `list(path: string, recursive: boolean): Iterator<DirEntry>` | `fs.list { path, recursive }` | List the children of the directory at `path` as a constant-memory iterator of `DirEntry`. |
| `maxReadSize(): number` |  | Maximum number of bytes `read` / `readText` will load whole; past this they return `undefined`. |
| `mkdir(path: string, recursive: boolean): void` | `fs.mkdir { path, recursive }` | Create the directory at `path`. |
| `move(from: string, to: string): void` | `fs.move { from, to }` | Move / rename. |
| `peek(path: string): Peek` | `fs.stat { path }` | Quick file-only inspection: preview (first ~256 bytes decoded UTF-8 lossy), detected `encoding` (`utf-8` / `utf-16le` / `utf-16be` / `latin1`), `lineEnding` (`lf` / `crlf`), and total `size`. |
| `read(path: string): Uint8Array \| undefined` | `fs.read { path }` | Read the whole file as a `Uint8Array`. |
| `readBytes(path: string, offset: number, length: number): Uint8Array` | `fs.read { path, length }` | Random-access byte-range read. |
| `readText(path: string): string \| undefined` | `fs.read { path }` | Read the whole file as a UTF-8 string. |
| `remove(path: string, recursive: boolean): void` | `fs.remove { path, recursive }` | Remove the entry at `path`. |
| `size(path: string): number` | `fs.stat { path }` | File size in bytes. |
| `stat(path: string): Stat \| undefined` | `fs.stat { path }` | Metadata about an entry under the VFS root. |
| `write(path: string, content: Uint8Array): void` | `fs.write { path, length: number }` | Write `content` to `path` atomically (temp-file + rename). |
| `writeText(path: string, content: string): void` | `fs.write { path, length: number }` | UTF-8 atomic write. |
| `writer(path: string): FileWriter` | `fs.write { path }` | Open `path` as an append-only `FileWriter`. |

### `DirEntry`

Directory entry yielded by `list(path, recursive)`.

| Member | Description |
| --- | --- |
| `readonly kind: string` | `"file"` / `"directory"` / `"symlink"` / `"other"`. |
| `readonly name: string` | Basename of the entry (no parent path). |
| `readonly path: string` | Full guest-visible path under the VFS root, with a leading `/`. |
| `readonly size: number` | File size in bytes; `0` for non-files. |

### `FileWriter`

Append-only writer returned by `writer(path)`.

| Member | Description |
| --- | --- |
| `close(): void` | Flush the internal buffer, fsync the temp file, and atomically rename it over the destination. |
| `writeBytes(bytes: Uint8Array): void` | Append `bytes` to the buffer. |
| `writeLine(line: string): void` | Append `line` followed by `\n` (LF). |

### `Info`

VFS configuration returned by `info()`: the root's `mode`, `access`, `volume` and `sizeLimit` (`-1` when no limit applies), plus its `mounts`.

| Member | Description |
| --- | --- |
| `readonly access: string` | `"read_write"`, or `"read_only"` when every write to the root throws a `PermissionDeniedError`. |
| `readonly mode: string` | The root's mode: `"none"` / `"ephemeral"` / `"per_session"` / `"named"`. |
| `readonly mounts: MountInfo[]` | The named volumes mounted below the root, sorted by path; empty when there are none. |
| `readonly sizeLimit: number` | The most bytes the files in the root may hold, or `-1` when no limit applies. |
| `readonly volume: string` | The named volume backing the root under `mode: "named"`; `""` otherwise, and for a directory `submilli run --vfs` exposes. |

### `MountInfo`

One volume mounted below the VFS root, as `info().mounts` lists it.

| Member | Description |
| --- | --- |
| `readonly access: string` | `"read_write"`, or `"read_only"` when every write under `path` throws a `PermissionDeniedError`. |
| `readonly mode: string` | Always `"named"`: mounts are named volumes the server declares. |
| `readonly path: string` | The guest path the volume is mounted at, such as `/memory`. |
| `readonly sizeLimit: number` | The most bytes the volume may hold, shared with everyone who mounts it, or `-1` when no limit applies. |
| `readonly volume: string` | The volume's name. |

### `Peek`

Lightweight file-only inspection — preview + transport metadata.

| Member | Description |
| --- | --- |
| `readonly encoding: string` | Detected text encoding: `"utf-8"` / `"utf-16le"` / `"utf-16be"` / `"latin1"`. |
| `readonly lineEnding: string` | `"crlf"` if a CRLF sequence appears in the preview; `"lf"` otherwise (including files with no line terminators yet). |
| `readonly preview: string` | First ~256 bytes of the file decoded as UTF-8 lossy. |
| `readonly size: number` | Total file size in bytes — same value as `size(path)`. |

### `Stat`

Filesystem metadata returned by `stat(path)`.

| Member | Description |
| --- | --- |
| `readonly kind: string` | One of `"file"` / `"directory"` / `"symlink"` / `"other"`. |
| `readonly modifiedAt: number` | Last-modified timestamp in milliseconds since the Unix epoch. |
| `readonly size: number` | File size in bytes. |

## `submilli:git`

Capability-controlled VFS repositories: history, staging, commits, branches and HTTPS fetch.

### `Repository`

A repository under the VFS root.

| Member | Capability | Description |
| --- | --- | --- |
| `static clone(url: string, path: string, options?: { branch?: string }): Repository` | `git.clone { path, remote: $url, remoteName: "origin", branch: string }` | Clone HTTPS into an empty VFS directory, following remote HEAD unless branch is specified. |
| `static init(path: string, options?: { branch?: string }): Repository` | `git.init { path }` | Initialize a repository, default branch main. |
| `static open(path: string): Repository` |  | Open an ordinary repository under the VFS root. |
| `constructor(path: string)` |  |  |
| `add(paths: string[]): void` |  | Stage explicit files or directories, including deletions. |
| `addRemote(name: string, url: string): void` |  | Add a named HTTPS remote, for example origin or upstream. |
| `branches(): { current: boolean; id: string; name: string }[]` |  | List local branches. |
| `commit(message: string): string` | `git.commit { path: string, branch: string }` | Commit staged changes with Blueprint identity and return the commit ID. |
| `createBranch(name: string, start?: string): void` |  | Create a branch without overwriting an existing branch. |
| `diff(options?: { from?: string; mode?: string; to?: string }): { binaryPaths: string[]; patch: string }` |  | Compare working (default), staged, or refs (requires from and to). |
| `fetch(remote?: string, branch?: string): { branches: string[] }` | `git.fetch { path: string, remoteName: $remote, remote: string, branch: string }` | Fetch remote-tracking branches. |
| `log(options?: { limit?: number; offset?: number }): { commits: { authorEmail: string; authorName: string; id: string; message: string }[]; nextOffset?: number }` |  | Read history, default 50 commits; limit 1..1000, nonnegative integer offset. |
| `pull(remote?: string, branch?: string): { current: string; previous: string }` | `git.fetch { path: string, remoteName: $remote, remote: string, branch: string }` | Fetch and fast-forward the current branch under git.fetch. |
| `remotes(): { name: string; url: string }[]` |  | List named remotes without credentials. |
| `setRemoteUrl(name: string, url: string): void` |  | Update an existing remote's HTTPS URL. |
| `show(ref: string, path: string): Uint8Array` |  | Read file bytes from a commit. |
| `status(): { branch?: string; clean: boolean; entries: { path: string; staged: string; unstaged: string; untracked: boolean }[] }` |  | Inspect staged, unstaged and untracked paths. |
| `switchBranch(name: string): void` |  | Switch local branches; requires a completely clean working tree. |

## `submilli:http`

Outbound HTTP: get/post/put/patch/delete/head.

| Function | Capability | Description |
| --- | --- | --- |
| `delete(url: string, headers?: Headers): Response` | `http.delete { host: $url.host, path: $url.path, body_size: number, timeout_ms: number }` | Issue a synchronous HTTP DELETE. |
| `download(url: string, path: string, options?: DownloadOptions): DownloadResult` | `http.download { host: $url.host, url_path: $url.path, vfs_path: $path, max_bytes: number, overwrite: boolean, decompress: boolean }`<br>`fs.write { path, max_bytes: number }` | Download a remote file directly to the VFS — wget-style. |
| `get(url: string, headers?: Headers): Response` | `http.get { host: $url.host, path: $url.path, body_size: number, timeout_ms: number }` | Issue a synchronous HTTP GET. |
| `head(url: string, headers?: Headers): Response` | `http.head { host: $url.host, path: $url.path, body_size: number, timeout_ms: number }` | Issue a synchronous HTTP HEAD. |
| `options(url: string, headers?: Headers): Response` | `http.options { host: $url.host, path: $url.path, body_size: number, timeout_ms: number }` | Issue a synchronous HTTP OPTIONS. |
| `patch(url: string, body?: string \| Uint8Array \| {} \| unknown[] \| null, headers?: Headers): Response` | `http.patch { host: $url.host, path: $url.path, body_size: number, timeout_ms: number }` | Issue a synchronous HTTP PATCH. |
| `post(url: string, body?: string \| Uint8Array \| {} \| unknown[] \| null, headers?: Headers): Response` | `http.post { host: $url.host, path: $url.path, body_size: number, timeout_ms: number }` | Issue a synchronous HTTP POST. |
| `put(url: string, body?: string \| Uint8Array \| {} \| unknown[] \| null, headers?: Headers): Response` | `http.put { host: $url.host, path: $url.path, body_size: number, timeout_ms: number }` | Issue a synchronous HTTP PUT. |
| `request(method: string, url: string, body?: string \| Uint8Array \| {} \| unknown[] \| null, headers?: Headers): Response` | `http.get { host: $url.host, path: $url.path, body_size: number, timeout_ms: number }`<br>`http.post { host: $url.host, path: $url.path, body_size: number, timeout_ms: number }`<br>`http.put { host: $url.host, path: $url.path, body_size: number, timeout_ms: number }`<br>`http.patch { host: $url.host, path: $url.path, body_size: number, timeout_ms: number }`<br>`http.delete { host: $url.host, path: $url.path, body_size: number, timeout_ms: number }`<br>`http.head { host: $url.host, path: $url.path, body_size: number, timeout_ms: number }`<br>`http.options { host: $url.host, path: $url.path, body_size: number, timeout_ms: number }` | Issue an HTTP request with a runtime-chosen verb. |

### `DownloadOptions`

Options bag for [`download`].

| Member | Description |
| --- | --- |
| `decompress?: boolean` | When `true`, transparently gunzip / unzstd the response if the `Content-Encoding` header (or URL suffix `.gz` / `.zst`) advertises a compressed stream. |
| `headers?: Headers \| undefined` | Request headers — typically used for auth (`Authorization: Bearer …`). |
| `maxBytes?: number` | Maximum bytes to write to disk (post-decompression when `decompress` is true). |
| `overwrite?: boolean` | When `true`, clobber an existing file at `path`. |
| `timeout?: number` | Per-request timeout in milliseconds. |

### `DownloadResult`

Result of [`download`] — what was written, where, and the response metadata.

| Member | Description |
| --- | --- |
| `readonly bytesWritten: number` | Bytes actually written to disk (post-decompression when `decompress` was set). |
| `readonly contentType: string` | `Content-Type` header from the response, lowercased. |
| `readonly duration_ms: number` | Wall-clock duration of the download in milliseconds, measured from temp-file open through atomic rename. |
| `readonly finalUrl: string` | Final URL after any redirects. |
| `readonly path: string` | The VFS path the file was written to — echoes the `path` argument. |
| `readonly status: number` | HTTP status code (e.g. `200`). |
| `toString(): string` | Concise description — `"Download(200, 1024 bytes -> /workspace/file.csv)"`. |

### `Headers`

HTTP header bag: `Map<string, string>` with names lowercased on the wire.

```typescript
type Headers = Map<string, string>;
```

### `Response`

Result of an HTTP call — status, headers, body, and the final URL after any redirects.

| Member | Description |
| --- | --- |
| `readonly body: string` | Response body, decoded as UTF-8. |
| `readonly headers: Headers` | Response headers. |
| `readonly ok: boolean` | `true` iff `status` is in the 2xx range. |
| `readonly status: number` | HTTP status code (e.g. `200`, `404`). |
| `readonly statusText: string` | Reason phrase (e.g. `"OK"`, `"Not Found"`). |
| `readonly url: string` | Final URL after any redirects. |
| `json(): unknown` | Parse the response body as JSON into `unknown`. |
| `throwForStatus(): void` | Throw if `ok` is `false`. |
| `toString(): string` | Concise description of the response — `"Response(200 OK, https://...)"`. |

## `submilli:llm`

Gated model calls: call/batch, and models() to discover them.

| Function | Capability | Description |
| --- | --- | --- |
| `batch<T>(model: string, prompts: string[], schema?: string): T` | `llm.call { model, prompt_count: $prompts.length }` | Send every prompt to `model` and return one completion each, positionally: `result[i]` is the outcome of `prompts[i]`, including when that element failed. |
| `call<T>(model: string, prompt: string, schema?: string): T` | `llm.call { model, prompt_count: 1 }` | Send one prompt to `model` and return its completion. |
| `models(): Model[]` | `llm.call { prompt_count: 0 }` | The models this runtime serves and this caller may call. |

### `Completion`

One prompt's outcome, positionally matched to the prompt that produced it.

| Member | Description |
| --- | --- |
| `readonly finishReason: string \| undefined` | The provider's own raw stop reason, for diagnosis only. |
| `readonly inputTokens: number \| undefined` | Prompt tokens the provider reported. |
| `readonly message: string \| undefined` | A fixed classification of the failure, `undefined` when `ok`. |
| `readonly ok: boolean` | Whether the model stopped naturally. |
| `readonly outputTokens: number \| undefined` | Completion tokens the provider reported, with the same `undefined`-means-indeterminate rule as `inputTokens`. |
| `readonly reason: string \| undefined` | Why this element is not a clean completion, `undefined` when `ok`. |
| `readonly retryable: boolean` | Whether re-sending this identical prompt could plausibly succeed. |
| `readonly status: number \| undefined` | The HTTP status the provider answered with, `undefined` when none was observed — which is what distinguishes a dead connection from a provider that answered with an error. |
| `readonly text: string \| undefined` | The completion text. |

### `Model`

One model this caller may call.

| Member | Description |
| --- | --- |
| `readonly contextWindow: number \| undefined` | The model's context window in tokens, or `undefined` when the operator declared none. |
| `readonly description: string \| undefined` | Operator-authored deployment intent, or `undefined` when none was declared. |
| `readonly name: string` | The model name, exactly as `call` and `batch` expect it. |

## `submilli:secrets`

Policy-gated access to Blueprint-declared secrets.

| Function | Capability | Description |
| --- | --- | --- |
| `get(secret: string): string \| undefined` | `secrets.get { name: $secret }` | Resolve a Blueprint-declared secret by name. |

## `submilli:session`

Session-scoped key-value state: get/has/set/remove/list.

| Function | Capability | Description |
| --- | --- | --- |
| `get<T>(key: string): T \| undefined` | `session.read { key }` | Read a session value, checked against `T`. |
| `has(key: string): boolean` | `session.read { key }` | Whether the session holds an entry for `key`. |
| `list(prefix: string, limit: number, cursor?: string): Page` | `session.list { prefix }`<br>`session.read { key } - per candidate key` | Enumerate session keys starting with `prefix`, in UTF-16 code-unit order. |
| `remove(key: string): boolean` | `session.remove { key }` | Delete `key`. |
| `set(key: string, value: unknown): void` | `session.write { key }` | Store `value` under `key`, replacing any previous entry. |

### `Entry`

One key a `list` page discloses.

| Member | Description |
| --- | --- |
| `readonly key: string` | The session key, in the exact code units it was stored under. |
| `readonly sizeBytes: number` | Serialized size of the stored value in bytes — the value alone, not the key and not the store's per-entry overhead. |

### `Page`

One page of a `list` call: the `entries` it discloses and the `nextCursor` that continues it.

| Member | Description |
| --- | --- |
| `readonly entries: Entry[]` | The keys this page discloses, in UTF-16 code-unit order. |
| `readonly nextCursor: string \| undefined` | Opaque cursor for the next page, or `undefined` when no further matching key remains. |

## `submilli:url`

URL parse/build and query-string handling. Pure compute.

| Function | Capability | Description |
| --- | --- | --- |
| `build(protocol: string, host: string, port: number \| undefined, path: string, query: Query, fragment?: string): string` |  | Serialise URL parts to an absolute URL string. |
| `decodeComponent(s: string): string` |  | Percent-decode `s`. |
| `decodeQuery(s: string): Query` |  | Parse a URL-encoded query string into a `Query` (`Map<string, string>`). |
| `encodeComponent(s: string): string` |  | Percent-encode `s` per RFC 3986 (UTF-8 then `%HH` for every byte that isn't an ASCII alphanumeric or one of `-`, `.`, `_`, `~`). |
| `encodeQuery(query: Query): string` |  | Serialise a `Query` (`Map<string, string>`) to a URL-encoded query string (`k=v&k=v`). |
| `parse(url: string): URL` |  | Parse an absolute URL string into its parts. |

### `Query`

Query-string parameter map.

```typescript
type Query = Map<string, string>;
```

### `URL`

Parsed URL parts — the return type of `parse`.

| Member | Description |
| --- | --- |
| `readonly fragment: string \| undefined` | Fragment string (without the `#` prefix), or `undefined` when the URL has no `#` segment. |
| `readonly host: string` | Host name, e.g. `"api.acme.com"`. |
| `readonly path: string` | Path component, including the leading `/`. |
| `readonly port: number \| undefined` | Port number, or `undefined` when the URL omits one. |
| `readonly protocol: string` | Scheme, e.g. `"https"`. |
| `readonly query: Query` | Query parameters as a `Query` (`Map<string, string>`). |

## `submilli:uuid`

UUID v4/v7 generation and validation.

| Function | Capability | Description |
| --- | --- | --- |
| `v4(): string` |  | Generate a random UUID v4 (RFC 4122). |
| `v7(): string` |  | Generate a time-ordered UUID v7 (RFC 9562). |
| `validate(string: string): boolean` |  | Returns `true` if `string` is a valid UUID (any version), `false` otherwise. |

<!-- /generated:stdlib -->
