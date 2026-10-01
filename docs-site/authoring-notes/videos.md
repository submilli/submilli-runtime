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
verbatim from the snapshot. Seven scene-aligned WebVTT cues begin at each scene's
+0.5-second audio lead and end at its recorded audio duration. They are not
word-aligned: review caption readability and refine cue breaks against the audio
before publication, especially the longer scenes.

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

The separate `submilli-private/website` companion links to the canonical home and
index. It must not duplicate the registry, transcript, or player. Deploy and verify
the docs routes before making site links live. This draft authorizes no deployment.

Analytics impact: new docs routes and site-to-docs links. No new events or player
tracking. Preserve existing site consent, attribution, and booking semantics.
The inspected docs layout has no analytics collector; playback measurement and
joined journeys are unverified. After separately authorized deployment, the
publishing owner should verify route coverage and cross-site continuity in the
existing analytics stack, without assuming it from the site's SDK alone.
