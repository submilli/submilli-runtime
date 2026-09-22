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
