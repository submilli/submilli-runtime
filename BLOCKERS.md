# Unit 1 (growth monitor): blockers and open questions for Ehud

Branch `claude/growth-monitor-agent` in submilli-runtime (packages) and
submilli-private (agent definition). Nothing was published, deployed, or sent.
The Codex automations in ~/.codex are untouched and still running.

## Needs Ehud: credentials (names only; values go in the runtime server's secret store)

1. `growth-monitor-posthog-personal-key` → `POSTHOG_PERSONAL_API_KEY`: a PostHog
   personal API key scoped to project 602724 with Query Read, Event Definition
   Read, Property Definition Read, Cohort Read. Also needed to run the PostHog
   package's live test (`POSTHOG_TEST_PROJECT_ID=602724`), which has not run:
   the HogQL functions it uses (`toUnixTimestamp64Milli`, bracket property
   access, `toDateTime(…, 'UTC')`) are verified against docs only.
2. `submilli-growth-slack-bot-token` → `SLACK_BOT_TOKEN`: a bot token for the
   Submilli Growth app (A0CABHXEXU0) with `channels:history` (and `chat:write`
   at cutover). Today's sender is an incoming webhook, which @submilli/slack-bot
   cannot use and which cannot read the channel back.
3. `growth-monitor-github-token` → `GITHUB_TOKEN`: fine-grained, read-only,
   public repositories of the submilli org.
4. `scrapecreators-api-key` → `SCRAPECREATORS_API_KEY` (an existing key may be reusable).

## Needs Ehud: decisions

5. **Repo home** for the launch-monitor code and this agent folder. Nothing was
   moved; `agents/growth-monitor/` in submilli-private holds only what the
   agent definition needs and can move wholesale.
6. **Scheduler.** The agent server has no scheduler. `schedules.yaml` specifies
   three triggers (`POST /v1/agents/growth-monitor/runs`). Options reusing
   existing tools: a Render cron job, or an Apps Script time trigger like the
   demo-booking scanner.
7. **Runtime server operator steps:** declare the named volume
   `growth-monitor-state`; install the new package (`@submilli/posthog`) after the runtime PR merges.
8. **Cutover** (shadow → live): see RUNBOOK.md checklist in the private PR.
9. **Stars and forks by account.** `@submilli/github` has no repository
   events, stargazers or forks listing, so star/fork alerts for identified
   accounts are not covered. Proposed follow-up: read-only `events.list`,
   `stargazers.list`, `forks.list` in `@submilli/github`. Approve?
10. **Docs authorship hash.** Adding `@submilli/posthog` to
    `docs/part-6-reference/curated-packages.md` changed the page, so its
    confirmed-authorship `contentHash` was refreshed (as in #318). Please
    re-confirm if that attestation should be yours.
11. **Model** for the agent: `anthropic/claude-sonnet-5` is a placeholder.
12. **X Latest:** ScrapeCreators' X search has no sort option; confirm with a
    live run that it returns latest posts.

Draft PRs: https://github.com/submilli/submilli-runtime/pull/322 (packages), https://github.com/submilli/submilli-private/pull/157 (agent).

## Resolved

13. **Hacker News package dropped (2026-11-03).** `@submilli/hacker-news` was
    a thin wrapper over the public, keyless HN Search API, so it is removed
    from the runtime PR. The agent calls `hn.algolia.com` directly from its
    program under one blueprint rule (`main`, `http.get`, host
    `hn.algolia.com`, paths `/api/v1/search_by_date` and `/api/v1/items/<numeric id>`).
    The system prompt carries the search and thread code with the same
    defaults (newest first, `typoTolerance=false`, `queryType=prefixNone`,
    thread flattened parent before children). The code was run live under
    that rule alone. Report: ~/submilli/migration/hn-check-report.md.
