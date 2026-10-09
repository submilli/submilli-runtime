import { label } from "submilli:test";
import secrets from "submilli:secrets";
import { createEvent, deleteEvent } from "@submilli/google-calendar";

function main(): void {
    label("live Calendar event create and cleanup");
    if (secrets.get("GOOGLE_ACCESS_TOKEN") === undefined) return;
    if (secrets.get("GOOGLE_LIVE_MUTATIONS") !== "true") return;

    const start = Temporal.Now.instant().add({ minutes: 10 });
    const end = start.add({ minutes: 15 });
    let eventId: string | undefined;
    try {
        const event = createEvent({
            summary: "Submilli Google Calendar integration test",
            description: "Temporary event created by the @submilli/google-calendar live test.",
            start: { dateTime: start.toString() },
            end: { dateTime: end.toString() },
            sendUpdates: "none",
        });
        eventId = event.id;
        assert(event.id.length > 0, "created event has an ID");
    } finally {
        if (eventId !== undefined) deleteEvent(eventId, { sendUpdates: "none" });
    }
}
