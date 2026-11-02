import { label, expectException } from "submilli:test";
import { listEvents, listEventDefinitions, listPropertyDefinitions, getCohort, capture } from "@submilli/posthog";
import { buildEventsQuery, captureFields, formatCursor, isoMillis, ownedFilter, parseCursor, utcSeconds } from "../src/request";
import { eventPageFrom, cohortFrom, eventDefinitionsFrom } from "../src/response";

const UUID_A = "0192d0a3-1111-7000-8000-000000000001";
const UUID_B = "0192d0a3-1111-7000-8000-000000000002";

function main(): void {
    label("builds a bounded, keyset-paged HogQL query");
    const start = Temporal.Instant.from("2026-10-24T06:50:00.250Z").epochMilliseconds;
    const end = Temporal.Instant.from("2026-10-24T08:50:00Z").epochMilliseconds;
    const hogql = buildEventsQuery({
        events: ["$pageview", "link_clicked"],
        startMs: start,
        endMs: end,
        properties: ["$host", "path"],
        personProperties: [],
        filters: [ownedFilter(undefined, "is_internal", "notIn", ["True", " yes "])],
        excludeCohortIds: [561927],
        limit: 100,
        cursorMs: null,
        cursorUuid: "",
    });
    assert(hogql.startsWith("SELECT toString(uuid), event, toUnixTimestamp64Milli(timestamp), toString(properties['$host']), toString(properties['path']) FROM events"), hogql);
    assert(hogql.includes("event IN ('$pageview', 'link_clicked')"), "event names");
    assert(hogql.includes("timestamp >= toDateTime('2026-10-24 06:50:00', 'UTC')"), "lower pruning bound");
    assert(hogql.includes("toUnixTimestamp64Milli(timestamp) >= " + start.toString()), "exact lower bound");
    assert(hogql.includes("toUnixTimestamp64Milli(timestamp) < " + end.toString()), "exact upper bound");
    assert(hogql.includes("person_id NOT IN COHORT 561927"), "cohort exclusion");
    assert(hogql.includes("lower(trim(coalesce(toString(properties['is_internal']), ''))) NOT IN ('true', 'yes')"), "null-safe, case-insensitive filter");
    assert(hogql.endsWith("ORDER BY toUnixTimestamp64Milli(timestamp) ASC, toString(uuid) ASC LIMIT 101"), "one extra row, no OFFSET");
    assert(!hogql.includes("OFFSET"), "personal API keys refuse OFFSET");

    label("cursor round-trips and narrows the next page");
    const cursor = formatCursor(start, UUID_A);
    const parsed = parseCursor(cursor);
    assert(parsed !== null && parsed[0] === start && parsed[1] === UUID_A, "parsed cursor");
    assert(parseCursor(undefined) === null, "no cursor");
    expectException(() => { parseCursor("1' OR 1=1"); }, "PostHogError");

    label("formats instants");
    assert(utcSeconds(start) === "2026-10-24 06:50:00", utcSeconds(start));
    assert(isoMillis(start) === "2026-10-24T06:50:00.250Z", isoMillis(start));
    assert(isoMillis(end) === "2026-10-24T08:50:00.000Z", isoMillis(end));

    label("maps a page and detects the extra row");
    const body = "{\"results\":[[\"" + UUID_A + "\",\"$pageview\"," + start.toString() + ",\"submilli.ai\",\"/docs\"],"
        + "[\"" + UUID_B + "\",\"link_clicked\",\"1792824600300\",\"submilli.ai\",null]]}";
    const page = eventPageFrom(body, 602724, ["$host", "path"], [], 1);
    assert(page.events.length === 1 && !page.isComplete, "one row, more to come");
    assert(page.events[0].timestamp === "2026-10-24T06:50:00.250Z", page.events[0].timestamp);
    assert(page.events[0].properties[1].value === "/docs", "property by position");
    assert(page.events[0].url === "https://us.posthog.com/project/602724/events/" + UUID_A + "/2026-10-24T06%3A50%3A00.250Z", page.events[0].url);
    assert(page.nextCursor === formatCursor(start, UUID_A), page.nextCursor);
    const last = eventPageFrom(body, 602724, ["$host", "path"], [], 5);
    assert(last.isComplete && last.nextCursor === "" && last.events[1].properties[1].value === null, "last page");
    expectException(() => { eventPageFrom("{\"results\":[[\"" + UUID_A + "\",\"$pageview\",1]]}", 602724, ["$host"], [], 5); }, "PostHogError");
    expectException(() => { eventPageFrom(JSON.stringify({ error: "syntax" }), 602724, [], [], 5); }, "PostHogError");

    label("maps cohorts and definitions");
    const cohort = cohortFrom(JSON.stringify({ id: 561927, name: "Internal", count: 0, is_calculating: false, last_calculation: "2026-10-20T09:00:05Z", errors_calculating: 0 }));
    assert(cohort.id === 561927 && cohort.count === 0 && !cohort.isCalculating && !cohort.deleted, "cohort");
    const defs = eventDefinitionsFrom(JSON.stringify({ count: 3, next: "https://us.posthog.com/next", results: [{ name: "$pageview", last_seen_at: null }] }), 0);
    assert(defs.items.length === 1 && defs.nextOffset === 1 && defs.count === 3, "definitions page");

    label("capture body carries only the given fields");
    const fields = captureFields("demo_booked", "person-1", [{ name: "source", value: "google_appointment_schedule" }], [{ name: "plan", value: "pilot" }], undefined, UUID_A);
    assert(fields === "\"event\":\"demo_booked\",\"distinct_id\":\"person-1\",\"properties\":{\"source\":\"google_appointment_schedule\",\"$set\":{\"plan\":\"pilot\"}},\"uuid\":\"" + UUID_A + "\"", fields);
    expectException(() => { captureFields("e", "", [], [], undefined, undefined); }, "PostHogError");
    expectException(() => { captureFields("e", "d", [{ name: "a", value: 1 }, { name: "a", value: 2 }], [], undefined, undefined); }, "PostHogError");

    label("operations validate before any check, secret or request");
    const window = { events: ["$pageview"], after: "2026-10-24T06:50:00Z", before: "2026-10-24T08:50:00Z" };
    expectException(() => { listEvents(0, window); }, "PostHogError");
    expectException(() => { listEvents(602724, { events: [], after: window.after, before: window.before }); }, "PostHogError");
    expectException(() => { listEvents(602724, { events: ["x') OR 1=1 --"], after: window.after, before: window.before }); }, "PostHogError");
    expectException(() => { listEvents(602724, { events: ["$pageview"], after: window.before, before: window.after }); }, "PostHogError");
    expectException(() => { listEvents(602724, { events: ["$pageview"], after: "yesterday", before: window.before }); }, "PostHogError");
    expectException(() => { listEvents(602724, { events: ["$pageview"], after: window.after, before: window.before, limit: 1001 }); }, "PostHogError");
    expectException(() => { listEvents(602724, { events: ["$pageview"], after: window.after, before: window.before, properties: ["a\\b"] }); }, "PostHogError");
    expectException(() => { listEvents(602724, { events: ["$pageview"], after: window.after, before: window.before, filters: [{ property: "x", operator: "notIn", values: [] }] }); }, "PostHogError");
    expectException(() => { listEvents(602724, { events: ["$pageview"], after: window.after, before: window.before, excludeCohortIds: [-1] }); }, "PostHogError");
    expectException(() => { listEventDefinitions(602724, { limit: 0 }); }, "PostHogError");
    expectException(() => { listPropertyDefinitions(602724, { offset: -1 }); }, "PostHogError");
    expectException(() => { getCohort(602724, 1.5); }, "PostHogError");
    expectException(() => { capture({ event: "bad'name", distinctId: "d" }); }, "PostHogError");
    expectException(() => { capture({ event: "ok", distinctId: "d", timestamp: "soon" }); }, "PostHogError");
}
