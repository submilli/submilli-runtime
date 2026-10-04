# Video integration

## Chapter brief and scope

Purpose: explain how a program moves tool coordination out of the model loop.
The reader knows what an agent and an API are. Afterward they can explain the
failed-payment example, distinguish model rounds from tool calls, and recognize
that execution still needs permission boundaries. Their next action is to read
how Submilli works or run the quickstart. This is not a benchmark, product demo,
or a rewrite of the introduction and book hierarchy.

`src/data/videos.json` owns film identity, asset metadata, publication state, and
future destinations. The canonical home is `/docs/why/`;
the index is `/docs/videos/`. The transcript is Markdown and included in the
existing agent exports. Unpublished records create no players. Challenges, Helps,
Using, and Understanding have review cuts; their publication remains pending.

The current book supersedes the September 30 Concepts → Execution model
proposal. This integration retains the book's hierarchy and adds the library;
the former execution-model URL redirects to Why Submilli.

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
HTTPS asset URL, `publishedVersion` to the approved immutable version ID, and
`status` to `published` in a reviewed change. Verify content
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

The separate `submilli-private/website` companion links directly to
`/docs/videos/`. The docs own the visual gallery, with numbered planned cards and
a separate transcript page. `IntroductionPlayer.astro` is shared with contextual docs
pages and the standalone embed route, keeping source, captions, poster and
publication state in one registry. Planned films have no play controls or
invented durations. The original SVG poster uses the approved film's
tool→program→report motif and brand palette. Verify the docs library route
before making the homepage link live. This draft authorizes no deployment.

Analytics impact: new docs routes and site-to-docs links. No new events or player
tracking. Preserve existing site consent, attribution, and booking semantics.
The inspected docs layout has no analytics collector; playback measurement and
joined journeys are unverified. After separately authorized deployment, the
publishing owner should verify route coverage and cross-site continuity in the
existing analytics stack, without assuming it from the site's SDK alone.

## Current book placement and stable publication

The Diátaxis book is now the default: Start here, Blueprints, Packages, Server,
Tutorials, and Reference. The introduction belongs on `/docs/why/`. The former
`/docs/concepts/execution-model/` route redirects there, and no competing Concepts
sidebar group is added. The library remains `/docs/videos/`; its deeper transcript
page and stable `/docs/videos/embed/code-execution-introduction/` endpoint are
unchanged. Retired `next/` routes are not part of this integration.

Film IDs are stable and define the series order: `code-execution-introduction`,
`challenges`, `helps`, `using`, `works`. The viewing sequence is concepts,
challenges, what Submilli is and how it helps, using it, then understanding it.
The gallery keeps transcripts off the main viewing page.

An editor save is a draft revision, not publication. An authorized Publish version
handoff must supply the reviewed public HTTPS asset URL, immutable version ID,
matching captions/transcript and approval receipt. Only then may the publishing
owner update `src`, `publishedVersion`, and `status: published` together. The
stable embed URL resolves the registry record, so consumers do not change URLs
for each export. A public source requires both published state and a version ID;
Mac-local/loopback URLs and credentials are rejected. No publisher backend or
external media service is introduced by this draft.

The approved introduction remains the 90.688-second original. Inspection of
`final-media-handoff.json`, `intro-v4/library-delivery.json`, and the October 4
`oct4-revision-handoff.json` found no approval replacing that original. The
four later films have delivered review cuts, with human listening pending.
Challenges and Helps have newer staged revisions with dirty/missing narration;
their gallery records say Revision pending. Using and Understanding say In review.
No review cut is used as a public source or described as final. The October 4
handoff is an editor import of separate catalog versions, not a publish receipt.

Hosting, production ElevenLabs licensing and final listening/review remain
publication gates. This PR performs no recording, paid generation, asset upload,
editor-source change or deployment.
