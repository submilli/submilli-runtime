# Linear

Use `@submilli/linear` to read or change issues, comments, teams, projects, and
users in a Linear workspace. Operations are synchronous and return plain typed
values; do not construct GraphQL queries yourself.

Read operations:

- `getViewer()` returns the user who owns the API token.
- `getIssue(id)` and `listIssues(filter, page)` fetch one issue or a filtered,
  paginated page of issues.
- `getTeam(id)`, `listTeams(page)`, `listProjects(page)`, and `listUsers(page)`
  fetch workspace reference data.

Write operations:

- `createIssue(input)` and `updateIssue(id, input)` create or change an issue.
  An update sends only fields explicitly set by the caller.
- `createComment(input)` adds a comment to an issue.

`updateIssue` and `createComment` read the issue first, so the policy check
carries the team it belongs to. An issue that does not exist throws before the
check.

Lists return a `Page<T>` with `nodes` and `pageInfo`. When
`pageInfo.hasNextPage` is true, pass `{ after: pageInfo.endCursor }` to request
the next page; a `null` cursor is treated as absent. The `page` argument is
optional; omit it to request the first 50 items. Always keep pagination
explicitly bounded.

`listIssues` accepts optional curated filters for team, assignee, workflow
state type, and lifecycle timestamp ranges; omit the filter to list all issues.
To find issues completed during a period, use `stateType: "completed"` together
with `completedAtAfter` and `completedAtBefore`. ISO timestamps from `Temporal` are accepted; a trailing
bracketed zone annotation is removed before calling Linear.

`Issue` includes lifecycle timestamps (`createdAt`, `updatedAt`, `completedAt`, `canceledAt`, `startedAt`, `triagedAt`, `archivedAt`, `autoClosedAt`), planning fields (`dueDate`, `estimate`, `priorityLabel`, `labelIds`), and embedded one-level relations (`assignee`, `creator`, `team`, `state`, `project`, `cycle`).

Credentials are supplied internally. Never request, accept, or pass an API key
in package calls. Each operation has its own `linear.app/<operation>`
capability, so a denied operation may require operator approval rather than a
different API call.

## Example

List in-progress issues for one team.

```ts
import linear from "@submilli/linear";

function main(): string {
    const teams = linear.listTeams({ first: 1 });
    if (teams.nodes.length === 0) return "No teams visible.";
    const team = teams.nodes[0];
    const issues = linear.listIssues({ teamId: team.id, stateType: "started" }, { first: 10 });
    if (issues.nodes.length === 0) return "Nothing in progress for " + team.name + ".";
    const lines: string[] = [];
    for (const issue of issues.nodes) {
        lines.push(issue.identifier + "  " + issue.title);
    }
    return lines.join("\n");
}
```

## Agent sessions

For an installed Linear agent, bind its app OAuth access token to `LINEAR_API_KEY`.
OAuth tokens are sent with `Bearer`; personal `lin_api_` keys remain raw. Agent
session mutations require the app identity that owns the session. Request
`actor=app` when authorizing; add `app:mentionable` and `app:assignable` if needed.

- `getAgentSession(id)` reads session status, links, plan, and issue/comment IDs.
- `listAgentActivities(id, page)` reads activity history, including user prompts
  and their signals. Pagination is explicit and bounded, just like issue lists.
- `createAgentActivity(input)` emits `thought`, `action`, `elicitation`, `response`,
  or `error`. A `response` marks work complete; an `elicitation` waits for input.
  Use `thought` for short progress updates, not private internal reasoning.
- Only `thought` and `action` may be `ephemeral`. Use `signal: "auth"` with an
  elicitation and `signalMetadata: { url, providerName }` for account linking, or
  `signal: "select"` and `signalMetadata: { options: [{ label, value }] }` for choices.
- `updateAgentSession(id, input)` updates links, summary, or the whole plan.
  `externalUrls` replaces all links; use `addedExternalUrls`/`removedExternalUrls`
  for incremental changes. Do not combine replacement and incremental fields.
- `createAgentSessionOnIssue({ issueId })` and
  `createAgentSessionOnComment({ commentId })` proactively create sessions without
  waiting for a mention or delegation. Both accept optional `externalUrls`.
- `listComments(issueId, page)` reads issue comments with parent thread IDs.
  `createComment({ issueId, parentId, body })` replies to a comment thread.

A webhook receiver must validate and deduplicate deliveries, acknowledge within
5 seconds, and route `created` to a new conversation and `prompted` to the existing
conversation keyed by Linear session ID. A new session needs an activity or
external URL update within 10 seconds. Handle a user `stop` signal in the harness:
interrupt ongoing work, then emit a final acknowledgement. These receiving,
routing, and cancellation responsibilities are outside this outbound API package.
Do not use an ordinary issue comment as a substitute for a session response.

Example: create a session and record a result. Calling this writes to Linear.

```ts
import linear from "@submilli/linear";

function main(): string {
    const issueId = "REPLACE_WITH_TEST_ISSUE_UUID";
    const session = linear.createAgentSessionOnIssue({ issueId: issueId });
    linear.createAgentActivity({
        agentSessionId: session.id,
        content: { type: "thought", body: "Checking the issue." },
        ephemeral: true,
    });
    const issue = linear.getIssue(issueId);
    linear.createAgentActivity({
        agentSessionId: session.id,
        content: {
            type: "response",
            body: issue === null ? "Issue not found." : "Reviewed " + issue.identifier,
        },
    });
    return session.id;
}
```
