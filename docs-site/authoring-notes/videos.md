# Video integration

## Chapter brief and scope

Purpose: explain how a program moves tool coordination out of the model loop.
The reader knows what an agent and an API are. Afterward they can explain the
failed-payment example, distinguish model rounds from tool calls, and recognize
that execution still needs permission boundaries. Their next action is to read
how Submilli works or run the quickstart. This is not a benchmark, product demo,
or a rewrite of the introduction and book hierarchy.

`src/data/videos.json` owns film identity, asset metadata, publication state, and
future destinations. The canonical home is `/docs/concepts/execution-model/`;
the index is `/docs/videos/`. The transcript is Markdown and included in the
existing agent exports. Planned records create no pages or players. Helps, Works,
Using, and Challenges are editorial slots, not finished films.

The September 30 restructuring proposal places this topic under Concepts →
Execution model. This change adds only that leaf and the library; it does not
move or rename Doron's chapters. No open runtime restructure PR was present when
inspected on October 1. Preserve these stable paths when the migration lands.

## Approved asset and release gate

Use only `editor-export-1790685929055.mp4`, not a later editor draft or Challenges
pilot. Its snapshot is `export-51c09794-8edf-4fe8-bb28-fa7613f54e18.json`.
The registry records its SHA-256 and 90.688-second container duration. Verified:
2,720 frames at 30 fps, 1920 × 1080, H.264 with AAC audio. The transcript is
verbatim from the snapshot. The 32 phrase-length WebVTT cues preserve every spoken word. Cue boundaries
for the first six scenes follow pauses detected in the existing audio; the last
scene uses its saved character alignment. All include the renderer's +0.5-second
audio lead. Each cue uses at most two lines of 42 characters, lasts 1–7 seconds,
and stays below 20 characters per second. Tests enforce these limits and complete
transcript coverage. No narration or production sources were changed. The pause-based
boundaries are not word-level forced alignment; final owner listening review remains
part of publication QA.

Publication requires owner confirmation of the production ElevenLabs account's
commercial-use license and an approved media host. No upload or new service is
part of this PR. Never commit the MP4. Once cleared, set `src` to the approved
HTTPS asset URL and `status` to `published` in a reviewed change. Verify content
type, byte-range support and browser playback; the same-origin VTT stays with the
docs build. Update publication-pending wording in the library and transcript in
that same change. Until then, production shows the transcript fallback without
a video request or broken player.

## Local review

Copy the verified file to the ignored path
`docs-site/public/_video-preview/introduction.mp4`, then run from the repo root:

```sh
SUBMILLI_VIDEO_PREVIEW_PATH=/docs/_video-preview/introduction.mp4 npm --prefix docs-site run dev
```

The override accepts only a local path and works only in development mode.
The build guard refuses a nonempty preview directory, because Astro copies even
ignored public files. Remove the local copy before building. Check native controls,
English captions, keyboard access, transcript navigation, mobile width, and themes.

## Companion site and analytics

The separate `submilli-private/website` companion is a visual viewing gallery.
Its featured iframe uses `/docs/videos/embed/code-execution-introduction/`, which
shares `IntroductionPlayer.astro` with the docs page and index. This keeps source, captions,
poster and publication state in one registry. Site cards for future films are
editorial previews without play controls or invented durations. The original SVG
poster uses the approved film's tool→program→report motif and brand palette.
Deploy and verify the docs embed route before making the site gallery live.
This draft authorizes no deployment.

Analytics impact: new docs routes and site-to-docs links. No new events or player
tracking. Preserve existing site consent, attribution, and booking semantics.
The inspected docs layout has no analytics collector; playback measurement and
joined journeys are unverified. After separately authorized deployment, the
publishing owner should verify route coverage and cross-site continuity in the
existing analytics stack, without assuming it from the site's SDK alone.

## October 1 draft-book compatibility

The newer `/docs/next/` book on main supersedes the earlier structural proposal:
Part 1 is Start here/explanation, Parts 2–4 are Blueprint, Package and Server
how-tos, Part 5 is Tutorials, and Part 6 is Reference. Its `next/why` chapter
already explains the same failed-payment program and evidence as this film.
The introduction registry therefore includes `/docs/next/why/` as a contextual
placement. The shared content override adds the film without changing Doron's
chapter source, sidebar visibility, search settings, or agent-export exclusion.

At cutover, coordinate changing this contextual path to `/docs/why/`. The draft
book has no Concepts section: decide with Doron whether the current execution-model
page stays as a supplementary deep link or redirects into Why, and whether its
separate sidebar group is retained. Do not make that book-wide decision in this
video PR. Existing canonical, transcript and embed URLs stay stable meanwhile;
the site gallery must not send ordinary readers into hidden draft pages.
