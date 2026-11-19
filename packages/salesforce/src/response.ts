// Response side of the Salesforce client: the result types, the raw API shapes
// they are decoded from, and error codes and messages. Internal: `src/lib.ts`
// re-exports the result types.

import { FieldValue, isCursor } from "./request";

/**
 * A record of any object. `fields` holds the values read, keyed by API name as
 * Salesforce spells it; a parent's fields are keyed by their path, such as
 * `Account.Name`, and a compound field's parts likewise, such as
 * `BillingAddress.city`. A parent that is not set reads as its path with `null`.
 */
export interface SalesforceRecord {
    /** Object API name, such as `Opportunity`. */
    object: string;
    /** 18-character record id. */
    id: string;
    /** The values read, by API name or path. */
    fields: Record<string, FieldValue>;
}

/** One page of results. */
export interface Page<T> {
    /** This page's records. */
    records: T[];
    /** How many records the whole query matches. */
    totalSize: number;
    /** True on the last page. */
    done: boolean;
    /** Pass back as `cursor`, with the same other arguments, for the next page; null on the last page. */
    nextCursor: string | null;
}

/** One row of an aggregate. */
export interface AggregateRow {
    /** Group values by label, such as `{ StageName: "Prospecting", "calendar_month(CloseDate)": 11 }`. */
    groups: Record<string, FieldValue>;
    /** Measures by label, such as `{ "count()": 12, "sum(Amount)": 480000 }`; null where no record had a value. */
    measures: Record<string, FieldValue>;
}

/** The result of an aggregate. */
export interface AggregateResult {
    /** One row per group, or one row without `groupBy`. */
    rows: AggregateRow[];
    /** The group labels, in the order given. */
    groups: string[];
    /** The measure labels, in the order given. */
    measures: string[];
}

/** One record's outcome in a batch write. */
export interface BatchOutcome {
    /** The record's id, or null when it was not written. */
    id: string | null;
    /** True when the record was written. */
    success: boolean;
    /** Why it failed; empty on success. With all-or-none writes, records that were valid fail with `ALL_OR_NONE_OPERATION_ROLLED_BACK`. */
    errors: RecordError[];
}

/** One error Salesforce reported for one record. */
export interface RecordError {
    /** Salesforce's code, such as `REQUIRED_FIELD_MISSING`. */
    errorCode: string;
    /** Salesforce's message. */
    message: string;
    /** The fields the error names. */
    fields: string[];
}

/** The result of an upsert. */
export interface UpsertResult {
    /** The record's id. */
    id: string;
    /** True when the record was created, false when an existing one was updated. */
    created: boolean;
}

/** Records changed in a time window. */
export interface UpdatedRecords {
    /** Ids of the records created or updated in the window. */
    ids: string[];
    /** The end of the window Salesforce covered. */
    latestDateCovered: string;
}

/** Records deleted in a time window. */
export interface DeletedRecords {
    /** The deleted records. */
    records: DeletedRecord[];
    /** The earliest time Salesforce still has deletions for. */
    earliestDateAvailable: string;
    /** The end of the window Salesforce covered. */
    latestDateCovered: string;
}

/** A deleted record. */
export interface DeletedRecord {
    /** The record's id. */
    id: string;
    /** When it was deleted. */
    deletedDate: string;
}

export interface ApiQueryResult {
    totalSize: number;
    done: boolean;
    nextRecordsUrl?: string | null;
    records: Record<string, unknown>[];
}

export interface ApiSearchResult {
    searchRecords: Record<string, unknown>[];
}

export interface ApiSaveResult {
    id?: string | null;
    success: boolean;
    created?: boolean;
    errors?: ApiRecordError[];
}

export interface ApiRecordError {
    statusCode?: string;
    errorCode?: string;
    message?: string;
    fields?: string[];
}

export interface ApiUpdated {
    ids: string[];
    latestDateCovered: string;
}

export interface ApiDeleted {
    deletedRecords: ApiDeletedRecord[];
    earliestDateAvailable: string;
    latestDateCovered: string;
}

export interface ApiDeletedRecord {
    id: string;
    deletedDate: string;
}

/**
 * Flatten a raw record: its fields keyed by API name, a parent's or compound
 * field's parts keyed by path, and Salesforce's `attributes` left out.
 * @param raw The record as Salesforce returned it.
 * @returns The record.
 */
export function recordFrom(raw: Record<string, unknown>): SalesforceRecord {
    const fields: Record<string, FieldValue> = {};
    flatten("", raw, fields);
    return { object: typeOf(raw), id: idOf(raw), fields: fields };
}

/**
 * Flatten a raw record's fields only.
 * @param raw The record as Salesforce returned it.
 * @returns Its fields by path.
 */
export function fieldsOf(raw: Record<string, unknown>): Record<string, FieldValue> {
    const fields: Record<string, FieldValue> = {};
    flatten("", raw, fields);
    return fields;
}

function flatten(prefix: string, raw: Record<string, unknown>, out: Record<string, FieldValue>): void {
    for (const key of Object.keys(raw)) {
        if (key === "attributes") continue;
        const value = raw[key];
        const path = prefix.length > 0 ? prefix + "." + key : key;
        if (value === null || value === undefined) out[path] = null;
        else if (typeof value === "string" || typeof value === "number" || typeof value === "boolean") out[path] = value;
        else if (typeof value === "object" && !Array.isArray(value)) flatten(path, value as Record<string, unknown>, out);
        else out[path] = JSON.stringify(value);
    }
}

// The object API name from a raw record's `attributes.type`, or "".
export function typeOf(raw: Record<string, unknown>): string {
    const attributes = raw["attributes"];
    if (attributes === null || attributes === undefined || typeof attributes !== "object") return "";
    const type = (attributes as Record<string, unknown>)["type"];
    return typeof type === "string" ? type : "";
}

// The record id from `Id`, else from the tail of `attributes.url`, else "".
export function idOf(raw: Record<string, unknown>): string {
    const id = raw["Id"];
    if (typeof id === "string") return id;
    const attributes = raw["attributes"];
    if (attributes === null || attributes === undefined || typeof attributes !== "object") return "";
    const url = (attributes as Record<string, unknown>)["url"];
    if (typeof url !== "string") return "";
    const slash = url.lastIndexOf("/");
    return slash >= 0 ? url.slice(slash + 1) : "";
}

// A text field of a raw record, or "" when absent, null or not text.
export function textField(raw: Record<string, unknown>, field: string): string {
    const value = raw[field];
    return typeof value === "string" ? value : "";
}

/**
 * A page of results.
 * @param result The raw query result.
 * @param map Decodes one raw record.
 * @returns The page, with the cursor of the next one.
 */
export function pageFrom<T>(result: ApiQueryResult, map: (raw: Record<string, unknown>) => T): Page<T> {
    const records: T[] = [];
    for (const raw of result.records) records.push(map(raw));
    return { records: records, totalSize: result.totalSize, done: result.done, nextCursor: cursorFrom(result) };
}

// The locator in `nextRecordsUrl` (`/services/data/v68.0/query/01g…-2000`), or null on the last page.
export function cursorFrom(result: ApiQueryResult): string | null {
    const next = result.nextRecordsUrl;
    if (result.done || next === null || next === undefined) return null;
    const slash = next.lastIndexOf("/");
    const cursor = slash >= 0 ? next.slice(slash + 1) : next;
    return isCursor(cursor) ? cursor : null;
}

/**
 * Aggregate rows, keyed back to their labels from the aliases `g0…` and `m0…`.
 * @param result The raw query result.
 * @param groups The group labels.
 * @param measures The measure labels.
 * @returns The rows and labels.
 */
export function aggregateFrom(result: ApiQueryResult, groups: string[], measures: string[]): AggregateResult {
    const rows: AggregateRow[] = [];
    for (const raw of result.records) {
        const groupValues: Record<string, FieldValue> = {};
        const measureValues: Record<string, FieldValue> = {};
        for (let i = 0; i < groups.length; i += 1) groupValues[groups[i]] = scalarOf(raw["g" + i.toString()]);
        for (let i = 0; i < measures.length; i += 1) measureValues[measures[i]] = scalarOf(raw["m" + i.toString()]);
        rows.push({ groups: groupValues, measures: measureValues });
    }
    return { rows: rows, groups: groups, measures: measures };
}

function scalarOf(value: unknown): FieldValue {
    if (typeof value === "string" || typeof value === "number" || typeof value === "boolean") return value;
    return null;
}

/**
 * Each record's outcome in a composite write.
 * @param results The raw results, in the order of the records sent.
 * @returns The outcomes.
 */
export function batchFrom(results: ApiSaveResult[]): BatchOutcome[] {
    const out: BatchOutcome[] = [];
    for (const result of results) {
        const errors: RecordError[] = [];
        for (const error of result.errors ?? []) errors.push(recordErrorFrom(error));
        const id = result.id ?? null;
        out.push({ id: id !== null && id.length > 0 ? id : null, success: result.success, errors: errors });
    }
    return out;
}

function recordErrorFrom(error: ApiRecordError): RecordError {
    return {
        errorCode: error.statusCode ?? error.errorCode ?? "UNKNOWN",
        message: error.message ?? "",
        fields: error.fields ?? [],
    };
}

/** What an error response says, decoded. */
export interface FailureDetails {
    errorCode: string;
    message: string;
    fields: string[];
    retryable: boolean;
}

/**
 * Decode an error response: Salesforce's `[{ errorCode, message, fields }]`,
 * an OAuth error `{ error, error_description }`, or anything else, mapped from
 * the HTTP status.
 * @param status HTTP status.
 * @param statusText HTTP status text.
 * @param body The response body.
 * @returns The error's code, message, fields and whether it is retryable.
 */
export function failureFrom(status: number, statusText: string, body: string): FailureDetails {
    let parsed: unknown = null;
    try {
        parsed = JSON.parse(body);
    } catch (error) {
        parsed = null;
    }
    let first: unknown = parsed;
    if (Array.isArray(parsed)) first = parsed.length > 0 ? parsed[0] : null;
    // An invocable action's or approval's failure: its result, with the errors inside.
    if (arrayAt(first, "errors").length > 0) first = arrayAt(first, "errors")[0];
    const error = errorDetails(first);
    const code = error.code.length > 0 ? error.code : statusCode(status);
    const message = error.detail.length > 0
        ? "Salesforce " + status.toString() + " " + code + ": " + error.detail
        : "Salesforce request failed: HTTP " + status.toString() + " " + statusText;
    const retryable = status === 429 || status === 503
        || code === "REQUEST_LIMIT_EXCEEDED" || code === "UNABLE_TO_LOCK_ROW" || code === "SERVER_UNAVAILABLE";
    return { errorCode: code, message: message, fields: error.fields, retryable: retryable };
}

// A code for an error answer without one.
export function statusCode(status: number): string {
    if (status === 400) return "BAD_REQUEST";
    if (status === 401) return "INVALID_SESSION_ID";
    if (status === 403) return "FORBIDDEN";
    if (status === 404) return "NOT_FOUND";
    if (status === 409) return "CONFLICT";
    if (status === 429) return "REQUEST_LIMIT_EXCEEDED";
    if (status >= 500) return "SERVER_ERROR";
    return "HTTP_" + status.toString();
}

// Reading Salesforce's JSON where its shape is not worth a type: each returns
// the value at `key` when it has the expected kind, else an empty value.

export function objectAt(raw: unknown, key: string): Record<string, unknown> {
    if (raw === null || raw === undefined || typeof raw !== "object" || Array.isArray(raw)) return {};
    const value = (raw as Record<string, unknown>)[key];
    if (value === null || value === undefined || typeof value !== "object" || Array.isArray(value)) return {};
    return value as Record<string, unknown>;
}

export function arrayAt(raw: unknown, key: string): unknown[] {
    if (raw === null || raw === undefined || typeof raw !== "object" || Array.isArray(raw)) return [];
    const value = (raw as Record<string, unknown>)[key];
    return Array.isArray(value) ? value : [];
}

export function textAt(raw: unknown, key: string): string | null {
    if (raw === null || raw === undefined || typeof raw !== "object" || Array.isArray(raw)) return null;
    const value = (raw as Record<string, unknown>)[key];
    return typeof value === "string" ? value : null;
}

export function numberAt(raw: unknown, key: string): number | null {
    if (raw === null || raw === undefined || typeof raw !== "object" || Array.isArray(raw)) return null;
    const value = (raw as Record<string, unknown>)[key];
    return typeof value === "number" ? value : null;
}

export function flagAt(raw: unknown, key: string): boolean {
    if (raw === null || raw === undefined || typeof raw !== "object" || Array.isArray(raw)) return false;
    return (raw as Record<string, unknown>)[key] === true;
}

// A JSON value as a field value: a scalar as it is, anything else as its JSON text.
export function scalarOrJson(value: unknown): FieldValue {
    if (value === null || value === undefined) return null;
    if (typeof value === "string" || typeof value === "number" || typeof value === "boolean") return value;
    return JSON.stringify(value);
}

// The errors an action (`actions/...`, `process/approvals`) reported in a 200 answer, as a SalesforceError.
export function actionFailure(result: unknown, what: string): FailureDetails {
    const errors = arrayAt(result, "errors");
    const error = errorDetails(errors.length > 0 ? errors[0] : null);
    const code = error.code.length > 0 ? error.code : "ACTION_FAILED";
    return { errorCode: code, message: what + " failed: " + code + (error.detail.length > 0 ? ": " + error.detail : ""), fields: error.fields, retryable: false };
}

// One error object as Salesforce writes it: `{ errorCode, message, fields }`, an
// action's `{ statusCode, message, fields }`, or OAuth's `{ error, error_description }`.
// Absent parts are "" and [].
function errorDetails(error: unknown): { code: string; detail: string; fields: string[] } {
    const code = textAt(error, "errorCode") ?? textAt(error, "statusCode") ?? textAt(error, "error") ?? "";
    const detail = textAt(error, "message") ?? textAt(error, "error_description") ?? "";
    const fields: string[] = [];
    for (const field of arrayAt(error, "fields")) {
        if (typeof field === "string") fields.push(field);
    }
    return { code: code, detail: detail, fields: fields };
}
