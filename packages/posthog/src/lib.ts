// A PostHog Cloud (US) client: bounded event queries over HogQL, schema
// discovery, cohort state, and event capture.
//
// Reads use a personal API key from the POSTHOG_PERSONAL_API_KEY secret
// (Query Read, plus Event/Property Definition Read and Cohort Read for the
// schema and cohort operations). Capture uses the project API key from the
// POSTHOG_PROJECT_API_KEY secret. Neither key is ever passed in or returned.
//
// There is deliberately no raw-SQL operation: `listEvents` builds its own
// HogQL, so a blueprint can see and limit the project, the event names, and
// every property a query reads.

import { get, post, Response } from "submilli:http";
import secrets from "submilli:secrets";
import { check } from "submilli:security";
import { httpError, invalidInput, missingCredentials } from "./errors";
import {
    buildEventsQuery,
    CaptureInput,
    CaptureProperty,
    captureFields,
    definitionsQuery,
    ListEventsOptions,
    OwnedFilter,
    ownedFilter,
    parseCursor,
    queryBody,
    readProperties,
    requireCohortIds,
    requireFields,
    requireFilterCount,
    requireId,
    requireInstantMs,
    requireLimit,
    requireList,
    requireOffset,
    requirePropertyNames,
    requireSafeText,
    show,
} from "./request";
import {
    Cohort,
    cohortFrom,
    DefinitionPage,
    EventDefinition,
    eventDefinitionsFrom,
    EventPage,
    eventPageFrom,
    PropertyDefinition,
    propertyDefinitionsFrom,
} from "./response";

export { PostHogError } from "./errors";
export { CaptureInput, CaptureProperty, EventField, ListEventsOptions, PropertyFilter, PropertyScope } from "./request";
export { Cohort, DefinitionPage, EventDefinition, EventPage, EventRow, PropertyDefinition, PropertyValue } from "./response";

/** Options for `listEventDefinitions`. */
export interface EventDefinitionOptions {
    /** Return only names containing this text. */
    search?: string;
    /** Page size, 1–500. Defaults to 100. */
    limit?: number;
    /** Offset from a previous page's `nextOffset`. */
    offset?: number;
}

/** Options for `listPropertyDefinitions`. */
export interface PropertyDefinitionOptions {
    /** `event` (default) or `person` properties. */
    type?: "event" | "person";
    /** For event properties, only those seen on these events. Up to 20. */
    eventNames?: string[];
    /** Return only names containing this text. */
    search?: string;
    /** Page size, 1–500. Defaults to 100. */
    limit?: number;
    /** Offset from a previous page's `nextOffset`. */
    offset?: number;
}

/** What PostHog answered to a capture. */
export interface CaptureResult {
    /** True when PostHog accepted the event (HTTP 200). Acceptance is not proof of ingestion. */
    accepted: boolean;
}

/**
 * Read one page of events in a fixed time window, oldest first, and check
 * each property path or events-table field it reads (properties.read). Page
 * with `nextCursor` until `isComplete` before treating the window as read.
 * @param projectId PostHog project id, such as 602724.
 * @param options Events, window, properties to return, filters, and pagination.
 * @returns One page of rows and the cursor for the next page.
 * @capability posthog.com/events.list { projectId: number, events: string[], properties: string[], personProperties: string[], fields: string[], excludeCohortIds: number[] }
 * @capability posthog.com/properties.read { projectId: number, scope: string, property: string }
 */
export function listEvents(projectId: number, options: ListEventsOptions): EventPage {
    const project = requireId(projectId, "projectId");
    const { events, after, before, properties, personProperties, fields, filters, excludeCohortIds, limit, cursor } = options;
    // Hand-written copies, not a helper: the read-once checker requires each
    // caller array that reaches check() to be copied in this function.
    const requestedEvents: string[] = [];
    for (const name of events) requestedEvents.push(name);
    const requestedProperties: string[] = [];
    if (properties !== undefined) {
        for (const name of properties) requestedProperties.push(name);
    }
    const requestedPerson: string[] = [];
    if (personProperties !== undefined) {
        for (const name of personProperties) requestedPerson.push(name);
    }
    const requestedFields: string[] = [];
    if (fields !== undefined) {
        for (const name of fields) requestedFields.push(name);
    }
    const eventNames = requireList(requestedEvents, "events", 1, 50);
    const selectedFields = requireFields(requestedFields);
    const selected = requirePropertyNames(requestedProperties, "properties", 50);
    const selectedPerson = requirePropertyNames(requestedPerson, "personProperties", 20);
    const owned: OwnedFilter[] = [];
    if (filters !== undefined) {
        for (const filter of filters) {
            const { scope, property, key, operator, values } = filter;
            const ownedValues: string[] = [];
            for (const value of values) ownedValues.push(value);
            owned.push(ownedFilter(scope, property, key, operator, ownedValues));
        }
    }
    requireFilterCount(owned.length);
    const cohorts: number[] = [];
    if (excludeCohortIds !== undefined) {
        for (const cohort of excludeCohortIds) cohorts.push(cohort);
    }
    const excluded = requireCohortIds(cohorts);
    const startMs = requireInstantMs(after, "after");
    const endMs = requireInstantMs(before, "before");
    if (Temporal.Instant.compare(Temporal.Instant.from(before), Temporal.Instant.from(after)) <= 0) {
        throw invalidInput("before must be later than after, got " + show(after) + " and " + show(before));
    }
    const pageSize = requireLimit(limit, 100, 1000);
    const position = parseCursor(cursor);
    // Every property path and field the query reads, selected or filtered, is
    // checked: as lists on events.list, and one at a time on properties.read
    // so a blueprint can allow a fixed set.
    const read = readProperties(selected, selectedPerson, selectedFields, owned);
    check("posthog.com/events.list", { projectId: project, events: eventNames, properties: read.event, personProperties: read.person, fields: read.field, excludeCohortIds: excluded });
    for (const property of read.event) check("posthog.com/properties.read", { projectId: project, scope: "event", property: property });
    for (const property of read.person) check("posthog.com/properties.read", { projectId: project, scope: "person", property: property });
    for (const property of read.field) check("posthog.com/properties.read", { projectId: project, scope: "field", property: property });
    const hogql = buildEventsQuery({
        events: eventNames,
        startMs: startMs,
        endMs: endMs,
        properties: selected,
        personProperties: selectedPerson,
        fields: selectedFields,
        filters: owned,
        excludeCohortIds: excluded,
        limit: pageSize,
        cursor: position,
    });
    const key = personalKey();
    // Both bounds round up to whole milliseconds; a window inside one
    // millisecond holds no event, so it is read as empty without a query, but
    // only after the checks and the key lookup, so a missing key still fails.
    if (endMs === startMs) return { events: [], nextCursor: "", isComplete: true };
    const response = post("https://us.posthog.com/api/projects/" + project.toString() + "/query/", queryBody(hogql), queryHeaders(key));
    return eventPageFrom(succeeded(response).body, project, selected, selectedPerson, selectedFields, pageSize);
}

/**
 * List the event names PostHog has seen in a project.
 * @param projectId PostHog project id.
 * @param options Optional search text and offset pagination.
 * @returns One page of event definitions.
 * @capability posthog.com/schema.read { projectId: number }
 */
export function listEventDefinitions(projectId: number, options: EventDefinitionOptions = {}): DefinitionPage<EventDefinition> {
    const project = requireId(projectId, "projectId");
    const { search, limit, offset } = options;
    const pageSize = requireLimit(limit, 100, 500);
    const start = requireOffset(offset);
    const query = definitionsQuery(pageSize, start, search, "");
    check("posthog.com/schema.read", { projectId: project });
    const response = get("https://us.posthog.com/api/projects/" + project.toString() + "/event_definitions/" + query, authHeaders(personalKey()));
    return eventDefinitionsFrom(succeeded(response).body, start);
}

/**
 * List the event or person property names PostHog has seen in a project.
 * @param projectId PostHog project id.
 * @param options Property type, event names, search text, and offset pagination.
 * @returns One page of property definitions.
 * @capability posthog.com/schema.read { projectId: number }
 */
export function listPropertyDefinitions(projectId: number, options: PropertyDefinitionOptions = {}): DefinitionPage<PropertyDefinition> {
    const project = requireId(projectId, "projectId");
    const { type, eventNames, search, limit, offset } = options;
    const kind = type === undefined ? "event" : type;
    if (kind !== "event" && kind !== "person") throw invalidInput("type must be event or person, got " + show(type));
    const names = requireList(copy(eventNames), "eventNames", 0, 20);
    const pageSize = requireLimit(limit, 100, 500);
    const start = requireOffset(offset);
    let extra = "&type=" + kind;
    if (names.length > 0) extra += "&event_names=" + encodeURIComponent(JSON.stringify(names)) + "&filter_by_event_names=true";
    const query = definitionsQuery(pageSize, start, search, extra);
    check("posthog.com/schema.read", { projectId: project });
    const response = get("https://us.posthog.com/api/projects/" + project.toString() + "/property_definitions/" + query, authHeaders(personalKey()));
    return propertyDefinitionsFrom(succeeded(response).body, start);
}

/**
 * Read a cohort's name, member count, and calculation state.
 * @param projectId PostHog project id.
 * @param cohortId Cohort id.
 * @returns The cohort, or null when it does not exist.
 * @capability posthog.com/cohorts.get { projectId: number, cohortId: number }
 */
export function getCohort(projectId: number, cohortId: number): Cohort | null {
    const project = requireId(projectId, "projectId");
    const cohort = requireId(cohortId, "cohortId");
    check("posthog.com/cohorts.get", { projectId: project, cohortId: cohort });
    const response = get("https://us.posthog.com/api/projects/" + project.toString() + "/cohorts/" + cohort.toString() + "/", authHeaders(personalKey()));
    if (response.status === 404) return null;
    return cohortFrom(succeeded(response).body);
}

/**
 * Send one event to the project the project API key belongs to.
 * @param input Event name, distinct id, properties, and optional time and UUID.
 * @returns Whether PostHog accepted the event.
 * @capability posthog.com/events.capture { event: string }
 */
export function capture(input: CaptureInput): CaptureResult {
    const { event, distinctId, properties, set, timestamp, uuid } = input;
    const name = requireSafeText(event, "event");
    const fields = captureFields(name, distinctId, copyProperties(properties), copyProperties(set), timestamp, uuid);
    check("posthog.com/events.capture", { event: name });
    const key = secrets.get("POSTHOG_PROJECT_API_KEY");
    if (key === undefined) throw missingCredentials("project API key");
    const body = "{\"api_key\":" + JSON.stringify(key) + "," + fields + "}";
    const headers = new Map<string, string>();
    headers.set("Content-Type", "application/json");
    const response = post("https://us.i.posthog.com/i/v0/e/", body, headers);
    succeeded(response);
    return { accepted: true };
}

function copy(values: string[] | undefined): string[] {
    const result: string[] = [];
    if (values === undefined) return result;
    for (const value of values) result.push(value);
    return result;
}

function copyProperties(values: CaptureProperty[] | undefined): CaptureProperty[] {
    const result: CaptureProperty[] = [];
    if (values === undefined) return result;
    for (const value of values) {
        const { name, value: data } = value;
        result.push({ name: name, value: data });
    }
    return result;
}

function personalKey(): string {
    const key = secrets.get("POSTHOG_PERSONAL_API_KEY");
    if (key === undefined) throw missingCredentials("personal API key");
    return key;
}

function authHeaders(key: string): Map<string, string> {
    const headers = new Map<string, string>();
    headers.set("Authorization", "Bearer " + key);
    headers.set("Accept", "application/json");
    return headers;
}

function queryHeaders(key: string): Map<string, string> {
    const headers = authHeaders(key);
    headers.set("Content-Type", "application/json");
    return headers;
}

function succeeded(response: Response): Response {
    if (response.ok) return response;
    throw httpError(response.status, response.body, response.headers.get("retry-after"));
}
