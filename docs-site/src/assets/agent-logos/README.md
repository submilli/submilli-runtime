Claude Code and Codex SVG marks from [Lobe Icons](https://github.com/lobehub/lobe-icons/tree/master/packages/static-svg/icons), retrieved 2026-10-04 (`claudecode.svg` and `codex.svg`). MIT license included. Brand marks belong to their respective owners. Used to identify app launch destinations.


Cursor mark from the same Lobe Icons library (`cursor.svg`), retrieved 2026-10-19.

Prompt handoff sources (checked 2026-10-19):
- Codex: OpenAI's [Promptfoo guide](https://developers.openai.com/api/docs/guides/promptfoo-codex/) uses `codex://threads/new?prompt=…`.
- Claude: [Desktop deep links](https://support.claude.com/en/articles/14729294-open-claude-desktop-with-a-link) documents `claude://code/new?q=…` and prefilled composers.
- Cursor: [Deeplinks](https://cursor.com/docs/reference/deeplinks) documents `cursor://anysphere.cursor-deeplink/prompt?text=…`, review before execution, and the 10,000-character URL limit.

Direct app links depend on an installed handler. Copy prompt works with other
clients. No link submits a prompt automatically. The renderer keeps encoded
links within 10,000 characters; longer prompts retain copy access.
