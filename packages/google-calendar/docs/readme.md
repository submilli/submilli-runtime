# Google Calendar

Use `@submilli/google-calendar` to:

<!-- submilli:if any="submilli/google-calendar.listCalendars submilli/google-calendar.listEvents submilli/google-calendar.getEvent" -->
- inspect calendars
<!-- submilli:endif -->
<!-- submilli:if capability="submilli/google-calendar.agenda" -->
- build agendas
<!-- submilli:endif -->
<!-- submilli:if capability="submilli/google-calendar.queryFreeBusy" -->
- check availability
<!-- submilli:endif -->
<!-- submilli:if capability="submilli/google-calendar.findFreeTime" -->
- find candidate meeting times
<!-- submilli:endif -->
<!-- submilli:if any="submilli/google-calendar.createEvent submilli/google-calendar.updateEvent submilli/google-calendar.respondToEvent submilli/google-calendar.deleteEvent" -->
- manage events
<!-- submilli:endif -->

<!-- submilli:if any="submilli/google-calendar.listCalendars submilli/google-calendar.listEvents" -->

All list methods return one explicit page and never paginate without a
caller-selected bound.

<!-- submilli:endif -->

The default calendar is `primary`; pass an explicit calendar ID when operating
elsewhere.

<!-- submilli:if any="submilli/google-calendar.createEvent submilli/google-calendar.updateEvent submilli/google-calendar.deleteEvent" -->

An attendee's `email` is one bare address such as `dana@example.com`; it is
sent in lowercase. `sendUpdates` is `all`, `externalOnly`, or `none`, and
`none` when unset.

<!-- submilli:endif -->

<!-- submilli:if capability="submilli/google-calendar.updateEvent" -->

`updateEvent` is a patch: omitted fields remain unchanged. When the patch has
no `attendees`, the event is read first so the policy check covers the
attendees it already has; a missing event throws `not_found`. Follow the
declaration's explicit clear flags when intentionally removing optional values.

<!-- submilli:endif -->

<!-- submilli:if capability="submilli/google-calendar.respondToEvent" -->

`respondToEvent` updates the authenticated attendee's response.

<!-- submilli:endif -->

<!-- submilli:if capability="submilli/google-calendar.deleteEvent" -->

Treat `deleteEvent` as a deliberate destructive action even though deleting an
already absent event is idempotent.

<!-- submilli:endif -->

Credentials are supplied internally. Never request, accept, or pass an access
token in package calls. Failures throw `CalendarError`.

<!-- submilli:if any="submilli/google-calendar.createEvent submilli/google-calendar.updateEvent submilli/google-calendar.respondToEvent submilli/google-calendar.deleteEvent" -->

Do not blindly retry mutations when the outcome may already have been
committed.

<!-- submilli:endif -->

<!-- submilli:if capability="submilli/google-calendar.agenda" -->

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
        const when = start !== undefined
            ? Temporal.Instant.from(start).toZonedDateTimeISO(zone).toPlainTime().toString()
            : "all day";
        lines.push(when + "  " + event.summary);
    }
    return lines.join("\n");
}
```

<!-- submilli:endif -->
