// Request validation and HogQL construction for the PostHog client. Internal:
// `src/lib.ts` re-exports the option types and calls these builders after it
// has copied the caller's values into package-owned ones.
//
// Every string the package writes into HogQL is validated against a charset
// with no quote, backslash, or control character, so no caller value can leave
// its string literal. Identifiers (property names) are written as bracketed
// string keys, never as bare SQL.

import { invalidInput } from "./errors";

/** Where a filtered property lives. */
export type PropertyScope = "event" | "person";

/** One property condition on `listEvents`. */
export interface PropertyFilter {
    /** `event` (default) reads `properties[...]`; `person` reads `person.properties[...]`. */
    scope?: PropertyScope;
    /** Property name, such as `$host` or `is_internal`. */
    property: string;
    /** `in` keeps rows whose value is one of `values`; `notIn` drops them. */
    operator: "in" | "notIn";
    /** Values to compare. Matching is case-insensitive and trims whitespace; a missing property compares as "". */
    values: string[];
}

/** Options for `listEvents`. `events`, `after` and `before` are required. */
export interface ListEventsOptions {
    /** Event names to return, such as `$pageview` or `link_clicked`. 1–50 names. */
    events: string[];
    /** Inclusive lower bound, an ISO-8601 instant such as `2026-10-24T06:50:00Z`. Millisecond precision. */
    after: string;
    /** Exclusive upper bound, an ISO-8601 instant. Must be after `after`. */
    before: string;
    /** Event property names to return with each row. Up to 50. */
    properties?: string[];
    /** Person property names to return with each row. Up to 20. */
    personProperties?: string[];
    /** Property conditions, all of which must hold. Up to 50. */
    filters?: PropertyFilter[];
    /** Cohort ids whose members' events are dropped (`person_id NOT IN COHORT id`). Up to 10. */
    excludeCohortIds?: number[];
    /** Rows per page, 1–1000. Defaults to 100. */
    limit?: number;
    /** `nextCursor` from the previous page of the same query. */
    cursor?: string;
}

/** A filter the package owns, already validated and lower-cased. */
export interface OwnedFilter {
    scope: PropertyScope;
    property: string;
    operator: "in" | "notIn";
    values: string[];
}

/** A validated `listEvents` query. */
export interface EventsQuery {
    events: string[];
    startMs: number;
    endMs: number;
    properties: string[];
    personProperties: string[];
    filters: OwnedFilter[];
    excludeCohortIds: number[];
    limit: number;
    cursorMs: number | null;
    cursorUuid: string;
}

const SAFE_TEXT = /^[^'"\\`\u0000-\u001f\u007f]{1,200}$/;
const CURSOR = /^([0-9]{1,16}):([0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12})$/;

/**
 * Validate a project, cohort or other positive integer id.
 * @param value The id.
 * @returns The id.
 */
export function requireProjectId(value: number): number {
    if (!Number.isInteger(value) || value < 1 || value > 1000000000000) {
        throw invalidInput("projectId must be a positive integer project id such as 602724, got " + value.toString());
    }
    return value;
}

/**
 * Validate text that will sit inside a HogQL string literal or a query parameter.
 * @param value The text.
 * @param name Argument name for the error message.
 * @returns The text, unchanged.
 */
export function requireSafeText(value: string, name: string): string {
    if (typeof value !== "string" || !SAFE_TEXT.test(value)) {
        throw invalidInput(name + " must be 1–200 characters with no quotes, backslashes, backticks or control characters, got " + JSON.stringify(value));
    }
    return value;
}

/**
 * Validate a list of safe texts, dropping duplicates.
 * @param values The package-owned list.
 * @param name Argument name for the error message.
 * @param min Fewest entries allowed.
 * @param max Most entries allowed.
 * @returns The distinct entries, in order.
 */
export function requireList(values: string[], name: string, min: number, max: number): string[] {
    if (values.length < min || values.length > max) {
        throw invalidInput(name + " must have " + min.toString() + "–" + max.toString() + " entries, got " + values.length.toString());
    }
    const result: string[] = [];
    for (const value of values) {
        const checked = requireSafeText(value, name + " entry");
        if (!result.includes(checked)) result.push(checked);
    }
    return result;
}

/**
 * Parse an ISO-8601 instant.
 * @param value The instant, with a time zone.
 * @param name Argument name for the error message.
 * @returns Milliseconds since the epoch.
 */
export function requireInstantMs(value: string, name: string): number {
    if (typeof value !== "string" || !/^[0-9]{4}-[0-9]{2}-[0-9]{2}T/.test(value)) {
        throw invalidInput(name + " must be an ISO-8601 instant with a time zone, such as 2026-10-24T06:50:00Z, got " + JSON.stringify(value));
    }
    try {
        return Temporal.Instant.from(value).epochMilliseconds;
    } catch (error) {
        throw invalidInput(name + " must be an ISO-8601 instant with a time zone, such as 2026-10-24T06:50:00Z, got " + JSON.stringify(value));
    }
}

/**
 * Validate a page size.
 * @param value Requested size, or undefined for the default.
 * @param fallback Default size.
 * @param max Largest size allowed.
 * @returns The page size.
 */
export function requireLimit(value: number | undefined, fallback: number, max: number): number {
    const limit = value === undefined ? fallback : value;
    if (!Number.isInteger(limit) || limit < 1 || limit > max) {
        throw invalidInput("limit must be an integer from 1 through " + max.toString() + ", got " + limit.toString());
    }
    return limit;
}

/**
 * Validate an offset.
 * @param value Requested offset, or undefined for 0.
 * @returns The offset.
 */
export function requireOffset(value: number | undefined): number {
    const offset = value === undefined ? 0 : value;
    if (!Number.isInteger(offset) || offset < 0 || offset > 1000000) {
        throw invalidInput("offset must be a non-negative integer, got " + offset.toString());
    }
    return offset;
}

/**
 * Validate cohort ids, dropping duplicates.
 * @param values The package-owned ids.
 * @returns The distinct ids.
 */
export function requireCohortIds(values: number[]): number[] {
    if (values.length > 10) throw invalidInput("excludeCohortIds may list at most 10 cohorts");
    const result: number[] = [];
    for (const value of values) {
        if (!Number.isInteger(value) || value < 1) {
            throw invalidInput("excludeCohortIds entries must be positive integer cohort ids, got " + value.toString());
        }
        if (!result.includes(value)) result.push(value);
    }
    return result;
}

/**
 * Validate one property filter and lower-case its values.
 * @param scope Event or person, default event.
 * @param property Property name.
 * @param operator in or notIn.
 * @param values The package-owned values.
 * @returns The filter the package owns.
 */
export function ownedFilter(scope: PropertyScope | undefined, property: string, operator: string, values: string[]): OwnedFilter {
    const where: PropertyScope = scope === undefined ? "event" : scope;
    if (where !== "event" && where !== "person") {
        throw invalidInput("filter scope must be event or person, got " + show(scope));
    }
    if (operator !== "in" && operator !== "notIn") {
        throw invalidInput("filter operator must be in or notIn, got " + show(operator));
    }
    const name = requireSafeText(property, "filter property");
    const lowered: string[] = [];
    for (const value of requireList(values, "filter values", 1, 50)) {
        const normalized = value.trim().toLowerCase();
        if (!lowered.includes(normalized)) lowered.push(normalized);
    }
    return { scope: where, property: name, operator: operator === "in" ? "in" : "notIn", values: lowered };
}

/**
 * Parse a `nextCursor`.
 * @param cursor The cursor, or undefined for the first page.
 * @returns Milliseconds and UUID of the last row read, or null.
 */
export function parseCursor(cursor: string | undefined): [number, string] | null {
    if (cursor === undefined || cursor === "") return null;
    if (!CURSOR.test(cursor)) {
        throw invalidInput("cursor must be a nextCursor returned by listEvents, got " + JSON.stringify(cursor));
    }
    const split = cursor.indexOf(":");
    return [parseInt(cursor.substring(0, split), 10), cursor.substring(split + 1).toLowerCase()];
}

/**
 * Render a value for an error message.
 * @param value Any value.
 * @returns Its JSON text, or "undefined".
 */
export function show(value: unknown): string {
    const rendered = JSON.stringify(value);
    return rendered === undefined ? "undefined" : rendered;
}

/**
 * Format an instant with millisecond precision.
 * @param ms Milliseconds since the epoch.
 * @returns The ISO-8601 UTC text.
 */
export function isoMillis(ms: number): string {
    const seconds = Math.floor(ms / 1000);
    const fraction = ms - seconds * 1000;
    const whole = Temporal.Instant.fromEpochMilliseconds(seconds * 1000).toString();
    const pad = fraction < 10 ? "00" : fraction < 100 ? "0" : "";
    return whole.substring(0, whole.length - 1) + "." + pad + fraction.toString() + "Z";
}

/**
 * Build a `nextCursor` from the last row of a page.
 * @param ms The row time in milliseconds.
 * @param uuid The row UUID.
 * @returns The cursor text.
 */
export function formatCursor(ms: number, uuid: string): string {
    return ms.toString() + ":" + uuid.toLowerCase();
}

/**
 * UTC wall time for `toDateTime(..., 'UTC')`, floored to whole seconds.
 * @param ms Milliseconds since the epoch.
 * @returns Text such as 2026-10-24 06:50:00.
 */
export function utcSeconds(ms: number): string {
    const iso = Temporal.Instant.fromEpochMilliseconds(Math.floor(ms / 1000) * 1000).toString();
    return iso.replace("T", " ").replace("Z", "").substring(0, 19);
}

function literal(value: string): string {
    return "'" + value + "'";
}

function propertyExpression(scope: PropertyScope, property: string): string {
    return (scope === "person" ? "person.properties[" : "properties[") + literal(property) + "]";
}

const MS = "toUnixTimestamp64Milli(timestamp)";

/**
 * Build the HogQL text for one page of `listEvents`.
 * @param query The validated query.
 * @returns The HogQL text.
 */
export function buildEventsQuery(query: EventsQuery): string {
    const columns: string[] = ["toString(uuid)", "event", MS];
    for (const property of query.properties) columns.push("toString(" + propertyExpression("event", property) + ")");
    for (const property of query.personProperties) columns.push("toString(" + propertyExpression("person", property) + ")");
    const names: string[] = [];
    for (const event of query.events) names.push(literal(event));
    const where: string[] = [
        "event IN (" + names.join(", ") + ")",
        // Whole-second bounds let ClickHouse prune by the timestamp index; the
        // millisecond terms make the window exact.
        "timestamp >= toDateTime(" + literal(utcSeconds(query.startMs)) + ", 'UTC')",
        "timestamp < toDateTime(" + literal(utcSeconds(query.endMs)) + ", 'UTC') + INTERVAL 1 SECOND",
        MS + " >= " + query.startMs.toString(),
        MS + " < " + query.endMs.toString(),
    ];
    for (const cohort of query.excludeCohortIds) where.push("person_id NOT IN COHORT " + cohort.toString());
    for (const filter of query.filters) {
        const values: string[] = [];
        for (const value of filter.values) values.push(literal(value));
        where.push("lower(trim(coalesce(toString(" + propertyExpression(filter.scope, filter.property) + "), ''))) "
            + (filter.operator === "in" ? "IN" : "NOT IN") + " (" + values.join(", ") + ")");
    }
    if (query.cursorMs !== null) {
        const ms = query.cursorMs.toString();
        where.push("(" + MS + " > " + ms + " OR (" + MS + " = " + ms + " AND toString(uuid) > " + literal(query.cursorUuid) + "))");
    }
    // One extra row tells the package whether another page exists, without
    // OFFSET (PostHog refuses OFFSET for personal API keys).
    return "SELECT " + columns.join(", ") + " FROM events WHERE " + where.join(" AND ")
        + " ORDER BY " + MS + " ASC, toString(uuid) ASC LIMIT " + (query.limit + 1).toString();
}

/**
 * JSON body for the query endpoint.
 * @param hogql The HogQL text.
 * @returns The JSON text.
 */
export function queryBody(hogql: string): string {
    return "{\"query\":{\"kind\":\"HogQLQuery\",\"query\":" + JSON.stringify(hogql) + "},\"name\":\"submilli posthog package\"}";
}

/** One property sent with `capture`. */
export interface CaptureProperty {
    /** Property name. */
    name: string;
    /** Property value. */
    value: string | number | boolean;
}

/** One event for `capture`. */
export interface CaptureInput {
    /** Event name, such as `demo_booked`. */
    event: string;
    /** The person or anonymous id the event belongs to. */
    distinctId: string;
    /** Event properties. Up to 100. */
    properties?: CaptureProperty[];
    /** Person properties to set (`$set`). Up to 50. */
    set?: CaptureProperty[];
    /** Event time as an ISO-8601 instant; omit for PostHog's receive time. */
    timestamp?: string;
    /** Event UUID, which PostHog uses to deduplicate retries. */
    uuid?: string;
}

function encodeProperties(properties: CaptureProperty[], name: string, max: number): string {
    if (properties.length > max) throw invalidInput(name + " may have at most " + max.toString() + " entries");
    const fields: string[] = [];
    const seen: string[] = [];
    for (const property of properties) {
        const key = property.name;
        const value = property.value;
        if (typeof key !== "string" || key.length === 0 || key.length > 200) {
            throw invalidInput(name + " names must be 1–200 characters");
        }
        if (seen.includes(key)) throw invalidInput(name + " repeats " + JSON.stringify(key));
        seen.push(key);
        if (typeof value === "number" && !Number.isFinite(value)) {
            throw invalidInput(name + " value for " + JSON.stringify(key) + " must be finite");
        }
        fields.push(JSON.stringify(key) + ":" + JSON.stringify(value));
    }
    return "{" + fields.join(",") + "}";
}

/**
 * The capture body's fields, without braces. The caller adds `api_key` after it reads the secret.
 * @param event Validated event name.
 * @param distinctId Distinct id.
 * @param properties Package-owned event properties.
 * @param set Package-owned person properties to set.
 * @param timestamp Optional ISO-8601 event time.
 * @param uuid Optional event UUID.
 * @returns The JSON fields, comma-separated.
 */
export function captureFields(event: string, distinctId: string, properties: CaptureProperty[], set: CaptureProperty[],
    timestamp: string | undefined, uuid: string | undefined): string {
    if (typeof distinctId !== "string" || distinctId.length === 0 || distinctId.length > 200) {
        throw invalidInput("distinctId must be 1–200 characters");
    }
    let props = encodeProperties(properties, "properties", 100);
    if (set.length > 0) {
        const encodedSet = encodeProperties(set, "set", 50);
        props = props === "{}" ? "{\"$set\":" + encodedSet + "}" : props.substring(0, props.length - 1) + ",\"$set\":" + encodedSet + "}";
    }
    const fields: string[] = [
        "\"event\":" + JSON.stringify(event),
        "\"distinct_id\":" + JSON.stringify(distinctId),
        "\"properties\":" + props,
    ];
    if (timestamp !== undefined) {
        requireInstantMs(timestamp, "timestamp");
        fields.push("\"timestamp\":" + JSON.stringify(timestamp));
    }
    if (uuid !== undefined) {
        if (!/^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$/.test(uuid)) {
            throw invalidInput("uuid must be a UUID, got " + JSON.stringify(uuid));
        }
        fields.push("\"uuid\":" + JSON.stringify(uuid));
    }
    return fields.join(",");
}

/**
 * Query string for the definition list endpoints.
 * @param limit Page size.
 * @param offset Offset.
 * @param search Optional search text.
 * @param extra Further encoded parameters, starting with &.
 * @returns The query string, starting with ?.
 */
export function definitionsQuery(limit: number, offset: number, search: string | undefined, extra: string): string {
    let query = "?limit=" + limit.toString() + "&offset=" + offset.toString();
    if (search !== undefined) query += "&search=" + encodeURIComponent(requireSafeText(search, "search"));
    return query + extra;
}
