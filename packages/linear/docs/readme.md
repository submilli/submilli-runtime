# Linear

Use `@submilli/linear` to work with a Linear workspace. Operations are
synchronous and return plain typed values; do not construct GraphQL queries
yourself.

<!-- submilli:if any="linear.app/getViewer linear.app/getIssue linear.app/listIssues linear.app/getTeam linear.app/listTeams linear.app/listProjects linear.app/listUsers" -->

Read operations:

<!-- submilli:endif -->

<!-- submilli:if capability="linear.app/getViewer" -->

- `getViewer()` returns the user who owns the API token.

<!-- submilli:endif -->

<!-- submilli:if capability="linear.app/getIssue" -->
- `getIssue(id)` fetches one issue.
<!-- submilli:endif -->
<!-- submilli:if capability="linear.app/listIssues" -->
- `listIssues(filter, page)` fetches a filtered, paginated page of issues.
<!-- submilli:endif -->
<!-- submilli:if capability="linear.app/getTeam" -->
- `getTeam(id)` fetches one team.
<!-- submilli:endif -->
<!-- submilli:if capability="linear.app/listTeams" -->
- `listTeams(page)` fetches a page of teams.
<!-- submilli:endif -->
<!-- submilli:if capability="linear.app/listProjects" -->
- `listProjects(page)` fetches a page of projects.
<!-- submilli:endif -->
<!-- submilli:if capability="linear.app/listUsers" -->
- `listUsers(page)` fetches a page of users.
<!-- submilli:endif -->

<!-- submilli:if any="linear.app/createIssue linear.app/updateIssue linear.app/createComment" -->

Write operations:

<!-- submilli:endif -->

<!-- submilli:if capability="linear.app/createIssue" -->
- `createIssue(input)` creates an issue.
<!-- submilli:endif -->
<!-- submilli:if capability="linear.app/updateIssue" -->
- `updateIssue(id, input)` changes an issue. An update sends only fields
  explicitly set by the caller.
<!-- submilli:endif -->

<!-- submilli:if capability="linear.app/createComment" -->

- `createComment(input)` adds a comment to an issue.

<!-- submilli:endif -->

<!-- submilli:if any="linear.app/updateIssue linear.app/createComment" -->

Writes to an existing issue read the issue first, so the policy check carries
the team it belongs to. An issue that does not exist throws before the
check.

<!-- submilli:endif -->

<!-- submilli:if any="linear.app/listIssues linear.app/listTeams linear.app/listProjects linear.app/listUsers linear.app/listComments linear.app/listAgentActivities" -->

Lists return a `Page<T>` with `nodes` and `pageInfo`. When
`pageInfo.hasNextPage` is true, pass `{ after: pageInfo.endCursor }` to request
the next page; a `null` cursor is treated as absent. The `page` argument is
optional; omit it to request the first 50 items. Always keep pagination
explicitly bounded.

<!-- submilli:endif -->

<!-- submilli:if capability="linear.app/listIssues" -->

`listIssues` accepts optional curated filters for team, assignee, workflow
state type, and lifecycle timestamp ranges; omit the filter to list all issues.
To find issues completed during a period, use `stateType: "completed"` together
with `completedAtAfter` and `completedAtBefore`. ISO timestamps from `Temporal` are accepted; a trailing
bracketed zone annotation is removed before calling Linear.

<!-- submilli:endif -->

<!-- submilli:if any="linear.app/getIssue linear.app/listIssues linear.app/createIssue linear.app/updateIssue" -->

`Issue` includes lifecycle timestamps (`createdAt`, `updatedAt`, `completedAt`, `canceledAt`, `startedAt`, `triagedAt`, `archivedAt`, `autoClosedAt`), planning fields (`dueDate`, `estimate`, `priorityLabel`, `labelIds`), and embedded one-level relations (`assignee`, `creator`, `team`, `state`, `project`, `cycle`).

<!-- submilli:endif -->

Credentials are supplied internally. Never request, accept, or pass an API key
in package calls. Each operation has its own `linear.app/<operation>`
capability, so a permission denial means the operation is forbidden, not
mis-called.

<!-- submilli:if all="linear.app/listTeams linear.app/listIssues" -->

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

<!-- submilli:endif -->

<!-- submilli:if any="linear.app/getAgentSession linear.app/listAgentActivities linear.app/createAgentActivity linear.app/updateAgentSession linear.app/createAgentSessionOnIssue linear.app/createAgentSessionOnComment linear.app/listComments" -->

## Agent sessions

<!-- submilli:if any="linear.app/getAgentSession linear.app/listAgentActivities linear.app/createAgentActivity linear.app/updateAgentSession linear.app/createAgentSessionOnIssue linear.app/createAgentSessionOnComment" -->

Agent session mutations require the app identity that owns the session.

<!-- submilli:endif -->

<!-- submilli:if capability="linear.app/getAgentSession" -->

- `getAgentSession(id)` reads session status, links, plan, and issue/comment IDs.

<!-- submilli:endif -->

<!-- submilli:if capability="linear.app/listAgentActivities" -->

- `listAgentActivities(id, page)` reads activity history, including user prompts
  and their signals. Pagination is explicit and bounded, just like issue lists.

<!-- submilli:endif -->

<!-- submilli:if capability="linear.app/createAgentActivity" -->

- `createAgentActivity(input)` emits `thought`, `action`, `elicitation`, `response`,
  or `error`. A `response` marks work complete; an `elicitation` waits for input.
  Use `thought` for short progress updates, not private internal reasoning.
- Only `thought` and `action` may be `ephemeral`. Use `signal: "auth"` with an
  elicitation and `signalMetadata: { url, providerName }` for account linking, or
  `signal: "select"` and `signalMetadata: { options: [{ label, value }] }` for choices.

<!-- submilli:endif -->

<!-- submilli:if capability="linear.app/updateAgentSession" -->

- `updateAgentSession(id, input)` updates links, summary, or the whole plan.
  `externalUrls` replaces all links; use `addedExternalUrls`/`removedExternalUrls`
  for incremental changes. Do not combine replacement and incremental fields.

<!-- submilli:endif -->

<!-- submilli:if capability="linear.app/createAgentSessionOnIssue" -->
- `createAgentSessionOnIssue({ issueId })` proactively creates a session on an
  issue without waiting for a mention or delegation. It accepts optional
  `externalUrls`.
<!-- submilli:endif -->
<!-- submilli:if capability="linear.app/createAgentSessionOnComment" -->
- `createAgentSessionOnComment({ commentId })` proactively creates a session on a
  comment without waiting for a mention or delegation. It accepts optional
  `externalUrls`.
<!-- submilli:endif -->

<!-- submilli:if capability="linear.app/listComments" -->

- `listComments(issueId, page)` reads issue comments with parent thread IDs.

<!-- submilli:endif -->

<!-- submilli:if capability="linear.app/createComment" -->

- `createComment({ issueId, parentId, body })` replies to a comment thread.

<!-- submilli:endif -->

<!-- submilli:if any="linear.app/getAgentSession linear.app/listAgentActivities linear.app/createAgentActivity linear.app/updateAgentSession linear.app/createAgentSessionOnIssue linear.app/createAgentSessionOnComment" -->

A webhook receiver must validate and deduplicate deliveries, acknowledge within
5 seconds, and route `created` to a new conversation and `prompted` to the existing
conversation keyed by Linear session ID. A new session needs an activity or
external URL update within 10 seconds. Handle a user `stop` signal in the harness:
interrupt ongoing work, then emit a final acknowledgement. These receiving,
routing, and cancellation responsibilities are outside this outbound API package.
Do not use an ordinary issue comment as a substitute for a session response.

<!-- submilli:endif -->

<!-- submilli:if all="linear.app/createAgentSessionOnIssue linear.app/createAgentActivity linear.app/getIssue" -->

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

<!-- submilli:endif -->

<!-- submilli:endif -->
