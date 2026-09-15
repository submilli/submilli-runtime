# Skipped and rejected test262 material

Record of what was *not* ported and why, so "skipped" never reads as
"passing". Per-area details live in `skips/<Area>.md`; this file holds the
conventions and the blanket rules. Three dispositions:

- **rejected** — excluded by a design decision; representative originals
  live under `rejected/` for a future revisit.
- **known gap** — should work per our spec but doesn't; ported anyway and
  marked `// expect-fail: <reason>`, tracked in Linear.
- **not ported** — portable in principle but below the curation bar
  (redundant with a ported case, or marginal); listed by pattern.

Blanket rejections that apply to every area (not repeated per area):
`prop-desc.js`, `length.js`, `name.js`, `not-a-constructor.js`,
`this-is-not-object.js`, `return-abrupt-*` (getter/`valueOf` coercion
traps), `Symbol.*` cases, species-constructor cases, prototype-chain
cases, `eval`-based cases, locale-dependent cases.

---
