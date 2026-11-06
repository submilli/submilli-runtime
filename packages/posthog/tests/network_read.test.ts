// Read-only live checks. Needs POSTHOG_PERSONAL_API_KEY (Query Read, Event and
// Property Definition Read, Cohort Read) and POSTHOG_TEST_PROJECT_ID. Capture is
// never exercised here: it would write an event into a real project.
import { label } from "submilli:test";
import secrets from "submilli:secrets";
import { listEvents, listEventDefinitions, listPropertyDefinitions } from "@submilli/posthog";

function main(): void {
    if (secrets.get("POSTHOG_PERSONAL_API_KEY") === undefined) {
        label("skip: POSTHOG_PERSONAL_API_KEY is not bound");
        return;
    }
    const projectText = secrets.get("POSTHOG_TEST_PROJECT_ID");
    if (projectText === undefined) {
        label("skip: POSTHOG_TEST_PROJECT_ID is not bound");
        return;
    }
    const projectId = parseInt(projectText, 10);

    label("schema discovery");
    const events = listEventDefinitions(projectId, { search: "pageview", limit: 5 });
    assert(events.count >= 0, "event definitions list");
    const props = listPropertyDefinitions(projectId, { type: "event", eventNames: ["$pageview"], limit: 5 });
    assert(props.items.length >= 0, "property definitions list");

    label("a one-day window pages to completion without OFFSET");
    const before = Temporal.Now.instant();
    const after = before.subtract({ hours: 24 });
    let cursor = "";
    let pages = 0;
    let previous = "";
    while (pages < 5) {
        const page = listEvents(projectId, {
            events: ["$pageview"],
            after: after.toString(),
            before: before.toString(),
            properties: ["$host", "$pathname"],
            limit: 50,
            cursor: cursor,
        });
        for (const row of page.events) {
            const key = row.timestamp + row.uuid;
            assert(key > previous, "rows are strictly ordered across pages");
            previous = key;
            assert(row.properties.length === 2, "selected properties come back in order");
        }
        pages++;
        if (page.isComplete) break;
        cursor = page.nextCursor;
    }
}
