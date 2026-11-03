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

/** Columns of the events table itself, outside `properties`: PostHog's bot and traffic classification. Keep in step with EVENT_FIELDS. */
export type EventField = "$virt_is_bot" | "$virt_traffic_type" | "$virt_traffic_category" | "$virt_bot_name";

const EVENT_FIELDS: string[] = ["$virt_is_bot", "$virt_traffic_type", "$virt_traffic_category", "$virt_bot_name"];

/** One property condition on `listEvents`. */
export interface PropertyFilter {
    /** `event` (default) reads `properties[...]`; `person` reads `person.properties[...]`. */
    scope?: PropertyScope;
    /** Property name, such as `$host` or `is_internal`. */
    property: string;
    /** For a property that holds an object, the key inside it, such as `utm_source` in `first_touch`. */
    key?: string;
    /** `in` keeps rows whose value is one of `values`; `notIn` drops them. */
    operator: "in" | "notIn";
    /** Non-blank values to compare. Comparison trims spaces and ignores ASCII case; a missing property compares as "". */
    values: string[];
}

/** Options for `listEvents`. `events`, `after` and `before` are required. */
export interface ListEventsOptions {
    /** Event names to return, such as `$pageview` or `link_clicked`. 1–50 names. */
    events: string[];
    /** Inclusive lower bound, an ISO-8601 instant such as `2026-10-24T06:50:00Z`, years 1970–2299. Finer than a millisecond rounds up. */
    after: string;
    /** Exclusive upper bound, an ISO-8601 instant. Must be after `after`. */
    before: string;
    /** Event property names to return with each row. Up to 50. */
    properties?: string[];
    /** Person property names to return with each row. Up to 20. */
    personProperties?: string[];
    /** Events-table fields to return with each row, such as `$virt_is_bot`. */
    fields?: EventField[];
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
    key: string | null;
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
    fields: string[];
    filters: OwnedFilter[];
    excludeCohortIds: number[];
    limit: number;
    /** Milliseconds and UUID of the last row already read, or null for the first page. */
    cursor: [number, string] | null;
}

/**
 * What one query reads, selected or filtered: event and person property paths
 * (`first_touch.utm_source` for a nested key) and events-table fields.
 */
export interface ReadProperties {
    event: string[];
    person: string[];
    field: string[];
}

const SAFE_TEXT = /^[^'"\\`\u0000-\u001f\u007f]{1,200}$/;
// Z or a whole-minute offset (the runtime rounds fractional-second offsets),
// up to nine fractional digits, and an optional [Zone] annotation.
const INSTANT = /^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}(:[0-5][0-9]([.,][0-9]{1,9})?)?(Z|[+-][0-9]{2}:[0-9]{2})(\[[A-Za-z0-9_+\/-]{1,64}\])?$/;
const CURSOR = /^([0-9]{1,16}):([0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12})$/;
// ClickHouse DateTime64 covers 1900–2299; the package keeps windows inside 1970–2299.
const LATEST_MS = 10413792000000;

/**
 * Validate a positive integer id, such as a project or cohort id.
 * @param value The id.
 * @param name Argument name for the error message.
 * @returns The id.
 */
export function requireId(value: number, name: string): number {
    if (!Number.isInteger(value) || value < 1 || value > 1000000000000) {
        throw invalidInput(name + " must be a positive integer id up to 10^12, got " + show(value));
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
        throw invalidInput(name + " must be 1–200 characters with no quotes, backslashes, backticks or control characters, got " + show(value));
    }
    return value;
}

/**
 * Validate a property name. A dot is refused, so a name is never mistaken for
 * a nested path; read a key inside an object property with a filter's `key`.
 * @param value The name.
 * @param name Argument name for the error message.
 * @returns The name, unchanged.
 */
export function requirePropertyName(value: string, name: string): string {
    const checked = requireSafeText(value, name);
    if (checked.includes(".")) throw invalidInput(name + " must not contain a dot; use a filter's `key` for a nested value, got " + show(value));
    return checked;
}

/**
 * Validate a list of property names, dropping duplicates.
 * @param values The package-owned list.
 * @param name Argument name for the error message.
 * @param max Most entries allowed.
 * @returns The distinct names, in order.
 */
export function requirePropertyNames(values: string[], name: string, max: number): string[] {
    const result = requireList(values, name, 0, max);
    for (const value of result) requirePropertyName(value, name + " entry");
    return result;
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
 * Parse an ISO-8601 instant inside the supported range, rounded up to the
 * next millisecond when it has a finer fraction (so `[after, before)` stays
 * exact at the millisecond granularity of the query).
 * @param value The instant, with a time zone.
 * @param name Argument name for the error message.
 * @returns Milliseconds since the epoch.
 */
export function requireInstantMs(value: string, name: string): number {
    const invalid = invalidInput(name + " must be an ISO-8601 instant with Z or a ±HH:MM offset (a plain [Zone] annotation is ignored; calendar and critical annotations are refused), before 2300, such as 2026-10-24T06:50:00Z, got " + show(value));
    if (typeof value !== "string" || !INSTANT.test(value)) throw invalid;
    let ms = 0;
    try {
        ms = Temporal.Instant.from(value).epochMilliseconds;
    } catch (error) {
        throw invalid;
    }
    if (subMillisecond(value)) ms++;
    if (ms < 0 || ms >= LATEST_MS) throw invalid;
    return ms;
}

/**
 * Whether an instant's fraction has a nonzero digit past the milliseconds.
 * @param value An instant that matched INSTANT.
 * @returns True when the instant is not a whole millisecond.
 */
export function subMillisecond(value: string): boolean {
    return /[.,][0-9]{3}[0-9]*[1-9]/.test(value);
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
        throw invalidInput("limit must be an integer from 1 through " + max.toString() + ", got " + show(limit));
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
        throw invalidInput("offset must be an integer from 0 through 1000000, got " + show(offset));
    }
    return offset;
}

/**
 * Validate cohort ids, dropping duplicates.
 * @param values The package-owned ids.
 * @returns The distinct ids.
 */
export function requireCohortIds(values: number[]): number[] {
    if (values.length > 10) throw invalidInput("excludeCohortIds may list at most 10 cohorts, got " + values.length.toString());
    const result: number[] = [];
    for (const value of values) {
        const id = requireId(value, "excludeCohortIds entry");
        if (!result.includes(id)) result.push(id);
    }
    return result;
}

/**
 * Validate the number of filters.
 * @param count How many filters the caller passed.
 */
export function requireFilterCount(count: number): void {
    if (count > 50) throw invalidInput("filters may have at most 50 entries, got " + count.toString());
}

/**
 * Validate events-table field names, dropping duplicates.
 * @param values The package-owned names.
 * @returns The distinct names.
 */
export function requireFields(values: string[]): string[] {
    const result: string[] = [];
    for (const value of values) {
        if (!EVENT_FIELDS.includes(value)) throw invalidInput("fields entries must be one of " + EVENT_FIELDS.join(", ") + ", got " + show(value));
        if (!result.includes(value)) result.push(value);
    }
    return result;
}

/**
 * Validate one property filter and normalize its values the way ClickHouse compares them.
 * @param scope Event or person, default event.
 * @param property Property name.
 * @param key Optional key inside an object property.
 * @param operator in or notIn.
 * @param values The package-owned values.
 * @returns The filter the package owns.
 */
export function ownedFilter(scope: PropertyScope | undefined, property: string, key: string | undefined, operator: string, values: string[]): OwnedFilter {
    const where: PropertyScope = scope === undefined ? "event" : scope;
    if (where !== "event" && where !== "person") {
        throw invalidInput("filter scope must be event or person, got " + show(scope));
    }
    if (operator !== "in" && operator !== "notIn") {
        throw invalidInput("filter operator must be in or notIn, got " + show(operator));
    }
    const name = requirePropertyName(property, "filter property");
    const nested = key === undefined ? null : requireSafeText(key, "filter key");
    const normalized: string[] = [];
    for (const value of requireList(values, "filter values", 1, 50)) {
        const folded = asciiLower(trimSpaces(value));
        if (folded === "") throw invalidInput("filter values must not be blank; a missing property already compares as \"\"");
        if (!normalized.includes(folded)) normalized.push(folded);
    }
    return { scope: where, property: name, key: nested, operator: operator === "in" ? "in" : "notIn", values: normalized };
}

/**
 * What a query reads: the properties it selects and those it filters on, as
 * paths, and the events-table fields it selects.
 * @param selected Selected event properties.
 * @param selectedPerson Selected person properties.
 * @param fields Selected events-table fields.
 * @param filters The query's filters.
 * @returns Distinct event and person property paths and field names.
 */
export function readProperties(selected: string[], selectedPerson: string[], fields: string[], filters: OwnedFilter[]): ReadProperties {
    const event: string[] = [];
    const person: string[] = [];
    for (const name of selected) event.push(name);
    for (const name of selectedPerson) person.push(name);
    for (const filter of filters) {
        const path = filter.key === null ? filter.property : filter.property + "." + filter.key;
        const into = filter.scope === "person" ? person : event;
        if (!into.includes(path)) into.push(path);
    }
    const field: string[] = [];
    for (const name of fields) field.push(name);
    return { event: event, person: person, field: field };
}

// ClickHouse `trim` strips only spaces and `lower` folds only ASCII letters;
// the package normalizes filter values the same way so both sides compare alike.
function trimSpaces(value: string): string {
    let start = 0;
    let end = value.length;
    while (start < end && value.charAt(start) === " ") start++;
    while (end > start && value.charAt(end - 1) === " ") end--;
    return value.substring(start, end);
}

function asciiLower(value: string): string {
    let result = "";
    for (let index = 0; index < value.length; index++) {
        const code = value.charCodeAt(index);
        result += code >= 65 && code <= 90 ? String.fromCharCode(code + 32) : value.charAt(index);
    }
    return result;
}

/**
 * Parse a `nextCursor`.
 * @param cursor The cursor, or undefined for the first page.
 * @returns Milliseconds and UUID of the last row read, or null.
 */
export function parseCursor(cursor: string | undefined): [number, string] | null {
    if (cursor === undefined || cursor === "") return null;
    if (!CURSOR.test(cursor)) {
        throw invalidInput("cursor must be a nextCursor returned by listEvents, got " + show(cursor));
    }
    const split = cursor.indexOf(":");
    return [parseInt(cursor.substring(0, split), 10), cursor.substring(split + 1).toLowerCase()];
}

/**
 * Render a value for an error message.
 * @param value Any value.
 * @returns Its JSON text (cut at 200 characters), NaN or Infinity for a non-finite number, or "undefined".
 */
export function show(value: unknown): string {
    if (typeof value === "number" && !Number.isFinite(value)) return value.toString();
    const rendered = JSON.stringify(value);
    if (rendered === undefined) return "undefined";
    if (rendered.length <= 200) return rendered;
    // Do not end on the first half of a surrogate pair.
    const high = rendered.charCodeAt(199);
    return rendered.substring(0, high >= 0xd800 && high <= 0xdbff ? 199 : 200) + "…";
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

function propertyExpression(scope: PropertyScope, property: string, key: string | null): string {
    const base = (scope === "person" ? "person.properties[" : "properties[") + literal(property) + "]";
    return key === null ? base : base + "[" + literal(key) + "]";
}

const MS = "toUnixTimestamp64Milli(timestamp)";

/**
 * Build the HogQL text for one page of `listEvents`.
 * @param query The validated query.
 * @returns The HogQL text.
 */
export function buildEventsQuery(query: EventsQuery): string {
    const columns: string[] = ["toString(uuid)", "event", MS];
    for (const property of query.properties) columns.push("toString(" + propertyExpression("event", property, null) + ")");
    for (const property of query.personProperties) columns.push("toString(" + propertyExpression("person", property, null) + ")");
    // Field names come from a fixed list, so they are written as identifiers.
    for (const field of query.fields) columns.push("toString(" + field + ")");
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
        where.push("lower(trim(coalesce(toString(" + propertyExpression(filter.scope, filter.property, filter.key) + "), ''))) "
            + (filter.operator === "in" ? "IN" : "NOT IN") + " (" + values.join(", ") + ")");
    }
    if (query.cursor !== null) {
        const ms = query.cursor[0].toString();
        where.push("(" + MS + " > " + ms + " OR (" + MS + " = " + ms + " AND toString(uuid) > " + literal(query.cursor[1]) + "))");
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
    /** Event properties. Up to 100. Use `set` rather than a `$set` property. */
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
        if (seen.includes(key)) throw invalidInput(name + " repeats " + show(key));
        seen.push(key);
        if (typeof value === "number" && !Number.isFinite(value)) {
            throw invalidInput(name + " value for " + show(key) + " must be finite");
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
    for (const property of properties) {
        if (property.name === "$set") throw invalidInput("pass person properties in `set`, not as a `$set` property");
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
        // Send the normalized UTC instant, not the caller's spelling of it.
        fields.push("\"timestamp\":" + JSON.stringify(isoMillis(requireInstantMs(timestamp, "timestamp"))));
    }
    if (uuid !== undefined) {
        if (!/^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$/.test(uuid)) {
            throw invalidInput("uuid must be a UUID, got " + show(uuid));
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
