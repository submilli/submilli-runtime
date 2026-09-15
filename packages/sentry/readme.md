# @submilli/sentry

A synchronous, typed Sentry Cloud client for organizations, projects, issues,
events, and core issue triage.

## Blueprint setup

Add the package and bind a Sentry user auth token:

```yaml
packages:
  - "@submilli/sentry"

secrets:
  SENTRY_AUTH_TOKEN:
    harness:
      required: true
```

Create the token in Sentry's **User settings → Auth tokens**. Grant only the
scopes used by your program:

- `org:read` for organizations
- `project:read` for projects
- `event:read` for issues and events
- `event:write` for `updateIssue`

The package targets `https://sentry.io/api/0/`. It does not support self-hosted
Sentry or regional base URLs.

```ts
import { listIssues, getIssueEvent, updateIssue } from "@submilli/sentry";

function main(): string {
    const page = listIssues("acme", {
        projects: ["4500000000000000"],
        environments: ["production"],
        limit: 20,
    });
    if (page.items.length === 0) return "No unresolved issues";

    const issue = page.items[0];
    const event = getIssueEvent("acme", issue.project.slug, issue.shortId);
    if (event !== null && event.exceptions.length > 0) {
        updateIssue("acme", issue.project.slug, issue.id, { priority: "high" });
    }
    return JSON.stringify({ issue: issue, event: event, nextCursor: page.nextCursor });
}
```

List calls return an opaque `nextCursor`; pass it back as `cursor`. Sentry's
default issue query is unresolved issues, so omit `query` to keep that default
or pass `query: ""` to list all issues. Numeric group IDs and short IDs such as
`BACKEND-123` are accepted for issue and event operations. These operations also
require the expected project slug, which is verified before event access or mutation.

Missing resources return `null` from get operations. Other failures throw
`SentryError`, including request and rate-limit metadata. Calls are never
retried automatically.

## Development

```sh
submilli build test -p @submilli/sentry
```

Set `SENTRY_AUTH_TOKEN` and `SENTRY_TEST_ORGANIZATION` to enable live reads.
Live mutation coverage additionally requires `SENTRY_LIVE_MUTATIONS=true`,
`SENTRY_TEST_PROJECT`, and `SENTRY_TEST_ISSUE_ID`; use a disposable issue because
the test temporarily changes its priority before restoring it.
