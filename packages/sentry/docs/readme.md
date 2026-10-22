# Sentry package guide

Use `@submilli/sentry` for typed Sentry Cloud issue investigation and triage.
Credentials are supplied internally; never pass credentials through function
arguments.

- Organization arguments are Sentry slugs.

<!-- submilli:if any="sentry.io/issues.get sentry.io/issueEvents.list sentry.io/issueEvents.get sentry.io/issues.update" -->

- Issue and event operations require a project slug plus a numeric group ID or
  short ID such as `API-42`. The declared project is verified against the issue.

<!-- submilli:endif -->

<!-- submilli:if any="sentry.io/organizations.list sentry.io/projects.list sentry.io/issues.list sentry.io/issueEvents.list" -->

- Continue list calls with the opaque `nextCursor` while `isComplete` is false.

<!-- submilli:endif -->

<!-- submilli:if any="sentry.io/issues.list sentry.io/issueEvents.list" -->

- Repeated project and environment filters are encoded as repeated query parameters.

<!-- submilli:endif -->

<!-- submilli:if capability="sentry.io/issues.list" -->

- Omit the issue `query` to keep Sentry's unresolved default; use an empty query to
  include every issue.

<!-- submilli:endif -->

<!-- submilli:if capability="sentry.io/issueEvents.get" -->

- `getIssueEvent` defaults to Sentry's `recommended` event and also accepts `latest`,
  `oldest`, or a concrete event ID.
- Event details expose exceptions, stack frames and source context, tags, user,
  breadcrumbs, request metadata, and a release summary. Unknown event entry types
  are ignored.

<!-- submilli:endif -->

<!-- submilli:if capability="sentry.io/issues.update" -->

- `updateIssue` intentionally supports only status, substatus, assignment, and
  low/medium/high priority. Use `clearAssignee: true` to unassign explicitly.

<!-- submilli:endif -->

- Missing get resources return `null`; other failures throw `SentryError`.
- Inspect the retry and Sentry rate-limit fields before deciding whether to retry.
  The package does not retry automatically.

This package targets the global `sentry.io` API only. It deliberately has no raw
REST escape hatch and does not include Discover queries, releases, attachments,
comments, Seer, administration, telemetry ingestion, or Sentry's MCP server.

<!-- submilli:if capability="sentry.io/issues.list" -->

## Example

Unresolved issues for an organization, newest first.

```ts
import sentry from "@submilli/sentry";

function main(): string {
    const issues = sentry.listIssues("pied-piper", { query: "is:unresolved", limit: 5 });
    if (issues.items.length === 0) return "No unresolved issues.";
    const lines: string[] = [];
    for (const issue of issues.items) {
        lines.push(issue.shortId + "  " + issue.title);
    }
    return lines.join("\n");
}
```

<!-- submilli:endif -->
