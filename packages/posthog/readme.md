# @submilli/posthog

A PostHog Cloud (US) client: bounded, fully paged event reads over HogQL,
event and property schema discovery, cohort calculation state, and event
capture. There is no raw-SQL operation: `listEvents` builds its own query, so
a blueprint sees, and can limit, the project, the event names, and every
property a query reads (selected or filtered).

| Capability | Operation | Fields |
| --- | --- | --- |
| `posthog.com/events.list` | `listEvents` | `projectId`, `events`, `properties`, `personProperties`, `fields`, `excludeCohortIds` |
| `posthog.com/properties.read` | `listEvents`, once per property path or field it reads | `projectId`, `scope` (`event`, `person` or `field`), `property` (a name, a nested path such as `first_touch.utm_source`, or a field such as `$virt_is_bot`) |
| `posthog.com/schema.read` | `listEventDefinitions`, `listPropertyDefinitions` | `projectId` |
| `posthog.com/cohorts.get` | `getCohort` | `projectId`, `cohortId` |
| `posthog.com/events.capture` | `capture` | `event` |

## Blueprint setup

```yaml
packages:
  - "@submilli/posthog"
secrets:
  POSTHOG_PERSONAL_API_KEY:
    store: posthog-personal-api-key
  POSTHOG_PROJECT_API_KEY:
    store: posthog-project-api-key
```

`submilli blueprint add-package @submilli/posthog --no-capabilities` adds the
package and the rules for its own calls. Then grant `main` only what it needs.
For example, a read-only analytics agent scoped to one project that may read
only an allowlist of properties. `listEvents` checks `posthog.com/properties.read`
once for every property a query selects or filters on, so an allowlist is a
plain filter on that capability:

```yaml
permissions:
  main:
    - capability: posthog.com/events.list
      filter: projectId == 602724
      action: allow
    - capability: posthog.com/properties.read
      filter: >-
        projectId == 602724 and scope == "event"
        and (property == "$host" or property == "$pathname" or property == "destination")
      action: allow
    - capability: posthog.com/schema.read
      filter: projectId == 602724
      action: allow
```

Declare only the secrets the granted operations use:

| Secret | Credential | Used by |
| --- | --- | --- |
| `POSTHOG_PERSONAL_API_KEY` | A personal API key scoped to the project, with Query Read, Event Definition Read, Property Definition Read and Cohort Read | `listEvents`, `listEventDefinitions`, `listPropertyDefinitions`, `getCohort` |
| `POSTHOG_PROJECT_API_KEY` | The project API key (`phc_…`), which can only send events | `capture` |

A missing secret throws `PostHogError` with code `missing_credentials`.

## Behavior notes

- Hosts: `us.posthog.com` (queries, schema, cohorts) and `us.i.posthog.com`
  (`POST /i/v0/e/`, capture). EU Cloud is not supported yet.
- PostHog refuses `OFFSET` for personal API keys, so `listEvents` pages by
  keyset on (millisecond timestamp, UUID) and fetches one extra row to know
  whether another page exists.
- Window bounds are written as `toDateTime('…', 'UTC')` for index pruning plus
  exact millisecond comparisons, so the project's time zone never shifts the
  window.
- Filter values are folded the way ClickHouse compares them: spaces trimmed,
  ASCII letters lower-cased. Windows must fall within 1970–2299.
- Every caller string placed in HogQL (event names, property names, filter
  values) is refused if it contains a quote, backslash, backtick or control
  character.
- PostHog's project limits apply: 240 queries a minute, 3 concurrent queries,
  and an hourly read budget (HTTP 429, code `budget_exceeded`).

## Development

```bash
submilli build test -p @submilli/posthog --skip-network
submilli build test -p @submilli/posthog --env-var POSTHOG_PERSONAL_API_KEY,POSTHOG_TEST_PROJECT_ID
```

The network test is read-only; it never calls `capture`.

## Policy tests

`tests/policy/read-scope.ts` runs as a real `main` caller under
`read-scope.yaml`, without a key or network. It shows that a rule can confine
`listEvents` to one project and allow only listed property paths and fields,
whether selected or filtered (allowing `first_touch.utm_source` allows neither
the whole `first_touch` object nor its other keys), that `getCohort` is scoped
by cohort id, and that `capture` stays denied when only reads are granted.
`cargo test -p submilli --test package_policy` runs it.
