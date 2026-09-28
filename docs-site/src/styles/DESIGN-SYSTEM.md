# Design-system provenance

`design-system.css` is a verbatim snapshot of
`design-system/tokens.css` from the private `submilli/submilli-private`
repository, taken from `origin/main` on 2026-09-27. Its source is the
canonical design system described by `design-system/README.md` at commit
`517fe87ba5960a8ca51b6c2f1d8723fc0fd9e8f0`.

The snapshot keeps this public documentation build independent of the private
repository. When canonical tokens change, update this file from the source in a
reviewed change and refresh this note's capture revision.

`theme.css` maps Starlight and the existing `--sub-*` compatibility variables
to the semantic design-system tokens. It intentionally keeps these docs-only
adaptations:

- A solid, compact reading surface and a smaller type scale than the marketing
  UI.
- A deep-blue light-theme link (`#2b6d99`) and readable muted text. The raw
  light semantic accent and muted values do not meet AA contrast on the docs'
  white and blue-100 surfaces.
- Code and callout backgrounds derived from semantic tokens, because Starlight
  needs context-specific surfaces that the canonical vocabulary does not name.
