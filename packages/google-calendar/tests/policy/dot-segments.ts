// Run through `submilli run --blueprint dot-segments.yaml`, not the allow-all build test runner.
import { EventDeleteOptions, deleteEvent } from "@submilli/google-calendar";

// `main` controls this object while the package reads it, and counts the reads of `calendarId`.
class CountingOptions implements EventDeleteOptions {
    private calendarReadCount: number = 0;

    get calendarId(): string {
        this.calendarReadCount += 1;
        return "team@group.calendar.google.com";
    }

    // `EventDeleteOptions` fields are writable, which a getter alone does not satisfy.
    set calendarId(value: string) {}

    calendarReads(): number {
        return this.calendarReadCount;
    }
}

function outcome(action: () => void): string {
    try {
        action();
        return "no error";
    } catch (error: PermissionDeniedError) {
        return error.caller + " denied at " + error.capability;
    } catch (error) {
        return error.name + ": " + error.message;
    }
}

function main(): void {
    // Without the refusal, `..` would remove `/events` and delete the calendar itself:
    // DELETE /calendar/v3/calendars/team@group.calendar.google.com.
    const parent = new CountingOptions();
    const refused = outcome(() => { deleteEvent("..", parent); });
    assert(refused.startsWith("TypeError: http DELETE: "), "an id of .. is refused before any request, got " + refused);
    assert(refused.includes("\"..\""), "the refusal names the segment, got " + refused);
    assert(parent.calendarReads() === 1, "calendarId is read once, got " + parent.calendarReads().toString());

    const encoded = outcome(() => { deleteEvent("%2E%2E", new CountingOptions()); });
    assert(encoded.startsWith("@submilli/google-calendar denied at http.delete"),
        "an escaped id is a name, not a segment, got " + encoded);

    // An ordinary id passes the refusal and stops at the package's http.delete denial.
    const ordinary = outcome(() => { deleteEvent("evt1", new CountingOptions()); });
    assert(ordinary === "@submilli/google-calendar denied at http.delete",
        "an ordinary id reaches the http policy, got " + ordinary);
}
