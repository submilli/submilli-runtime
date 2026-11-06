# PostHog package guide

Use `@submilli/posthog` to read events from a PostHog Cloud (US) project in
bounded time windows, inspect the project's event and property schema, read a
cohort's calculation state, and send events. Credentials are supplied
internally; never pass a key as an argument.

- `projectId` is PostHog's numeric project id, such as `602724`.
- Failures throw `PostHogError` with a `code`: `invalid_input` for an argument
  the package refused before any request, `missing_credentials`,
  `budget_exceeded` when PostHog's hourly query budget is spent (see
  `retryAfterSeconds`), `http_<status>` for other HTTP failures, and
  `invalid_response`. The package does not retry.
- A `PermissionDeniedError` means the blueprint forbids that call (for example
  another project, or a property it does not allow), not that it was mis-called.

<!-- submilli:if capability="posthog.com/events.list" -->

## Read events in a window

`listEvents(projectId, options)` returns one page of rows, oldest first, for
the named `events` with `after <= timestamp < before`. Times are ISO-8601
instants with Z or a ±HH:MM offset, such as `Temporal.Now.instant().toString()`;
a bound finer than a millisecond rounds up to the next one.

- Name every property you need in `properties` (event) or `personProperties`
  (person). Rows carry exactly those, in request order, as text or `null`. There
  is no way to select all properties.
- `filters` keep (`in`) or drop (`notIn`) rows by property value. For a
  property that holds an object, `key` names the field inside it, such as
  `{ property: "first_touch", key: "utm_source" }`. Comparison trims spaces and
  ignores ASCII case; values must not be blank. A missing property compares as
  `""`, so `notIn` keeps rows that lack the property. A filtered property counts
  as read, the same as a selected one; a nested filter reads only its path
  (`first_touch.utm_source`). The blueprint may allow only some properties: a
  denial names the property it refused.
- `fields` returns PostHog's own classification columns, which live on the
  event rather than in its properties: `$virt_is_bot`, `$virt_traffic_type`,
  `$virt_traffic_category`, `$virt_bot_name`. Rows carry them in `fields`.
- `excludeCohortIds` drops events from members of those cohorts.
- Pages are `limit` rows (1–1000, default 100). While `isComplete` is false,
  call again with the same options and `cursor: page.nextCursor`. A window is
  fully read only once a page reports `isComplete`. Do not advance your own
  checkpoint past `before` until then.
- Each row has `uuid`, `event`, `timestamp`, and `url`, the event's page in
  the PostHog app.

<!-- submilli:endif -->

<!-- submilli:if capability="posthog.com/schema.read" -->

## Inspect the schema

`listEventDefinitions(projectId, { search, limit, offset })` lists event names
PostHog has seen, with `lastSeenAt`. `listPropertyDefinitions(projectId, {
type, eventNames, search, limit, offset })` lists `event` or `person` property
names with their `propertyType`. Continue with `offset: page.nextOffset` until
it is `null`. Check that a property exists before relying on it: a property
that is missing from the schema reads as `null` on every row, which is not the
same as a confirmed empty value.

<!-- submilli:endif -->

<!-- submilli:if capability="posthog.com/cohorts.get" -->

## Read a cohort

`getCohort(projectId, cohortId)` returns the cohort's `name`, `count`,
`isCalculating`, `lastCalculation`, `errorsCalculating`, and `deleted`, or
`null` when it does not exist. A count of 0 says the cohort matched nobody at
its last calculation; it does not prove which people it should contain.

<!-- submilli:endif -->

<!-- submilli:if capability="posthog.com/events.capture" -->

## Send an event

`capture({ event, distinctId, properties, set, timestamp, uuid })` sends one
event to the project the configured project key belongs to. `properties` and
`set` (person properties) are lists of `{ name, value }`. Pass a stable `uuid`
so PostHog can deduplicate a retried send. `accepted: true` means PostHog
accepted the request; ingestion happens later.

<!-- submilli:endif -->

<!-- submilli:if capability="posthog.com/events.list" -->

## Example

Every link click on the website in one window, read to completion.

```ts
import { listEvents, EventRow } from "@submilli/posthog";

function main(): string {
    const rows: EventRow[] = [];
    let cursor = "";
    while (true) {
        const page = listEvents(602724, {
            events: ["link_clicked"],
            after: "2026-10-24T06:00:00Z",
            before: "2026-10-24T08:00:00Z",
            properties: ["$host", "destination"],
            filters: [{ property: "$host", operator: "in", values: ["submilli.ai"] }],
            limit: 200,
            cursor: cursor,
        });
        for (const row of page.events) rows.push(row);
        if (page.isComplete) break;
        cursor = page.nextCursor;
    }
    const lines: string[] = [];
    for (const row of rows) {
        lines.push(row.timestamp + " " + (row.properties[1].value ?? "(no destination)"));
    }
    return lines.length.toString() + " clicks\n" + lines.join("\n");
}
```

<!-- submilli:endif -->
