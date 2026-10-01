# Google Calendar

Use `@submilli/google-calendar` to inspect calendars, build agendas, check
availability, find candidate meeting times, and manage events.

The API covers calendar and event reads, event creation/update/response/deletion,
free/busy queries, and bounded `agenda` and `findFreeTime` helpers. All list methods
return one explicit page and never paginate without a caller-selected bound.

The read surface is `listCalendars`, `listEvents`, `getEvent`, `agenda`,
`queryFreeBusy`, and `findFreeTime`. The write surface is `createEvent`,
`updateEvent`, `respondToEvent`, and `deleteEvent`. The default calendar is
`primary`; pass an explicit calendar ID when operating elsewhere.

An attendee's `email` is one bare address such as `dana@example.com`; it is
sent in lowercase. `sendUpdates` is `all`, `externalOnly`, or `none`, and
`none` when unset.

`updateEvent` is a patch: omitted fields remain unchanged. When the patch has
no `attendees`, the event is read first so the policy check covers the
attendees it already has; a missing event throws `not_found`. Follow the
declaration's explicit clear flags when intentionally removing optional values.
`respondToEvent` updates the authenticated attendee's response. Treat
`deleteEvent` as a deliberate destructive action even though deleting an
already absent event is idempotent.

Credentials are supplied internally. Never request, accept, or pass an access
token in package calls. Failures throw `CalendarError`; do not blindly retry
mutations when the outcome may already have been committed.

## Example

Today's agenda in the user's time zone. Note there is no `endOfDay()`: the
window ends at the next day's `startOfDay()`. `timeMin`/`timeMax` accept
`Temporal` strings directly — including the bracketed `toString()` form — and
are normalized to UTC. An event has either a timed `dateTime` or an all-day
`date`; always handle both.

```ts
import calendar from "@submilli/google-calendar";

function main(): string {
    const zone = "Asia/Jerusalem";
    const today = Temporal.Now.zonedDateTimeISO(zone).startOfDay();
    const result = calendar.agenda({
        timeMin: today.toString(),
        timeMax: today.add({ days: 1 }).toString(),
        timeZone: zone,
    });
    if (result.events.length === 0) return "No meetings today.";
    const lines: string[] = [];
    for (const entry of result.events) {
        const event = entry.event;
        const start = event.start.dateTime;
        const when = start !== null
            ? Temporal.Instant.from(start).toZonedDateTimeISO(zone).toPlainTime().toString()
            : "all day";
        lines.push(when + "  " + event.summary);
    }
    return lines.join("\n");
}
```
