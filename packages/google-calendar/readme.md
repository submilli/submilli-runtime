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

## Policy tests

The scripts in `tests/policy/` run as a real `main` caller under restricted
blueprints of the same name, without a token or network:

- `attendees.ts` shows that `createEvent` and `updateEvent` are held to rules
  on `attendees` and `sendUpdates`, in whichever case an address is written,
  and that `deleteEvent` is held to a rule on `sendUpdates`.

`cargo test -p submilli --test package_policy` runs them.

## Notification recipients on updates

`updateEvent` still checks post-update addresses as `attendees`. It additionally
checks `removedAttendees` and `notificationRecipients`. For `all` and
`externalOnly`, it reads current attendees and includes the union of current and
replacement addresses in `notificationRecipients`; removed addresses appear in
`removedAttendees`. This conservatively covers `externalOnly` without guessing
which attendees Google considers internal. With `none`, both notification lists
are empty. Replacements with `none` need no metadata read; omitted attendees still
resolve current attendees. Only attendee emails and the omission flag are requested before the check. A
denial, failed lookup, or incomplete attendee list prevents the patch.

```sh
node --test packages/google-calendar/scripts/contract.test.mjs
```
