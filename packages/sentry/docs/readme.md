# Sentry package guide

Use `@submilli/sentry` for typed Sentry Cloud issue investigation and triage.
The token stays inside the package as `SENTRY_AUTH_TOKEN`; never pass credentials
through function arguments.

- Organization arguments are Sentry slugs.
- Issue and event operations require a project slug plus a numeric group ID or
  short ID such as `API-42`. The declared project is verified against the issue.
- Continue list calls with the opaque `nextCursor` while `isComplete` is false.
- Repeated project and environment filters are encoded as repeated query parameters.
- Omit the issue `query` to keep Sentry's unresolved default; use an empty query to
  include every issue.
- `getIssueEvent` defaults to Sentry's `recommended` event and also accepts `latest`,
  `oldest`, or a concrete event ID.
- Event details expose exceptions, stack frames and source context, tags, user,
  breadcrumbs, request metadata, and a release summary. Unknown event entry types
  are ignored.
- `updateIssue` intentionally supports only status, substatus, assignment, and
  low/medium/high priority. Use `clearAssignee: true` to unassign explicitly.
- Project-scoped capability payloads contain only organization, project, and issue;
  mutation values such as status, assignee, and priority are deliberately omitted.
- Missing get resources return `null`; other failures throw `SentryError`.
- Inspect the retry and Sentry rate-limit fields before deciding whether to retry.
  The package does not retry automatically.

This package targets the global `sentry.io` API only. It deliberately has no raw
REST escape hatch and does not include Discover queries, releases, attachments,
comments, Seer, administration, telemetry ingestion, or Sentry's MCP server.

## Example

Unresolved issues for an organization, newest first.

```ts
import sentry from "@submilli/sentry";

function main(): string {
    const issues = sentry.listIssues("acme", { query: "is:unresolved", limit: 5 });
    if (issues.items.length === 0) return "No unresolved issues.";
    const lines: string[] = [];
    for (const issue of issues.items) {
        lines.push(issue.shortId + "  " + issue.title);
    }
    return lines.join("\n");
}
```
