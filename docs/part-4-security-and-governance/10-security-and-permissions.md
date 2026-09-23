---
title: "Security and permissions"
slug: security
sidebar:
  hidden: true
---

This chapter is being written.

<!--
  AUTHORING NOTE — promises made by earlier chapters that this one must keep.

  "Crafting a blueprint" (06) covers rule shape, first-match resolution, and
  just enough filter syntax for its examples, then says "the full grammar is in
  security and permissions". Include here, checked against
  crates/submilli-blueprint/src/filter.rs:

  1. The full filter grammar: `or` over `and` over `not` over comparison or
     parenthesised group; operators `== != < <= > >=`, `glob`, `matches`,
     `contains`; string literals and escapes; numbers, `true`/`false`/`null`;
     dotted field paths and array indexing; `${vars.NAME}` bare or inside a
     quoted string; `matches` takes a literal regex only (no interpolation), is
     unanchored, and is compiled at parse time.
  2. Evaluation rules an evaluator needs stated: a missing field, wrong-kind
     value, or unbound variable is a non-match, never an error; so `!=` on a
     missing field is false and `not (...)` on a missing field is true;
     `== null` matches only an explicit null; `${vars.X}` is coerced by the
     context field's kind; a variable interpolated into a `glob` pattern is
     escaped, so a value containing `*` or `?` can't widen the pattern.
  3. Filters are parsed at registration, so a malformed filter fails
     `blueprint apply`, not the first call; show a parse diagnostic.
  4. `default: allow` as a blocklist posture and why the book doesn't recommend
     it; `ask-human` exists in the schema but currently behaves as deny (left
     out of 06 on purpose).
  5. The `secrets.get` main carve-out and caller attribution, already
     introduced in 03 and 06; this chapter can go into the check() context and
     the permission-denied message's "do not work around" tail.

  Cross-check "How Submilli works" (03), which links here as "[Security and
  permissions] covers the rule syntax".
-->

## Git capabilities

The [blueprint's Git configuration](/docs/blueprints#let-the-program-commit)
enables the module; it does not grant permissions. Each gated operation checks
the current caller, including methods on a repository object obtained by another
package. Git grants cover the directories and files those operations create or
check out, without additional filesystem grants.

All Git capabilities report the normalized absolute repository `path` and
operation `op`. Each gated operation requires one grant:

| Capability | Used by | Additional filter fields |
| --- | --- | --- |
| `git.init` | `init` | None |
| `git.clone` | `clone` | `remoteName`, `remote`, `branch` |
| `git.fetch` | `fetch`, `pull` | `remoteName`, `remote`, `branch` |
| `git.commit` | `commit` | `branch` |

`git.init` authorizes creating a local repository and its directory. `git.clone`
authorizes creating and populating the destination itself; it does not require
`git.init`, `git.fetch`, or filesystem grants. `git.fetch` also authorizes the
working-tree update performed by a fast-forward pull.

Local inspection, staging, branch creation and switching, and remote
configuration require no separate Git capability. They remain confined to the
VFS, and branch switching still refuses dirty worktrees.

Use an explicit branch for clone or fetch when the grant filters on `branch`,
so the initial check can match before contacting the remote. Each selected
remote branch is checked before repository data is received. Commit checks
the current local branch.

`remote` is the complete, normalized HTTPS repository URL, not a hostname.
Capability checks resolve dot segments and normalize hostnames and default
ports before matching this field. Credentials and query strings are prohibited
in these URLs. Redirects are refused and outbound connections use the host's
DNS/IP policy. Named remotes do not grant
network access: fetch checks their current URL every time.

An allowed clone or fetch includes authentication to its permitted remote.
After an HTTP authentication challenge, the runtime resolves `GIT_TOKEN` and
sends it with the configured username; the token never enters guest code. The
auth proxy, guest headers, credential helpers, and prompts cannot supply Git
credentials.

Filesystem permissions do not authorize direct changes to `.git`, aliases
into it, or recursive removal/movement of a directory containing it.
Repository paths cannot contain symlinks; repository metadata cannot use
symlinks or external object stores. Hooks, external filters, includes, and
ambient Git configuration are not executed;
there is no fallback to a Git subprocess. See the [API compatibility
notes](/docs/standard-library#git-repositories) for supported repository formats.

A host handing a repository to native Git or a future microVM must wait for
active execution to finish and give that executor exclusive ownership; the
runtime cannot lock out an unrelated host process. See [resource
limits](/docs/resource-limits#git-work) for worker cancellation and VFS lifetime.
