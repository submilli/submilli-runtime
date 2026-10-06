# Video library

The docs video registry lives at `src/data/videos.json`. It contains the stable
film identity, series order, book destinations, and independent-host embed URL.
The docs repository deliberately contains no MP4, poster, caption, release, or
per-version metadata. The independent Render service owns playback, native
controls, and captions; captions start off and viewers can enable them with the
player's CC control.

The library is `/docs/videos/`. The registry keeps the five films ordered as
introduction, Challenges, Helps, Using, and Works; the library shows only published
films in that order. `VideoContent.astro` places the same stable
embeds at the registry's contextual book anchors. The standalone introduction
transcript remains available for agents and readers, but it points to the host's
watch URL instead of copying a duration or narration that can become stale.

When the host updates a cut, its existing `/embed/<film>/` and `/watch/<film>/`
URLs remain unchanged. A docs build therefore does not participate in video
publishing. Changes to book placement or film identity still belong in the docs
registry and should preserve the five-film order.

Only films with `status: "published"` are rendered or get embed redirects.
Set a film to `status: "hidden"` while its host is unavailable; this hides it
across the book and library without removing its registry entry. Using and Works
are temporarily hidden because their embed endpoints return 404. Restore their
published status after verifying playback on the independent host.

For local browser checks, use the live host URLs. Do not add local media files,
preview overrides, production source exports, or credentials to this repository.
