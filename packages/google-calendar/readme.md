# @submilli/google-calendar

Synchronous Google Calendar helpers for calendars, events, free/busy queries,
agendas, and bounded free-time searches.

## Google and blueprint setup

Enable the Google Calendar API for the OAuth client. For the full package
surface, request:

```text
https://www.googleapis.com/auth/calendar
```

Read-only deployments can use
`https://www.googleapis.com/auth/calendar.readonly`. Google also offers
resource-specific scopes, such as `calendar.events` and `calendar.freebusy`,
when the deployment exposes only those operations.

Declare the package and harness-bound access token:

```yaml
packages:
  - "@submilli/google-calendar"

secrets:
  GOOGLE_ACCESS_TOKEN:
    harness:
      required: true
```

Bind the OAuth access token as `GOOGLE_ACCESS_TOKEN` when creating the harness
session. The package does not refresh tokens, so the harness must replace
expired tokens. It reads the token through `submilli:secrets`; no `auth_proxy`
rule is needed.

Run `submilli blueprint add-package @submilli/google-calendar` to scaffold the
package's permissions, then review the generated operation grants.

## Development

Run:

```bash
submilli build test -p @submilli/google-calendar
```

Live reads run when `.env` contains `GOOGLE_ACCESS_TOKEN`. Temporary event
creation and cleanup additionally require `GOOGLE_LIVE_MUTATIONS=true`.
