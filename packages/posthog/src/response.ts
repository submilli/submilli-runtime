// Response mapping for the PostHog client. Internal: `src/lib.ts` re-exports
// the result types and calls these mappers.

import { invalidResponse, PostHogError } from "./errors";
import { formatCursor, isoMillis } from "./request";

/** One named property value. Values arrive as text; a missing property is null. */
export interface PropertyValue {
    /** Property name, as requested. */
    name: string;
    /** The value as text, or null when the event or person does not have it. */
    value: string | null;
}

/** One event row from `listEvents`. */
export interface EventRow {
    /** The event's UUID. */
    uuid: string;
    /** Event name. */
    event: string;
    /** Event time, an ISO-8601 UTC instant with millisecond precision. */
    timestamp: string;
    /** The requested event properties, in request order. */
    properties: PropertyValue[];
    /** The requested person properties, in request order. */
    personProperties: PropertyValue[];
    /** The requested events-table fields, in request order. */
    fields: PropertyValue[];
    /** The event's page in the PostHog app. */
    url: string;
}

/** One page of `listEvents` results, oldest first. */
export interface EventPage {
    /** Rows on this page, ordered by time then UUID. */
    events: EventRow[];
    /** Pass as `cursor` with the same options for the next page; empty on the last page. */
    nextCursor: string;
    /** True when this is the last page of the window. */
    isComplete: boolean;
}

/** One event definition from the project's schema. */
export interface EventDefinition {
    /** Event name. */
    name: string;
    /** When PostHog last saw this event, an ISO-8601 instant, or null. */
    lastSeenAt: string | null;
}

/** One property definition from the project's schema. */
export interface PropertyDefinition {
    /** Property name. */
    name: string;
    /** PostHog's property type, such as String, Numeric, Boolean or DateTime, or null when unset. */
    propertyType: string | null;
}

/** One page of definitions. */
export interface DefinitionPage<T> {
    /** Definitions on this page. */
    items: T[];
    /** Total definitions that match, as PostHog reports it. */
    count: number;
    /** Offset for the next page, or null on the last page. */
    nextOffset: number | null;
}

/** A cohort's calculation state. */
export interface Cohort {
    /** Cohort id. */
    id: number;
    /** Cohort name. */
    name: string;
    /** Member count at the last calculation, or null when unknown. */
    count: number | null;
    /** Whether PostHog is recalculating the cohort now. */
    isCalculating: boolean;
    /** When the cohort was last calculated, an ISO-8601 instant, or null. */
    lastCalculation: string | null;
    /** Calculation errors PostHog recorded, or 0. */
    errorsCalculating: number;
    /** Whether the cohort is deleted. */
    deleted: boolean;
}

interface QueryResponse {
    results?: unknown[] | null;
    error?: string | null;
}

interface ApiEventDefinition {
    name?: string | null;
    last_seen_at?: string | null;
}

interface ApiPropertyDefinition {
    name?: string | null;
    property_type?: string | null;
}

interface ApiDefinitionList<T> {
    count?: number | null;
    next?: string | null;
    results?: T[] | null;
}

interface ApiCohort {
    id?: number | null;
    name?: string | null;
    count?: number | null;
    is_calculating?: boolean | null;
    last_calculation?: string | null;
    errors_calculating?: number | null;
    deleted?: boolean | null;
}

// A body of the wrong shape fails the checked casts with a TypeError, or a
// read of a missing element with a MissingValueError; callers branch on
// PostHogError codes, so report it as one.
function shaped<T>(map: () => T): T {
    try {
        return map();
    } catch (error) {
        if (error instanceof PostHogError) throw error;
        throw invalidResponse("PostHog returned an unexpected response: " + error.message);
    }
}

function decode(body: string): unknown {
    try {
        return JSON.parse(body);
    } catch (error) {
        throw invalidResponse("PostHog returned a body that is not JSON");
    }
}

function text(value: unknown): string | null {
    if (value === null || value === undefined) return null;
    if (typeof value === "string") return value;
    if (typeof value === "number") return value.toString();
    if (typeof value === "boolean") return value ? "true" : "false";
    return JSON.stringify(value) ?? "";
}

function milliseconds(value: unknown): number {
    if (typeof value === "number" && Number.isInteger(value)) return value;
    if (typeof value === "string" && /^[0-9]{1,16}$/.test(value)) return parseInt(value, 10);
    throw invalidResponse("PostHog returned a row without a millisecond timestamp");
}

// The PostHog app URL of one event.
function eventUrl(projectId: number, uuid: string, timestamp: string): string {
    return "https://us.posthog.com/project/" + projectId.toString() + "/events/" + encodeURIComponent(uuid) + "/" + encodeURIComponent(timestamp);
}

/**
 * Map one HogQL page; the query fetched one row more than `limit`.
 * @param body Response body.
 * @param projectId Project id.
 * @param properties Selected event properties, in column order.
 * @param personProperties Selected person properties, in column order.
 * @param fields Selected events-table fields, in column order.
 * @param limit Requested page size.
 * @returns The page.
 */
export function eventPageFrom(body: string, projectId: number, properties: string[], personProperties: string[], fields: string[], limit: number): EventPage {
    return shaped((): EventPage => {
        const parsed = decode(body) as QueryResponse;
        if (typeof parsed.error === "string" && parsed.error.length > 0) {
            throw invalidResponse("PostHog query failed: " + parsed.error);
        }
        const rows = parsed.results;
        if (rows === undefined || rows === null) throw invalidResponse("PostHog query response has no results");
        const width = 3 + properties.length + personProperties.length + fields.length;
        const events: EventRow[] = [];
        let nextCursor = "";
        let lastMs = 0;
        for (const raw of rows) {
            if (events.length === limit) {
                // The extra row exists, so another page follows the last row kept.
                nextCursor = formatCursor(lastMs, events[events.length - 1].uuid);
                break;
            }
            const row = raw as unknown[];
            if (row.length !== width) throw invalidResponse("PostHog returned a row with " + row.length.toString() + " columns, expected " + width.toString());
            const uuid = text(row[0]);
            const event = text(row[1]);
            if (uuid === null || event === null) throw invalidResponse("PostHog returned a row without a uuid or event name");
            lastMs = milliseconds(row[2]);
            const timestamp = isoMillis(lastMs);
            events.push({
                uuid: uuid,
                event: event,
                timestamp: timestamp,
                properties: valuesAt(row, properties, 3),
                personProperties: valuesAt(row, personProperties, 3 + properties.length),
                fields: valuesAt(row, fields, 3 + properties.length + personProperties.length),
                url: eventUrl(projectId, uuid, timestamp),
            });
        }
        return { events: events, nextCursor: nextCursor, isComplete: nextCursor === "" };
    });
}

// The named columns starting at `first`, in request order.
function valuesAt(row: unknown[], names: string[], first: number): PropertyValue[] {
    const values: PropertyValue[] = [];
    for (let index = 0; index < names.length; index++) values.push({ name: names[index], value: text(row[first + index]) });
    return values;
}

// Advance by the rows PostHog returned, not the ones kept, and stop when a page is empty.
function nextOffset(next: string | null | undefined, offset: number, returned: number): number | null {
    return next === null || next === undefined || next === "" || returned === 0 ? null : offset + returned;
}

/**
 * Map an event definitions page.
 * @param body Response body.
 * @param offset Offset requested.
 * @returns The page.
 */
export function eventDefinitionsFrom(body: string, offset: number): DefinitionPage<EventDefinition> {
    return shaped((): DefinitionPage<EventDefinition> => {
        const parsed = decode(body) as ApiDefinitionList<ApiEventDefinition>;
        const items: EventDefinition[] = [];
        const results = parsed.results ?? [];
        for (const item of results) {
            if (typeof item.name === "string") items.push({ name: item.name, lastSeenAt: item.last_seen_at ?? null });
        }
        return { items: items, count: parsed.count ?? items.length, nextOffset: nextOffset(parsed.next, offset, results.length) };
    });
}

/**
 * Map a property definitions page.
 * @param body Response body.
 * @param offset Offset requested.
 * @returns The page.
 */
export function propertyDefinitionsFrom(body: string, offset: number): DefinitionPage<PropertyDefinition> {
    return shaped((): DefinitionPage<PropertyDefinition> => {
        const parsed = decode(body) as ApiDefinitionList<ApiPropertyDefinition>;
        const items: PropertyDefinition[] = [];
        const results = parsed.results ?? [];
        for (const item of results) {
            if (typeof item.name === "string") items.push({ name: item.name, propertyType: item.property_type ?? null });
        }
        return { items: items, count: parsed.count ?? items.length, nextOffset: nextOffset(parsed.next, offset, results.length) };
    });
}

/**
 * Map a cohort.
 * @param body Response body.
 * @returns The cohort.
 */
export function cohortFrom(body: string): Cohort {
    return shaped((): Cohort => {
        const parsed = decode(body) as ApiCohort;
        if (typeof parsed.id !== "number") throw invalidResponse("PostHog cohort response has no id");
        return {
            id: parsed.id,
            name: parsed.name ?? "",
            count: parsed.count ?? null,
            isCalculating: parsed.is_calculating === true,
            lastCalculation: parsed.last_calculation ?? null,
            errorsCalculating: parsed.errors_calculating ?? 0,
            deleted: parsed.deleted === true,
        };
    });
}
