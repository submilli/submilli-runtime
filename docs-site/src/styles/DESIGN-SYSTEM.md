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

- A white light-theme reading canvas, retaining blue for links, focus, and active navigation.
- A solid, compact reading surface and a smaller type scale than the marketing
  UI.
- A deep-blue light-theme link (`#2b6d99`) and readable muted text. The raw
  light semantic accent and muted values do not meet AA contrast on the docs'
  white reading surfaces.
- Code and callout backgrounds derived from semantic tokens, because Starlight
  needs context-specific surfaces that the canonical vocabulary does not name.


## Documentation design review — 2026-10-06

This pass studies interaction and information hierarchy, rather than reproducing
another company's branding. The examples below were inspected in a browser.
Submilli keeps its typefaces, blue accent, theme preference, content, and routes.

| Source | Observed pattern | Application in Submilli |
| --- | --- | --- |
| [Stripe: API keys](https://docs.stripe.com/keys) | A short lead and page utilities precede the article. Navigation, article, and local contents have distinct visual weights. | Surface each page's existing description; add compact Markdown/copy utilities next to the title; bound the contents panel to its own column. |
| [Stripe: checkout quickstart](https://docs.stripe.com/payments/quickstart) | Instructions and a complete, downloadable example sit together. Language choices affect the example, rather than sending the reader to a different guide. | Keep commands/results visually related and keep code controls secondary. A runnable, downloadable walkthrough is a separate follow-up; this pass does not simulate one. |
| [Stripe: authentication](https://docs.stripe.com/api/authentication) | Small code controls and predictable text/example alignment make dense reference material scannable. | Keep copy icons small, preserve authored file titles, and use alignment and row rules for reference tables. |
| [Increase: documentation](https://increase.com/documentation) | A neutral canvas and collapsed navigation groups leave most of the screen for reading. | White light-mode canvas; quieter containers; collapse inactive technical branches by default while opening the current branch. |
| [Increase: idempotency keys](https://increase.com/documentation/idempotency-keys) | Requests and their results are understood through sequence and proximity, without prominent Input/Output banners. | Retain the shared command box and plain result with a left rule from the preceding code-block refinement. |
| [Increase: Accounts](https://increase.com/documentation/api/accounts) | Resource description, attributes, examples, and page-copy tools occupy stable places. On a phone the columns become a single sequence and navigation becomes a compact menu. | Show purpose before detail, keep page-copy tools near the title, and verify narrow-screen navigation, search, code, and table overflow. |

### Rules for the shared renderer

- Give prose the strongest continuous reading path. Body text is 16px with a
  1.75 line height; headings, spacing, and restrained separators carry hierarchy.
- Keep marketing-scale cards out of routine reading. Videos have a title, player,
  and quiet caption, without another padded card. Setup retains its three direct
  actions at the top of Install and Quickstart.
- Navigation has three jobs: global discovery in search, chapter selection in
  the sidebar, and orientation within an article in the local contents list.
  Do not repeat all three as competing toolbars. Keep the mobile contents bar.
- Book a meeting is the primary header action. Agent setup remains a secondary
  text link, with both actions available in the mobile menu. The booking link
  uses the same destination as the public website.
- Every visible page can expose its already-published Markdown. Copy feedback is
  explicit; a failed fetch or clipboard operation must not look successful.
  View Markdown remains usable without JavaScript.
- Keep authorship tooltips and page-action labels out of the search index. They
  remain accessible on the page without replacing useful article excerpts.
- Controls hidden at rest must remain discoverable through keyboard focus, with
  visible touch targets on devices without hover. Preserve reduced-motion styles.
- Keep long code and tables inside their own horizontal scroll area. The page
  itself must not scroll sideways. Overflowing tables must be keyboard reachable.
- Put the next chapter before maintenance links in the footer. Editing, source
  formats, community links, and timestamps remain available with lower emphasis.
- Implement shared behavior in the renderer and theme, not in every Markdown file.
  Preserve wording, examples, authorship, and existing bookmarked headings.

### Follow-up work with a separate content/product scope

1. **Runnable Quickstart project.** Stripe's major advantage is the complete
   example adjacent to each step. Package the existing sample files with a
   deterministic test and downloadable project, then consider step-linked code
   navigation. Do not add fake execution or a second maintained copy of examples.
2. **Structured API reference.** Increase places attributes and request/response
   examples side by side. Derive endpoint/type metadata from the existing source
   pipeline before introducing that layout; do not guess structure from Markdown
   headings or rewrite human-authored explanations automatically.
3. **Task-based search quality.** The built index returns Permissions and Diagnose
   a denial for `permission denied`, and Connect a harness for `connect a harness`.
   This pass removes authorship-tooltip boilerplate observed in those excerpts.
   Expand the query set from real support questions; improve titles, synonyms, or
   indexing only from observed misses. These spot checks do not establish broad
   search quality.
4. **Editorial walkthrough.** Check that each Start here page has a clear outcome,
   prerequisites, one working path, a success check, and an obvious next step.
   Human authors own narrative revisions under `docs/WRITING.md`.

These are recommendations from this review, not claims that the docs now match
all of Stripe's or Increase's capabilities.
