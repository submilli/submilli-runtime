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

`npm run build` produces the static website in `docs-site/dist/`, including
HTML, styles, scripts, and the search index. The preparation script creates an
ignored TypeScript configuration stub if the runtime package tooling has not
been initialized; it never replaces an existing generated configuration.

## Editing

Edit pages in `docs/`. Frontmatter `slug` controls the path below `/docs/`;
the index has an empty slug. `sidebar.hidden: true` keeps unfinished chapters
out of navigation. Sidebar groups are configured in `astro.config.mjs`; their
directories are relative to this application because the book lives outside it.

## Hosting

The build targets `https://submilli.ai/docs/`. Publish the **contents** of `dist/`
at that URL prefix, including `_astro/`, `pagefind/`, and the favicon. Route
`/docs` to `/docs/`. The marketing website continues to serve `/` and `/blog/`.
For a shared static deployment, place this output in the marketing output's
`docs/` directory; for separate origins, route `/docs/*` to this site's output
with the prefix stripped. Neither source repository needs to build the other.

The `Documentation` workflow checks and builds the site and uploads a
`documentation-site` artifact. It does not deploy. Connect the docs output to
hosting before deploying the marketing site without its former book pages.
The existing pre-launch `noindex, nofollow` setting is preserved.
