import { label, expectException } from "submilli:test";
import { listEvents, listEventDefinitions, listPropertyDefinitions, getCohort, capture, PostHogError, PropertyFilter } from "@submilli/posthog";
import { buildEventsQuery, captureFields, formatCursor, isoMillis, ownedFilter, parseCursor, readProperties, requireFields, requireInstantMs, subMillisecond, utcSeconds } from "../src/request";
import { httpError } from "../src/errors";
import { eventPageFrom, cohortFrom, eventDefinitionsFrom, propertyDefinitionsFrom } from "../src/response";

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
        fields: [],
        filters: [ownedFilter(undefined, "is_internal", undefined, "notIn", ["True", " yes "])],
        excludeCohortIds: [561927],
        limit: 100,
        cursor: null,
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
    assert(hogql.includes("timestamp < toDateTime('2026-10-24 08:50:00', 'UTC') + INTERVAL 1 SECOND"), "upper pruning bound");
    assert(!hogql.includes("toString(uuid) >"), "no cursor clause on the first page");

    label("nested keys, person scope and the cursor clause");
    const nested = buildEventsQuery({
        events: ["$pageview"], startMs: start, endMs: end, properties: [], personProperties: ["is_internal"], fields: ["$virt_is_bot"],
        filters: [
            ownedFilter("person", "first_touch", "utm_source", "notIn", ["E2E"]),
            ownedFilter("event", "$host", undefined, "in", ["submilli.ai"]),
        ],
        excludeCohortIds: [], limit: 10, cursor: [start, UUID_A],
    });
    assert(nested.includes("toString(person.properties['is_internal']), toString($virt_is_bot) FROM"), "person then field columns");
    assert(nested.includes("lower(trim(coalesce(toString(person.properties['first_touch']['utm_source']), ''))) NOT IN ('e2e')"), nested);
    assert(nested.includes("lower(trim(coalesce(toString(properties['$host']), ''))) IN ('submilli.ai')"), "in filter");
    assert(nested.includes("(toUnixTimestamp64Milli(timestamp) > " + start.toString() + " OR (toUnixTimestamp64Milli(timestamp) = "
        + start.toString() + " AND toString(uuid) > '" + UUID_A + "'))"), "keyset clause");
    const read = readProperties(["$host"], ["is_internal"], ["$virt_is_bot"], [ownedFilter("person", "first_touch", "utm_source", "notIn", ["e2e"]), ownedFilter(undefined, "$host", undefined, "in", ["x"])]);
    assert(read.event.join(",") === "$host", read.event.join(","));
    assert(read.person.join(",") === "is_internal,first_touch.utm_source", "a nested filter reads its path, not the whole object");
    assert(read.field.join(",") === "$virt_is_bot", "fields are read too");
    label("filter values fold like ClickHouse: ASCII only, spaces only, never blank");
    const folded = ownedFilter(undefined, "p", undefined, "in", ["  ÉCOLE Test  "]);
    assert(folded.values[0] === "École test", folded.values[0]);
    expectException(() => { ownedFilter(undefined, "p", undefined, "in", ["   "]); }, "PostHogError");

    label("cursor round-trips and narrows the next page");
    const cursor = formatCursor(start, UUID_A);
    const parsed = parseCursor(cursor);
    assert(parsed !== null && parsed[0] === start && parsed[1] === UUID_A, "parsed cursor");
    assert(parseCursor(undefined) === null, "no cursor");
    const upper = parseCursor("0:" + UUID_A.toUpperCase());
    assert(upper !== null && upper[0] === 0 && upper[1] === UUID_A, "cursor at ms 0, UUID lower-cased");
    expectException(() => { parseCursor("1' OR 1=1"); }, "PostHogError");

    label("formats instants");
    assert(utcSeconds(start) === "2026-10-24 06:50:00", utcSeconds(start));
    assert(isoMillis(start) === "2026-10-24T06:50:00.250Z", isoMillis(start));
    assert(isoMillis(end) === "2026-10-24T08:50:00.000Z", isoMillis(end));
    assert(isoMillis(0) === "1970-01-01T00:00:00.000Z", isoMillis(0));
    const latest = Temporal.Instant.from("2299-12-31T23:59:59.999Z").epochMilliseconds;
    assert(isoMillis(latest) === "2299-12-31T23:59:59.999Z", isoMillis(latest));

    label("maps a page and detects the extra row");
    const body = "{\"results\":[[\"" + UUID_A + "\",\"$pageview\"," + start.toString() + ",\"submilli.ai\",\"/docs\"],"
        + "[\"" + UUID_B + "\",\"link_clicked\",\"1792824600300\",\"submilli.ai\",null]]}";
    const page = eventPageFrom(body, 602724, ["$host", "path"], [], [], 1);
    assert(page.events.length === 1 && !page.isComplete, "one row, more to come");
    assert(page.events[0].timestamp === "2026-10-24T06:50:00.250Z", page.events[0].timestamp);
    assert(page.events[0].properties[1].value === "/docs", "property by position");
    assert(page.events[0].url === "https://us.posthog.com/project/602724/events/" + UUID_A + "/2026-10-24T06%3A50%3A00.250Z", page.events[0].url);
    assert(page.nextCursor === formatCursor(start, UUID_A), page.nextCursor);
    const last = eventPageFrom(body, 602724, ["$host", "path"], [], [], 5);
    assert(last.isComplete && last.nextCursor === "" && last.events[1].properties[1].value === null, "last page");
    expectException(() => { eventPageFrom("{\"results\":[[\"" + UUID_A + "\",\"$pageview\",1]]}", 602724, ["$host"], [], [], 5); }, "PostHogError");
    expectException(() => { eventPageFrom(JSON.stringify({ error: "syntax" }), 602724, [], [], [], 5); }, "PostHogError");

    const withField = eventPageFrom("{\"results\":[[\"" + UUID_A + "\",\"$pageview\",5,\"h\",true]]}", 602724, ["$host"], [], ["$virt_is_bot"], 5);
    assert(withField.events[0].fields[0].name === "$virt_is_bot" && withField.events[0].fields[0].value === "true", "field column");

    label("JSON of the wrong shape is an invalid response, not a TypeError");
    for (const body of ["null", "{\"results\":[1]}", "{\"results\":{\"a\":1}}"]) {
        let code = "";
        try { eventPageFrom(body, 602724, [], [], [], 5); } catch (error: PostHogError) { code = error.code; }
        assert(code === "invalid_response", body + " gave " + code);
    }
    expectException(() => { cohortFrom("null"); }, "PostHogError");
    expectException(() => { eventDefinitionsFrom("{\"next\":\"x\",\"results\":[null]}", 0); }, "PostHogError");

    label("maps cohorts and definitions");
    const cohort = cohortFrom(JSON.stringify({ id: 561927, name: "Internal", count: 0, is_calculating: false, last_calculation: "2026-10-20T09:00:05Z", errors_calculating: 0 }));
    assert(cohort.id === 561927 && cohort.count === 0 && !cohort.isCalculating && !cohort.deleted, "cohort");
    const defs = eventDefinitionsFrom(JSON.stringify({ count: 3, next: "https://us.posthog.com/next", results: [{ name: "$pageview", last_seen_at: null }] }), 0);
    assert(defs.items.length === 1 && defs.nextOffset === 1 && defs.count === 3, "definitions page");
    const emptyWithNext = eventDefinitionsFrom(JSON.stringify({ count: 3, next: "https://us.posthog.com/next", results: [] }), 4);
    assert(emptyWithNext.nextOffset === null, "an empty page ends paging");
    const props = propertyDefinitionsFrom("{\"count\":2,\"next\":null,\"results\":[{\"name\":\"$host\",\"property_type\":\"String\"},{\"name\":null}]}", 0);
    assert(props.items.length === 1 && props.items[0].propertyType === "String" && props.nextOffset === null, "property definitions");
    expectException(() => { cohortFrom("<html>bad gateway</html>"); }, "PostHogError");

    label("HTTP errors carry the budget code and Retry-After");
    const budget = httpError(429, "{\"code\":\"api_queries_budget_exceeded\"}", "120");
    assert(budget.code === "budget_exceeded" && budget.retryAfterSeconds === 120 && budget.status === 429, budget.code);
    assert(httpError(429, "slow down", null).code === "http_429", "a plain 429 keeps its status code");
    const other = httpError(403, "forbidden", "soon");
    assert(other.code === "http_403" && other.retryAfterSeconds === null, other.code);

    label("capture body carries only the given fields");
    const fields = captureFields("demo_booked", "person-1", [{ name: "source", value: "google_appointment_schedule" }], [{ name: "plan", value: "pilot" }], undefined, UUID_A);
    assert(fields === "\"event\":\"demo_booked\",\"distinct_id\":\"person-1\",\"properties\":{\"source\":\"google_appointment_schedule\",\"$set\":{\"plan\":\"pilot\"}},\"uuid\":\"" + UUID_A + "\"", fields);
    expectException(() => { captureFields("e", "", [], [], undefined, undefined); }, "PostHogError");
    expectException(() => { captureFields("e", "d", [{ name: "a", value: 1 }, { name: "a", value: 2 }], [], undefined, undefined); }, "PostHogError");
    expectException(() => { captureFields("e", "d", [{ name: "$set", value: "x" }], [], undefined, undefined); }, "PostHogError");
    const timed = captureFields("e", "d", [], [], "2026-10-24T09:50:00.5+03:00", undefined);
    assert(timed.endsWith("\"timestamp\":\"2026-10-24T06:50:00.500Z\""), timed);
    const annotated = captureFields("e", "d", [], [], "2026-10-24T09:50:00+03:00[Asia/Jerusalem]", undefined);
    assert(annotated.endsWith("\"timestamp\":\"2026-10-24T06:50:00.000Z\""), annotated);

    label("operations validate before any check, secret or request");
    const window = { events: ["$pageview"], after: "2026-10-24T06:50:00Z", before: "2026-10-24T08:50:00Z" };
    expectException(() => { listEvents(0, window); }, "PostHogError");
    expectException(() => { listEvents(602724, { events: [], after: window.after, before: window.before }); }, "PostHogError");
    expectException(() => { listEvents(602724, { events: ["x') OR 1=1 --"], after: window.after, before: window.before }); }, "PostHogError");
    expectException(() => { listEvents(602724, { events: ["$pageview"], after: window.before, before: window.after }); }, "PostHogError");
    expectException(() => { listEvents(602724, { events: ["$pageview"], after: "yesterday", before: window.before }); }, "PostHogError");
    expectException(() => { listEvents(602724, { events: ["$pageview"], after: "1800-01-01T00:00:00Z", before: window.before }); }, "PostHogError");
    expectException(() => { listEvents(602724, { events: ["$pageview"], after: "1969-12-31T23:59:59.999Z", before: window.before }); }, "PostHogError");
    expectException(() => { listEvents(602724, { events: ["$pageview"], after: window.after, before: "2300-01-01T00:00:00Z" }); }, "PostHogError");
    expectException(() => { requireFields(["$ip"]); }, "PostHogError");
    assert(requireFields(["$virt_is_bot", "$virt_is_bot"]).length === 1, "duplicate fields collapse");
    expectException(() => { listEvents(602724, { events: ["$pageview"], after: window.after, before: window.before, properties: ["first_touch.utm_source"] }); }, "PostHogError");
    expectException(() => { listEvents(602724, { events: ["$pageview"], after: window.after, before: window.before, filters: [{ property: "a.b", operator: "in", values: ["x"] }] }); }, "PostHogError");
    expectException(() => { listEvents(602724, { events: ["$pageview"], after: "2026-10-24T06:50:00+00:00:00.5", before: window.before }); }, "PostHogError");
    label("instants: the runtime's own strings, offsets, and rounding up past the millisecond");
    const base = Temporal.Instant.from("2026-10-24T06:50:00Z").epochMilliseconds;
    assert(requireInstantMs("2026-10-24T09:50:00+03:00", "after") === base, "whole-minute offset");
    assert(requireInstantMs("2026-10-24T06:50Z", "after") === base, "seconds omitted");
    assert(requireInstantMs("2026-10-24T06:50:00,5Z", "after") === base + 500, "comma decimal");
    assert(requireInstantMs("2026-10-24T06:50:00.123000Z", "after") === base + 123, "trailing zeros are exact");
    assert(requireInstantMs("2026-10-24T06:50:00.123456Z", "after") === base + 124, "microseconds round up");
    assert(requireInstantMs("2026-10-24T06:50:00.000000001Z", "after") === base + 1, "nanoseconds round up");
    assert(requireInstantMs("2026-10-24T09:50:00+03:00[Asia/Jerusalem]", "after") === base, "zone annotation");
    const now = Temporal.Now.instant();
    assert(requireInstantMs(now.toString(), "before") >= now.epochMilliseconds, "Temporal.Now strings are accepted");
    expectException(() => { requireInstantMs("2026-10-24T06:50:00z", "after"); }, "PostHogError");
    expectException(() => { requireInstantMs("2026-10-24T09:50:00+0300", "after"); }, "PostHogError");
    expectException(() => { requireInstantMs("2026-10-24T06:50:00.1234567890Z", "after"); }, "PostHogError");
    assert(requireInstantMs("2026-12-31T23:59:59.999000001Z", "after") === Temporal.Instant.from("2027-01-01T00:00:00Z").epochMilliseconds, "rounding carries into the next day and year");
    assert(requireInstantMs("2026-10-24T06:50:00+00:00[Asia/Jerusalem]", "after") === base, "the offset wins over the annotation");
    expectException(() => { requireInstantMs("2026-10-24T06:50:00Z[u-ca=hebrew]", "after"); }, "PostHogError");
    expectException(() => { requireInstantMs("2026-10-24T06:50:00Z[!Asia/Jerusalem]", "after"); }, "PostHogError");
    expectException(() => { requireInstantMs("2026-10-24T23:59:60Z", "after"); }, "PostHogError");
    expectException(() => { requireInstantMs("2299-12-31T23:59:59.9990001Z", "before"); }, "PostHogError");
    // A window inside one millisecond is still checked and needs the key, like any other read.
    let sameMsCode = "";
    try { listEvents(602724, { events: ["$pageview"], after: "2026-10-24T06:50:00.0001Z", before: "2026-10-24T06:50:00.0009Z" }); } catch (error: PostHogError) { sameMsCode = error.code; }
    assert(sameMsCode === "missing_credentials", "same-millisecond window gave " + sameMsCode);
    let nanMessage = "";
    try { listEvents(602724, { events: ["$pageview"], after: window.after, before: window.before, limit: NaN }); } catch (error: PostHogError) { nanMessage = error.message; }
    assert(nanMessage.endsWith("got NaN"), nanMessage);
    expectException(() => { listEvents(602724, { events: ["$pageview"], after: "2026-10-24T06:50:00.0001Z", before: "2026-10-24T06:50:00.0009Z", limit: 0 }); }, "PostHogError");
    assert(!subMillisecond("2026-10-24T06:50:00.120Z") && subMillisecond("2026-10-24T06:50:00.1201Z"), "subMillisecond");
    assert(!subMillisecond("2026-10-24T09:50:00.5+03:00"), "offset digits are not part of the fraction");
    const many: string[] = [];
    for (let index = 0; index < 51; index++) many.push("p" + index.toString());
    expectException(() => { listEvents(602724, { events: ["$pageview"], after: window.after, before: window.before, properties: many }); }, "PostHogError");
    expectException(() => { listEvents(602724, { events: ["$pageview"], after: window.after, before: window.before, personProperties: many.slice(0, 21) }); }, "PostHogError");
    expectException(() => { listEvents(602724, { events: ["$pageview"], after: window.after, before: window.before, excludeCohortIds: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11] }); }, "PostHogError");
    const tooManyFilters: PropertyFilter[] = [];
    for (const name of many) tooManyFilters.push({ property: name, operator: "notIn", values: ["x"] });
    expectException(() => { listEvents(602724, { events: ["$pageview"], after: window.after, before: window.before, filters: tooManyFilters }); }, "PostHogError");
    expectException(() => { listEvents(602724, { events: ["$pageview"], after: window.after, before: window.before, excludeCohortIds: [1e21] }); }, "PostHogError");
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
