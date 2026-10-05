// Run through `submilli run --blueprint attendees.yaml`, not the allow-all build test runner.
import { Attendee, CalendarError, EventCreateInput, createEvent, deleteEvent, updateEvent } from "@submilli/google-calendar";

// `main` controls this object while the package reads it. `email` answers an allowed address to
// its first read and the blocked one to every later read, so a package that reads it for `check`
// and again for the request approves one attendee and invites another.
class FlippingAttendee implements Attendee {
    private emailReadCount: number = 0;

    get email(): string {
        this.emailReadCount += 1;
        return this.emailReadCount === 1 ? "allowed@example.com" : "blocked@example.com";
    }

    // `Attendee` fields are writable, which a getter alone does not satisfy.
    set email(value: string) {}

    emailReads(): number {
        return this.emailReadCount;
    }
}

function deniedAt(capability: string, caller: string, action: () => void): void {
    let outcome = "no error";
    try {
        action();
    } catch (error: PermissionDeniedError) {
        outcome = error.caller + " denied at " + error.capability;
    } catch (error) {
        outcome = error.name + ": " + error.message;
    }
    const expected = caller + " denied at " + capability;
    assert(outcome === expected, "expected " + expected + ", got " + outcome);
}

function reachesCredentialBoundary(action: () => void): void {
    // This denial proves the attendee check passed, without reading a token or calling Calendar.
    deniedAt("secrets.get", "@submilli/google-calendar", action);
}

function rejectedAs(code: string, action: () => void): void {
    let outcome = "no error";
    try {
        action();
    } catch (error: CalendarError) {
        outcome = error.code;
    } catch (error) {
        outcome = error.name + ": " + error.message;
    }
    assert(outcome === code, "expected " + code + ", got " + outcome);
}

function meeting(attendees: Attendee[], sendUpdates: string | null): EventCreateInput {
    const input: EventCreateInput = {
        summary: "Review",
        start: { dateTime: "2026-10-05T10:00:00Z" },
        end: { dateTime: "2026-10-05T10:30:00Z" },
        attendees: attendees,
    };
    if (sendUpdates !== null) input.sendUpdates = sendUpdates;
    return input;
}

function attendeeRuleHolds(capability: string, call: (attendees: Attendee[], sendUpdates: string | null) => void, readsCurrentAttendees: boolean = false): void {
    reachesCredentialBoundary(() => { call([{ email: "allowed@example.com" }, { email: "second@example.com" }], "none"); });
    // An unset `sendUpdates` is checked as "none", which is what Calendar does with it.
    reachesCredentialBoundary(() => { call([{ email: "allowed@example.com" }], null); });
    deniedAt(capability, "main", () => { call([{ email: "allowed@example.com" }, { email: "blocked@example.com" }], "none"); });
    // Policy reads the address in one spelling, however the caller wrote it.
    for (const spelling of ["Blocked@Example.com", "BLOCKED@EXAMPLE.COM", "blocked@example.com."]) {
        deniedAt(capability, "main", () => { call([{ email: spelling }], "none"); });
    }
    // Who is emailed is policy-visible too: this blueprint allows no invitation email.
    if (readsCurrentAttendees) {
        // Notification updates resolve prior attendees before the business check.
        // Offline contract tests cover denial after that metadata read.
        reachesCredentialBoundary(() => { call([{ email: "allowed@example.com" }], "all"); });
        reachesCredentialBoundary(() => { call([{ email: "allowed@example.com" }], "externalOnly"); });
    } else {
        deniedAt(capability, "main", () => { call([{ email: "allowed@example.com" }], "all"); });
        deniedAt(capability, "main", () => { call([{ email: "allowed@example.com" }], "externalOnly"); });
    }
    rejectedAs("invalid_send_updates", () => { call([{ email: "allowed@example.com" }], "everyone"); });
    // One entry is one address.
    for (const entry of ["allowed@example.com, blocked@example.com", "Allowed <blocked@example.com>", " allowed@example.com", "allowed", ""]) {
        rejectedAs("invalid_attendee", () => { call([{ email: entry }], "none"); });
    }

    const flipping = new FlippingAttendee();
    reachesCredentialBoundary(() => { call([flipping], "none"); });
    assert(flipping.emailReads() === 1, capability + " read email " + flipping.emailReads().toString() + " times, expected 1");
}

function main(): string {
    attendeeRuleHolds("submilli/google-calendar.createEvent", (attendees: Attendee[], sendUpdates: string | null): void => {
        createEvent(meeting(attendees, sendUpdates));
    });
    attendeeRuleHolds("submilli/google-calendar.updateEvent", (attendees: Attendee[], sendUpdates: string | null): void => {
        if (sendUpdates === null) updateEvent("event1", { attendees: attendees });
        else updateEvent("event1", { attendees: attendees, sendUpdates: sendUpdates });
    }, true);
    // An event created without attendees has an empty list, which a deny-list allows.
    reachesCredentialBoundary(() => {
        createEvent({ summary: "Focus", start: { dateTime: "2026-10-05T10:00:00Z" }, end: { dateTime: "2026-10-05T10:30:00Z" } });
    });

    reachesCredentialBoundary(() => { deleteEvent("event1"); });
    reachesCredentialBoundary(() => { deleteEvent("event1", { sendUpdates: "none" }); });
    deniedAt("submilli/google-calendar.deleteEvent", "main", () => { deleteEvent("event1", { sendUpdates: "all" }); });
    rejectedAs("invalid_send_updates", () => { deleteEvent("event1", { sendUpdates: "everyone" }); });
    return "attendee capability checks passed";
}
