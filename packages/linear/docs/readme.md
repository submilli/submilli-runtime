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

Lists return a `Page<T>` with `nodes` and `pageInfo`. When
`pageInfo.hasNextPage` is true, pass `{ after: pageInfo.endCursor }` to request
the next page. Always keep pagination explicitly bounded.

`listIssues` accepts curated filters for team, assignee, workflow state type,
and lifecycle timestamp ranges. To find issues completed during a period, use
`stateType: "completed"` together with `completedAtAfter` and
`completedAtBefore`. ISO timestamps from `Temporal` are accepted; a trailing
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
