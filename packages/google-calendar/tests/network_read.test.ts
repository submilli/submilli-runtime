import { label } from "submilli:test";
import secrets from "submilli:secrets";
import { listCalendars, queryFreeBusy } from "@submilli/google-calendar";

function main(): void {
    label("live calendar list when GOOGLE_ACCESS_TOKEN is available");
    if (secrets.get("GOOGLE_ACCESS_TOKEN") === null) return;
    const page = listCalendars({ limit: 1 });
    assert(page.items.length <= 1, "respects the requested bound");

    label("live free/busy accepts bracketed Temporal bounds");
    const result = queryFreeBusy({
        calendarIds: ["primary"],
        timeMin: "2026-08-11T00:00:00-07:00[America/Los_Angeles]",
        timeMax: "2026-08-11T01:00:00-07:00[America/Los_Angeles]",
    });
    assert(result.timeMin === "2026-08-11T07:00:00Z", "bounds are normalized to UTC");
    assert(result.calendars.length === 1, "one calendar queried");
}
