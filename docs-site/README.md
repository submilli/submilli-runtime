# Documentation website

The user book is Markdown in `../docs/`. This Astro/Starlight application reads
those files directly; no private repository or runtime build is needed.
`../llm-prompt.md` is a runtime build input and is not part of the book.

## Build and preview

Use Node.js 22 or newer, from the repository root:

```sh
npm --prefix docs-site ci
npm --prefix docs-site run check
npm --prefix docs-site run build
npm --prefix docs-site run preview
```

Open the preview URL with `/docs/` appended (normally
`http://localhost:4321/docs/`). For live editing, run
`npm --prefix docs-site run dev`.

`npm run build` produces the static website in `docs-site/dist/docs/`, including
HTML, styles, scripts, and the search index. The preparation script creates an
ignored TypeScript configuration stub if the runtime package tooling has not
been initialized; it never replaces an existing generated configuration.

## Editing

Use the [writing framework](WRITING.md) to define a chapter's purpose and reader
outcomes before drafting, then review the draft against those outcomes.

Edit pages in `docs/`. Frontmatter `slug` controls the path below `/docs/`;
the index has an empty slug. `sidebar.hidden: true` keeps unfinished chapters
out of navigation. Sidebar groups are configured in `astro.config.mjs`; their
directories are relative to this application because the book lives outside it.

## Theme

The look follows the Submilli design system: `#131313` dark canvas (light
optional), SF Pro Rounded with a self-hosted Nunito fallback for text, IBM Plex
Mono for labels and code, and Light Blue as the single accent.

- `src/styles/theme.css` — brand tokens mapped onto Starlight's CSS variables,
  plus restyles for the sidebar, table of contents, search, content, and asides.
- `src/components/` — Starlight component overrides registered in
  `astro.config.mjs`: header, logo lockup, GitHub link with build-time star
  count, two-state theme toggle, page title with group eyebrow, docs home hero,
  footer, pagination, edit link, and last-updated date (read from git because
  the book lives outside the default docs directory).
- `src/code-themes.mjs` — the dark and light Expressive Code themes.
- `src/plugins/satteri-security-aside.mjs` — adds `:::security` to the four
  built-in asides for facts the runtime enforces.
- `src/content/i18n/en.json` — UI strings such as the search placeholder.

Chapters can use `:::note`, `:::tip`, `:::caution`, `:::danger`, and
`:::security` callouts, and `title="file.ts"` on code fences for a file tab.

## Hosting

The build targets `https://submilli.ai/docs/`. Astro writes directly to
`dist/docs/`, so publishing `dist/` at a domain root serves the book and all
its assets under `/docs/`. No copy step or prefix-stripping rewrite is needed.
The marketing website continues to serve `/` and `/blog/`.

### Render

- Service type: Static Site, repository `submilli/submilli-runtime`, branch `main`.
- Root Directory: leave empty (the build also reads the sibling `docs/` folder).
- Build Command: `npm --prefix docs-site ci && npm --prefix docs-site run build`.
- Publish Directory: `docs-site/dist`.
- Environment: `NODE_VERSION=22`.
- Remove the former `/docs` redirect and `/docs/*` rewrite from the docs service.
- Open `https://YOUR-DOCS-SERVICE.onrender.com/docs/` after deploying.

To keep the public URL on the marketing domain, its static service can rewrite
`/docs/*` to `https://YOUR-DOCS-SERVICE.onrender.com/docs/*`, preserving the prefix.

The `Documentation` workflow checks and builds the site and uploads a
`documentation-site` artifact. It does not deploy. Connect the docs output to
hosting before deploying the marketing site without its former book pages.
The existing pre-launch `noindex, nofollow` setting is preserved.
