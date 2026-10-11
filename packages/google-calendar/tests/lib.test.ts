import { label } from "submilli:test";
import {
    CalendarError,
    EventCreateInput,
    EventUpdateInput,
    FreeBusyInput,
    AgendaOptions,
    queryFreeBusy,
    createEvent,
    listEvents,
} from "@submilli/google-calendar";

function main(): void {
    label("CalendarError preserves stable fields");
    const error = new CalendarError("rateLimitExceeded", "try later", 429);
    assert(error.code === "rateLimitExceeded", "code is preserved");
    assert(error.status === 429, "status is preserved");

    label("public event and helper inputs are structural");
    const create: EventCreateInput = {
        summary: "Planning",
        start: { dateTime: "2026-07-23T10:00:00Z" },
        end: { dateTime: "2026-07-23T10:30:00Z" },
        attendees: [{ email: "person@example.com" }],
        createGoogleMeet: true,
    };
    const update: EventUpdateInput = { clearDescription: true, attendees: [] };
    const freeBusy: FreeBusyInput = {
        calendarIds: ["primary"],
        timeMin: "2026-07-23T00:00:00Z",
        timeMax: "2026-07-24T00:00:00Z",
    };
    const agenda: AgendaOptions = {
        timeMin: freeBusy.timeMin,
        timeMax: freeBusy.timeMax,
        maxCalendars: 3,
    };
    assert(create.createGoogleMeet === true, "meet creation is represented");
    assert(update.clearDescription === true, "clear flags explicitly remove text");
    assert(freeBusy.calendarIds.length === 1, "free/busy requires explicit calendars");
    assert(agenda.maxCalendars === 3, "agenda is bounded");

    label("junk timestamps are rejected client-side with the parameter named");
    assert(
        errorCode(() => {
            queryFreeBusy({
                calendarIds: ["primary"],
                timeMin: "2026-08-11",
                timeMax: "2026-08-12T00:00:00Z",
            });
        }) === "invalid_timestamp:timeMin",
        "date-only timeMin is rejected before any request",
    );
    assert(
        errorCode(() => {
            createEvent({
                summary: "Planning",
                start: { dateTime: "not a timestamp" },
                end: { dateTime: "2026-08-11T10:30:00Z" },
            });
        }) === "invalid_timestamp:dateTime",
        "junk event dateTime is rejected before any request",
    );

    label("bracketed Temporal timestamps are not rejected as invalid");
    const bracketed = errorCode(() => {
        queryFreeBusy({
            calendarIds: ["primary"],
            timeMin: "2026-08-11T00:00:00-07:00[America/Los_Angeles]",
            timeMax: "2026-08-12T00:00:00-07:00[America/Los_Angeles]",
        });
    });
    assert(bracketed !== "invalid_timestamp:timeMin", "bracketed bounds pass normalization");

    label("an unbound access token is reported before any request");
    assert(errorCode(() => { listEvents(); }) === "missing_token", "no token");
}

function errorCode(operation: () => void): string {
    try {
        operation();
    } catch (e) {
        if (e instanceof CalendarError) {
            if (e.message.indexOf("timeMin") >= 0) return e.code + ":timeMin";
            if (e.message.indexOf("dateTime") >= 0) return e.code + ":dateTime";
            return e.code;
        }
        return "unexpected";
    }
    return "none";
}
