# Video library

The docs video registry lives at `src/data/videos.json`. It contains the stable
film identity, series order, book destinations, and independent-host embed URL.
The docs repository deliberately contains no MP4, poster, caption, release, or
per-version metadata. The independent Render service owns playback, native
controls, and captions; captions start off and viewers can enable them with the
player's CC control.

The library is `/docs/videos/`. Its published films are ordered as introduction,
Challenges, Helps, Quickstart, How Submilli works, Packages, and Blueprints.
How Submilli works appears under the execution explanation on `/docs/server/`.
Packages and Blueprints also appear on
`/docs/packages/` and `/docs/blueprints/`, respectively.

`VideoContent.astro` places the same stable embeds at the registry's contextual book anchors. The standalone introduction
transcript remains available for agents and readers, but it points to the host's
watch URL instead of copying a duration or narration that can become stale.

When the host updates a cut, its existing `/embed/<film>/` and `/watch/<film>/`
URLs remain unchanged. A docs build therefore does not participate in video
publishing. Changes to book placement or film identity still belong in the docs
registry. Keep the registry limited to finalized, hosted films; its entries also
generate the library, chapter embeds, and legacy embed redirects.

The production prebuild checks every published player, manifest, media file, caption
file, and transcript. A missing or invalid dependency fails the build with the
film name. Deploy and verify the host release before merging a new registry entry.

Only films with `status: "published"` are rendered or get embed redirects.
Set a film to `status: "hidden"` while its host is unavailable; this hides it
across the book and library without removing its registry entry. Restore its
published status only after verifying playback on the independent host.

For local browser checks, use the live host URLs. Do not add local media files,
preview overrides, production source exports, or credentials to this repository.

Publish a newly approved film on the independent host before merging its docs
entry. Confirm the watch page, embed, media, captions, and transcript there. When
removing entries, use a clean docs deployment and verify that retired embed
redirects are no longer served; cached static files can outlive a normal build.

When withdrawing a film, remove its docs entry and deploy the docs before removing
its hosted player or media. This avoids leaving a published page with a broken embed.
