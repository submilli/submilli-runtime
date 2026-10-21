// Request side of the Sentry client: the option and triage-input types, query
// and body builders, and local input validation. Internal: `src/lib.ts`
// re-exports the types, and programs reach the helpers only through its operations.

import { encodeComponent } from "submilli:url";
import { validationError } from "./errors";

const DEFAULT_LIMIT = 50;
const MAX_LIMIT = 100;

/** Supported Sentry issue sort orders. */
export type IssueSort = "date" | "freq" | "inbox" | "new" | "recommended" | "trends" | "user";

/** Filters and cursor pagination for listIssues. */
export interface ListIssuesOptions {
    /** Number of results, from 1 through 100. Defaults to 50. */
    limit?: number;
    /** Opaque cursor returned by a previous page. */
    cursor?: string;
    /** Repeated project ID filters. */
    projects?: string[];
    /** Repeated environment filters. */
    environments?: string[];
    /** Sentry search query; an explicit empty string includes all issues. */
    query?: string;
    /** Relative time period such as 24h. */
    statsPeriod?: string;
    /** ISO range start; must be paired with end. */
    start?: string;
    /** ISO range end; must be paired with start. */
    end?: string;
    /** Statistics bucket period. */
    groupStatsPeriod?: string;
    /** Enable Sentry's short-ID query lookup behavior. */
    shortIdLookup?: boolean;
    /** Result sort order. */
    sort?: IssueSort;
}

/** Filters and cursor pagination for listIssueEvents. */
export interface ListIssueEventsOptions {
    /** Number of results, from 1 through 100. Defaults to 50. */
    limit?: number;
    /** Opaque cursor returned by a previous page. */
    cursor?: string;
    /** Repeated environment filters. */
    environments?: string[];
    /** Sentry event search query. */
    query?: string;
    /** Relative time period such as 24h. */
    statsPeriod?: string;
    /** ISO range start; must be paired with end. */
    start?: string;
    /** ISO range end; must be paired with start. */
    end?: string;
    /** Ask Sentry to include full event bodies in list results. */
    full?: boolean;
    /** Return events in Sentry's deterministic pseudo-random order. */
    sample?: boolean;
}

/** Issue statuses supported by the core triage surface. */
export type IssueStatus = "resolved" | "resolvedInNextRelease" | "unresolved" | "ignored";
/** Issue priorities supported by Sentry. */
export type IssuePriority = "low" | "medium" | "high";
/** Issue substatuses documented by Sentry's update endpoint. */
export type IssueSubstatus =
    "archived_until_escalating" |
    "archived_until_condition_met" |
    "archived_forever" |
    "escalating" |
    "ongoing" |
    "regressed" |
    "new";

/** Partial core triage changes for updateIssue. */
export interface UpdateIssueInput {
    /** New workflow status. */
    status?: IssueStatus;
    /** New substatus, supplied together with a compatible status. */
    substatus?: IssueSubstatus;
    /** User or team assignment accepted by Sentry's assignedTo field. */
    assignedTo?: string;
    /** Explicitly clear the current assignee. */
    clearAssignee?: boolean;
    /** New issue priority. */
    priority?: IssuePriority;
}

/**
 * Build Sentry's repeated-parameter issue query.
 * @param options Issue filters and pagination; omit it for the default page size only.
 * @returns Query string beginning with `?`, with each project and environment as a repeated parameter.
 */
export function buildIssueQuery(options: ListIssuesOptions = {}): string {
    const { limit, projects, environments, cursor, query, statsPeriod, start, end, groupStatsPeriod, sort, shortIdLookup } = options;
    const parts: string[] = [];
    addQuery(parts, "limit", pageLimit(limit).toString());
    addRepeated(parts, "project", projects);
    addRepeated(parts, "environment", environments);
    addOptionalQuery(parts, "cursor", cursor, false);
    addOptionalQuery(parts, "query", query, true);
    addOptionalQuery(parts, "statsPeriod", statsPeriod, false);
    addOptionalQuery(parts, "start", start, false);
    addOptionalQuery(parts, "end", end, false);
    addOptionalQuery(parts, "groupStatsPeriod", groupStatsPeriod, false);
    addOptionalQuery(parts, "sort", sort, false);
    if (shortIdLookup !== undefined) addQuery(parts, "shortIdLookup", shortIdLookup ? "1" : "0");
    validateTimeRange(statsPeriod, start, end);
    return "?" + parts.join("&");
}

/**
 * Build the partial update JSON body.
 * @param input Triage changes; validated before serializing.
 * @returns JSON body containing only the supplied fields; `clearAssignee` becomes `"assignedTo":null`.
 */
export function buildUpdateIssueBody(input: UpdateIssueInput): string {
    validateUpdate(input);
    const fields: string[] = [];
    if (input.status !== undefined) fields.push(jsonProperty("status", JSON.stringify(input.status)));
    if (input.substatus !== undefined) fields.push(jsonProperty("substatus", JSON.stringify(input.substatus)));
    if (input.assignedTo !== undefined) fields.push(jsonProperty("assignedTo", JSON.stringify(input.assignedTo)));
    if (input.clearAssignee === true) fields.push(jsonProperty("assignedTo", "null"));
    if (input.priority !== undefined) fields.push(jsonProperty("priority", JSON.stringify(input.priority)));
    return "{" + fields.join(",") + "}";
}

/**
 * Return whether a validated issue reference needs Sentry's short-ID resolver.
 *
 * Sentry builds a short ID as `PROJECT-<counter>` with the counter in base32,
 * not decimal — `RUST-7E` is an ordinary one. Requiring digits rejects most real
 * short IDs, so the suffix test is "uppercase alphanumeric", which still tells a
 * short ID apart from the all-digit group ID this resolver exists to pass
 * through, and from a lowercase slug like `not-an-id`.
 * @param issueId Issue reference, already validated as non-empty text.
 * @returns `true` for a short ID such as `PROJECT-7E`; `false` for a numeric group ID or any other text.
 */
export function isShortIssueId(issueId: string): boolean {
    if (issueId.length === 0) return false;
    const parts = issueId.split("-");
    if (parts.length < 2) return false;
    const suffix = parts[parts.length - 1];
    if (suffix.length === 0) return false;
    for (let i = 0; i < suffix.length; i += 1) {
        const code = suffix.charCodeAt(i);
        const isDigit = code >= 48 && code <= 57;
        const isUpper = code >= 65 && code <= 90;
        if (!isDigit && !isUpper) return false;
    }
    const prefixLength = issueId.length - suffix.length - 1;
    return prefixLength > 0;
}

export function buildPageQuery(limit: number | undefined, cursor: string | undefined): string {
    const parts: string[] = [];
    addQuery(parts, "per_page", pageLimit(limit).toString());
    addOptionalQuery(parts, "cursor", cursor, false);
    return "?" + parts.join("&");
}

export function buildEventQuery(options: ListIssueEventsOptions): string {
    const { limit, environments, cursor, query, statsPeriod, start, end, full, sample } = options;
    const parts: string[] = [];
    addQuery(parts, "per_page", pageLimit(limit).toString());
    addRepeated(parts, "environment", environments);
    addOptionalQuery(parts, "cursor", cursor, false);
    addOptionalQuery(parts, "query", query, true);
    addOptionalQuery(parts, "statsPeriod", statsPeriod, false);
    addOptionalQuery(parts, "start", start, false);
    addOptionalQuery(parts, "end", end, false);
    if (full !== undefined) addQuery(parts, "full", full ? "1" : "0");
    if (sample !== undefined) addQuery(parts, "sample", sample ? "1" : "0");
    validateTimeRange(statsPeriod, start, end);
    return "?" + parts.join("&");
}

function addRepeated(parts: string[], key: string, values: string[] | undefined): void {
    if (values === undefined) return;
    for (const value of values) addQuery(parts, key, value);
}

function addOptionalQuery(parts: string[], key: string, value: string | undefined, allowEmpty: boolean): void {
    if (value === undefined || (!allowEmpty && value.length === 0)) return;
    addQuery(parts, key, value);
}

function addQuery(parts: string[], key: string, value: string): void {
    parts.push(encodeComponent(key) + "=" + encodeComponent(value));
}

function pageLimit(value: number | undefined): number {
    const limit = value ?? DEFAULT_LIMIT;
    if (limit < 1 || limit > MAX_LIMIT || limit !== Math.floor(limit)) {
        throw validationError("invalid_page_size", "limit must be an integer between 1 and 100");
    }
    return limit;
}

function validateTimeRange(statsPeriod: string | undefined, start: string | undefined, end: string | undefined): void {
    if (statsPeriod !== undefined && (start !== undefined || end !== undefined)) {
        throw validationError("invalid_time_range", "statsPeriod cannot be combined with start or end");
    }
    if ((start === undefined) !== (end === undefined)) {
        throw validationError("invalid_time_range", "start and end must be provided together");
    }
}

export function validateUpdate(input: UpdateIssueInput): void {
    const status: string = input.status ?? "";
    const substatus: string = input.substatus ?? "";
    if (input.assignedTo !== undefined && input.clearAssignee === true) {
        throw validationError("invalid_update", "assignedTo and clearAssignee cannot be combined");
    }
    if (input.assignedTo !== undefined && input.assignedTo.length === 0) {
        throw validationError("invalid_update", "assignedTo cannot be empty; use clearAssignee instead");
    }
    if (substatus.length > 0 && status.length === 0) {
        throw validationError("invalid_update", "substatus requires status in the same update");
    }
    if (substatus.length > 0 && (status === "resolved" || status === "resolvedInNextRelease")) {
        throw validationError("invalid_update", "resolved statuses cannot be combined with substatus");
    }
    if (substatus.length > 0 && status === "ignored" && !substatus.startsWith("archived_")) {
        throw validationError("invalid_update", "ignored status requires an archived substatus");
    }
    if (substatus.length > 0 && status === "unresolved" && substatus.startsWith("archived_")) {
        throw validationError("invalid_update", "unresolved status cannot use an archived substatus");
    }
    if (
        input.status === undefined && input.substatus === undefined && input.assignedTo === undefined &&
        input.clearAssignee !== true && input.priority === undefined
    ) {
        throw validationError("empty_update", "updateIssue requires at least one change");
    }
}

export function requireText(value: string, name: string): string {
    if (value.length === 0) throw validationError("invalid_input", name + " cannot be empty");
    return value;
}

function jsonProperty(name: string, value: string): string {
    return JSON.stringify(name) + ":" + value;
}
