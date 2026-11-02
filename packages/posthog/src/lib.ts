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
import { PostHogError } from "./errors";
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
    requireCohortIds,
    requireInstantMs,
    requireLimit,
    requireList,
    requireOffset,
    requireProjectId,
    requireSafeText,
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
export { CaptureInput, CaptureProperty, ListEventsOptions, PropertyFilter, PropertyScope } from "./request";
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
 * Read one page of events in a fixed time window, oldest first. The query reads
 * only the named events and properties; page with `nextCursor` until
 * `isComplete` before treating the window as fully read.
 * @param projectId PostHog project id, such as 602724.
 * @param options Events, window, properties to return, filters, and pagination.
 * @returns One page of rows and the cursor for the next page.
 * @capability posthog.com/events.list { projectId: number, events: string[], properties: string[], personProperties: string[] }
 */
export function listEvents(projectId: number, options: ListEventsOptions): EventPage {
    const project = requireProjectId(projectId);
    const { events, after, before, properties, personProperties, filters, excludeCohortIds, limit, cursor } = options;
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
    const eventNames = requireList(requestedEvents, "events", 1, 50);
    const selected = requireList(requestedProperties, "properties", 0, 50);
    const selectedPerson = requireList(requestedPerson, "personProperties", 0, 20);
    const owned: OwnedFilter[] = [];
    if (filters !== undefined) {
        for (const filter of filters) {
            const { scope, property, operator, values } = filter;
            const ownedValues: string[] = [];
            for (const value of values) ownedValues.push(value);
            owned.push(ownedFilter(scope, property, operator, ownedValues));
        }
    }
    if (owned.length > 50) throw new PostHogError("invalid_input", "filters may have at most 50 entries", 0, null);
    const cohorts: number[] = [];
    if (excludeCohortIds !== undefined) {
        for (const cohort of excludeCohortIds) cohorts.push(cohort);
    }
    const excluded = requireCohortIds(cohorts);
    const startMs = requireInstantMs(after, "after");
    const endMs = requireInstantMs(before, "before");
    if (endMs <= startMs) throw new PostHogError("invalid_input", "before must be later than after", 0, null);
    const pageSize = requireLimit(limit, 100, 1000);
    const position = parseCursor(cursor);
    // Every property the query reads, selected or filtered, is in the check.
    const readEvent: string[] = [];
    const readPerson: string[] = [];
    for (const name of selected) readEvent.push(name);
    for (const name of selectedPerson) readPerson.push(name);
    for (const filter of owned) {
        if (filter.scope === "person") {
            if (!readPerson.includes(filter.property)) readPerson.push(filter.property);
        } else if (!readEvent.includes(filter.property)) {
            readEvent.push(filter.property);
        }
    }
    check("posthog.com/events.list", { projectId: project, events: eventNames, properties: readEvent, personProperties: readPerson });
    const hogql = buildEventsQuery({
        events: eventNames,
        startMs: startMs,
        endMs: endMs,
        properties: selected,
        personProperties: selectedPerson,
        filters: owned,
        excludeCohortIds: excluded,
        limit: pageSize,
        cursorMs: position === null ? null : position[0],
        cursorUuid: position === null ? "" : position[1],
    });
    const response = post("https://us.posthog.com/api/projects/" + project.toString() + "/query/", queryBody(hogql), jsonHeaders(personalKey()));
    return eventPageFrom(succeeded(response).body, project, selected, selectedPerson, pageSize);
}

/**
 * List the event names PostHog has seen in a project.
 * @param projectId PostHog project id.
 * @param options Optional search text and offset pagination.
 * @returns One page of event definitions.
 * @capability posthog.com/schema.read { projectId: number }
 */
export function listEventDefinitions(projectId: number, options: EventDefinitionOptions = {}): DefinitionPage<EventDefinition> {
    const project = requireProjectId(projectId);
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
    const project = requireProjectId(projectId);
    const { type, eventNames, search, limit, offset } = options;
    const kind = type === undefined ? "event" : type;
    if (kind !== "event" && kind !== "person") throw new PostHogError("invalid_input", "type must be event or person", 0, null);
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
    const project = requireProjectId(projectId);
    const cohort = requireProjectId(cohortId);
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
    if (key === undefined) throw new PostHogError("missing_credentials", "The PostHog project API key is not configured for this blueprint", 0, null);
    const body = "{\"api_key\":" + JSON.stringify(key) + "," + fields + "}";
    const response = post("https://us.i.posthog.com/i/v0/e/", body, jsonHeaders(""));
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
    if (key === undefined) throw new PostHogError("missing_credentials", "The PostHog personal API key is not configured for this blueprint", 0, null);
    return key;
}

function authHeaders(key: string): Map<string, string> {
    const headers = new Map<string, string>();
    headers.set("Authorization", "Bearer " + key);
    headers.set("Accept", "application/json");
    return headers;
}

function jsonHeaders(key: string): Map<string, string> {
    const headers = key === "" ? new Map<string, string>() : authHeaders(key);
    headers.set("Content-Type", "application/json");
    return headers;
}

function succeeded(response: Response): Response {
    if (response.ok) return response;
    const retry = response.headers.get("retry-after");
    const seconds = retry === undefined || retry === null || !/^[0-9]{1,6}$/.test(retry) ? null : parseInt(retry, 10);
    const detail = response.body.length > 300 ? response.body.substring(0, 300) : response.body;
    const code = response.status === 429 && detail.includes("api_queries_budget_exceeded") ? "budget_exceeded" : "http_" + response.status.toString();
    throw new PostHogError(code, "PostHog returned HTTP " + response.status.toString() + ": " + detail, response.status, seconds);
}
