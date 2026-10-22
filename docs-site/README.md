# Documentation website

The user book is Markdown in `../docs/`. This Astro/Starlight application reads
those files directly; no private repository or runtime build is needed.
`../llm-prompt.md` is a runtime build input and is not part of the book.

## Build and preview

Use Node.js 22.18 or newer, from the repository root:

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

## Link checks

CI checks the built HTML with Lychee v0.24.2. It checks local pages and anchors,
the canonical `https://submilli.ai/docs` URLs against the same build, and external
HTTP links. This repository's GitHub `edit/main` and `blob/main` links are checked
against source files in the checkout, avoiding GitHub's rate-limited web UI.
Missing source files still fail the check. Signed-in account-setup pages that reject
automated clients are excluded explicitly in `lychee.toml`.

To run the fixture checks locally, install Lychee and set
`LYCHEE_BIN=/path/to/lychee`, then run `npm run check:links:fixtures`.

## Editing

Use the [writing framework](../docs/WRITING.md) to define a chapter's purpose and reader
outcomes before drafting, then review the draft against those outcomes.

Edit pages in `docs/`. Frontmatter `slug` controls the path below `/docs/`;
the index has an empty slug. `sidebar.hidden: true` keeps unfinished chapters
out of navigation. Sidebar groups are configured in `astro.config.mjs`; their
directories are relative to this application because the book lives outside it.

## Theme

The look uses a versioned snapshot of the shared Submilli design-system tokens: `#131313` dark canvas (light
optional), SF Pro Rounded with a self-hosted Nunito fallback for text, IBM Plex
Mono for labels and code, and Light Blue as the single accent.

- `src/styles/design-system.css` — shared token snapshot, with provenance and reading-surface adaptations documented alongside it.
- `src/styles/theme.css` — shared tokens mapped onto Starlight's CSS variables,
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

Website links pass `?theme=dark` so the docs match the dark marketing page,
including when opening from a preview on a different origin. The inline theme
provider applies that choice before content paints, persists it using Starlight's
storage key, and removes the handoff parameter while preserving other parameters
and anchors. Readers can then switch themes normally. Direct visits retain the
saved preference or use the system preference if none is saved. Blocked browser
storage does not prevent the current page's theme handoff or toggle.

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

### Retired documentation URLs

`src/lib/legacy-docs.ts` lists retired chapter paths and their current destinations.
The static build emits redirect documents at those paths, so the docs origin
can replace old content with a link and browser redirect. These documents do
not configure HTTP status codes on the hosting service.

For permanent HTTP redirects on `submilli.ai`, configure matching 301 rules on
the marketing service before its `/docs/*` proxy rewrite. Remove obsolete docs
files from that service's published output first: Render serves an existing
file before evaluating redirect or rewrite rules. Preserve the current docs
proxy and query parameters. Verify both the old HTML URLs and their `.md`
alternatives after deployment, along with representative bookmarked anchors.

### Crawling and search

The public book is indexable. Starlight generates
`https://submilli.ai/docs/sitemap-index.xml` and its child sitemap using the
`site` and `base` settings. The book cutover is complete: published chapters
use their final slugs and participate in Pagefind search.

Crawler rules belong at `https://submilli.ai/robots.txt`, which the marketing
website owns (`website/public/robots.txt` in its repository). That file must
allow `/docs/` and advertise
`Sitemap: https://submilli.ai/docs/sitemap-index.xml`. A file in this app's
`public/` directory would be served at `/docs/robots.txt` and would not control
crawling. Deploy the marketing robots change together with the docs launch.

After deployment, check the public responses (including any `X-Robots-Tag`
headers) and confirm the chapter HTML has no `noindex` or `nofollow` directive:

```sh
curl --fail --include https://submilli.ai/robots.txt
curl --fail --include https://submilli.ai/docs/why/
curl --fail https://submilli.ai/docs/sitemap-index.xml
curl --fail https://submilli.ai/docs/sitemap-0.xml
```

Submit the docs sitemap in Google Search Console if the domain is verified
there. Crawl permission does not guarantee immediate search-engine indexing.
Standalone video embeds retain their own `noindex, nofollow` directive and
are excluded from the sitemap.

For the site's search, use a production build and preview: Pagefind's index
is generated at build time. Open `/docs/`, search for `blueprint` and `fuel`,
and follow a result for each. Results and search assets must stay under
`/docs/`, with no retired `/next/` paths or doubled `/docs/docs/` prefix.

## Documentation for agents

The build generates `/docs/llms.txt` (chapter index), `/docs/llms-full.txt` (the complete
visible book), and `/docs/<slug>.md` from the same `docs/` sources. The docs
home exports as `/docs/index.md`. Every visible page needs a title, description,
and explicit slug; `sidebar.hidden: true` excludes a page from all exports.
The index follows the book's directory groups and sidebar order.

Markdown exports include the chapter title and source URL without presentation
frontmatter. Links to visible chapters point to their absolute Markdown URLs;
other relative links resolve against the chapter's public HTML URL. Code blocks
and callouts retain their original source. Each visible HTML page advertises its
Markdown alternative and the index in `<link>` elements, plus visible footer
links. The bundled skill directs assistants to the index for topics beyond its
local references.

`npm run check` tests index coverage, hidden-page exclusion, metadata, and link
rewriting. `npm run build` also verifies the generated files and HTML discovery
links. The exports are build artifacts; run a build before previewing them.
Astro's normal preview serves the book and agent indexes under `/docs/`.
The marketing service's existing `/docs/*` rewrite forwards all these files to
the docs service; no additional root-path rewrites are needed.

Serve `.md` files as `text/markdown; charset=utf-8` and `.txt` files as
`text/plain; charset=utf-8`. These must be file responses, not the marketing
site's HTML fallback. The index files are in `dist/docs/`, alongside the book.
After publishing, verify the public paths:

```sh
curl --fail https://submilli.ai/docs/llms.txt
curl --fail https://submilli.ai/docs/llms-full.txt
curl --fail https://submilli.ai/docs/blueprints.md
```

## Analytics

The docs site uses the existing PostHog project when `PUBLIC_POSTHOG_KEY` and
`PUBLIC_POSTHOG_HOST` are set in the hosting environment. The key is a public
capture token; do not put a secret in source or commit an environment file.

Analytics follows the marketing site’s opt-out behavior and respects the existing
`submilli.analytics-consent.v1` preference and PostHog opt-out. Readers can change
the preference using the docs analytics settings. Automatic capture, automatic
pageviews, feature flags, surveys, and recorded request
bodies are disabled. Pageviews are emitted once per canonical
docs URL, including client-side `astro:page-load` navigation. URL properties are
reduced to origin and path before capture. Session replay loads through PostHog's
default browser bundle on the canonical `submilli.ai` hostname only, using the
existing project's recording rules. Input masking is explicitly disabled at the
owner's request; replay can include text readers enter. Console logs, network
headers/bodies and error capture remain disabled. Preview origins and marked
`analytics_test` sessions do not record.

Internal browsers must open `https://submilli.ai/docs/?analytics=off` once per
browser/profile. The query applies before SDK initialization and saves the shared
website/docs opt-out in localStorage plus the first-party
`submilli_analytics_consent=denied` cookie (`Path=/`, host-only); either denial
blocks startup. A localStorage denial is propagated to already-open tabs by the
storage event; cookie-only enrollment requires reloading already-open tabs. Each profile
and fresh agent context must be enrolled separately; this repository cannot
enforce opt-out in unmanaged profiles or claim that an anonymous browser belongs
to a team member. The internal cohort is a reporting filter, not a browser
recording gate. Preview/Render origins and marked verification sessions do not
initialize analytics or capture events.

The event inventory is `$pageview` (`page_path`, `$current_url`),
`link_clicked` (`destination`, `external`), and `docs_cta_clicked`
(`cta_id`, `destination`, `external`). Meeting clicks also emit the existing
`booking_cta_clicked` contract (`cta_location: docs` or `docs-invitation`,
`booking_provider: google_calendar`, `destination`, `external`) so website
funnels can include docs traffic. CTA IDs cover `meeting`, `website`, `github`,
`discord`, `agent_setup`, `view_markdown`, and `agent_docs`.
Synthetic verification can add `?analytics_test=1` (or a verification UTM). The
marker is sticky for the browser tab session, but marked sessions are suppressed:
the SDK does not initialize and no events are captured, including after navigating
to another docs page. Use a fresh profile or clear the marker before testing
normal production capture.

At `https://submilli.ai/docs/`, the same capture key and `localStorage` persistence
reuse marketing’s PostHog identity and `submilli.attribution.v1` first/latest touch.
The direct Render hostname is a separate origin and cannot join that identity.
Production capture requires configuring both public environment values in the
docs Render service and rebuilding after merge; repository checks alone do not
verify received production events.

## Reader invitation

The dismissible invitation requires ten seconds of visible docs browsing plus a
reader action: following a docs link, searching, copying a page/code/section link,
or scrolling at least 80 pixels with wheel, touch or keyboard input. A landing,
restored scroll position or programmatic click alone never qualifies. Engagement
can carry into the next page only through a just-clicked docs link. Every page
starts a fresh ten-second visible delay, including docs navigation. Reloads and
direct landings also require fresh engagement. Closing it
or successfully saving an email permanently dismisses it in localStorage for
that browser and origin. It offers
an email form plus the same Google scheduler as the header. It takes no
focus and does not block navigation. Set `PUBLIC_CONTACT_ENDPOINT` to a verified
production contact-intake URL to enable it; without an endpoint it stays hidden.
Excluded routes include 404, hidden sidebar pages and standalone video embeds.

The endpoint accepts JSON `{ "email": "reader@example.com", "form_id": "docs-invitation" }`
and must return a 2xx `{ "ok": true }` only after durable contact storage.
Validate input server-side, deduplicate by normalized email, allow only the
production origin via CORS, and keep service credentials on the server. No email
is stored in browser storage or added to explicit analytics events. Configure the
current contact service only after verifying its durable-storage receipt. A
preview uses only a local mock endpoint, never production contact storage.

After deployment, the PR author verifies real email storage and retry semantics,
the booking destination, a production replay, and each team browser's opt-out.
An enabled SDK or recording toggle alone does not establish working playback.

Coverage is limited to JavaScript-enabled HTML visits. Direct Markdown, `llms.txt`,
and bot requests without JavaScript are not counted; agent-docs CTA clicks measure
navigation intent, not confirmed retrieval.
