// Request side of the Salesforce client: the input types shared by every
// operation, and the normalization and validation of names, ids, values and
// pages. Internal: `src/lib.ts` re-exports the input types, and
// programs reach the helpers only through its operations.

import { validationError } from "./errors";

const DEFAULT_PAGE_SIZE = 200;
const MIN_PAGE_SIZE = 200;
const MAX_PAGE_SIZE = 2000;
const ID_ALPHABET = "ABCDEFGHIJKLMNOPQRSTUVWXYZ012345";

/** A field's value as Salesforce reads and writes it: text (also dates, date-times and picklist values), a number, a checkbox, or null for an empty field. */
export type FieldValue = string | number | boolean | null;

/** One field to write: its API name, such as `StageName` or `Region__c`, and its value; `null` clears it. */
export interface FieldWrite {
    /** Field API name. */
    field: string;
    /** The value: text (also dates `YYYY-MM-DD`, date-times and picklist values), a number, a checkbox, or null. */
    value: FieldValue;
}

/** Which page to read. */
export interface PageOptions {
    /** Records per page, 200 to 2000 (default 200). Salesforce may return fewer. */
    pageSize?: number;
    /** `nextCursor` from the previous page; omit it for the first page. Pass the same other arguments with it. */
    cursor?: string;
}

/** A validated page request. */
export interface PageWindow {
    pageSize: number;
    cursor: string | null;
}

/** A Salesforce API name, as given (trimmed) and as checked (lowercased). */
export interface ApiName {
    /** The name as the program spelled it, sent to Salesforce. */
    name: string;
    /** The lowercased name a check sees: Salesforce treats `Amount` and `amount` as the same field. */
    key: string;
}

/**
 * Normalize an object or field API name: letters, digits and underscores,
 * starting with a letter, such as `Opportunity`, `Region__c` or `ns__Thing__c`.
 * @param value The name as given.
 * @param argument Argument name for the error message.
 * @returns The trimmed name and its lowercased key.
 */
export function normalizeName(value: string, argument: string): ApiName {
    const name = value.trim();
    if (!isApiName(name)) {
        throw validationError(
            "invalid_name",
            argument + " must be a Salesforce API name such as `Opportunity` or `Region__c` (letters, digits and underscores, starting with a letter), got " + JSON.stringify(value),
        );
    }
    return { name: name, key: name.toLowerCase() };
}

/**
 * Normalize a field path: an API name, or up to five relationship names and a
 * field joined by dots, such as `Account.Owner.Name`.
 * @param value The path as given.
 * @param argument Argument name for the error message.
 * @returns The trimmed path and its lowercased key.
 */
export function normalizePath(value: string, argument: string): ApiName {
    const path = value.trim();
    const parts = path.split(".");
    let ok = parts.length <= 6;
    for (const part of parts) {
        if (!isApiName(part)) ok = false;
    }
    if (!ok) {
        throw validationError(
            "invalid_name",
            argument + " must be a field API name or a relationship path such as `Account.Name`, got " + JSON.stringify(value),
        );
    }
    return { name: path, key: path.toLowerCase() };
}

function isApiName(name: string): boolean {
    if (name.length === 0 || name.length > 120) return false;
    for (let i = 0; i < name.length; i += 1) {
        const code = name.charCodeAt(i);
        const letter = (code >= 65 && code <= 90) || (code >= 97 && code <= 122);
        const digit = code >= 48 && code <= 57;
        if (i === 0 && !letter) return false;
        if (!letter && !digit && code !== 95) return false;
    }
    return !name.endsWith("_");
}

/**
 * Normalize a record id to its 18-character form. A 15-character id is
 * case-sensitive and gains its checksum; an 18-character id has the case of
 * its first 15 characters restored from its checksum, so ids that differ only
 * in case, or in length, check and send the same value.
 * @param value The id as given.
 * @param argument Argument name for the error message.
 * @returns The 18-character id.
 */
export function normalizeId(value: string, argument: string): string {
    const id = value.trim();
    const refused = validationError(
        "invalid_id",
        argument + " must be a Salesforce record id (15 or 18 letters and digits), got " + JSON.stringify(value),
    );
    if (id.length !== 15 && id.length !== 18) throw refused;
    for (let i = 0; i < id.length; i += 1) {
        const code = id.charCodeAt(i);
        const ok = (code >= 65 && code <= 90) || (code >= 97 && code <= 122) || (code >= 48 && code <= 57);
        if (!ok) throw refused;
    }
    if (id.length === 15) return id + idChecksum(id);
    let restored = "";
    for (let chunk = 0; chunk < 3; chunk += 1) {
        const bits = ID_ALPHABET.indexOf(id.charAt(15 + chunk).toUpperCase());
        if (bits < 0) throw refused;
        for (let i = 0; i < 5; i += 1) {
            const char = id.charAt(chunk * 5 + i);
            const upper = (bits & (1 << i)) !== 0;
            const isLetter = char.toLowerCase() !== char.toUpperCase();
            if (upper && !isLetter) throw refused;
            restored += upper ? char.toUpperCase() : char.toLowerCase();
        }
    }
    return restored + idChecksum(restored);
}

// The three checksum characters of a case-sensitive 15-character id.
export function idChecksum(id: string): string {
    let suffix = "";
    for (let chunk = 0; chunk < 3; chunk += 1) {
        let bits = 0;
        for (let i = 0; i < 5; i += 1) {
            const code = id.charCodeAt(chunk * 5 + i);
            if (code >= 65 && code <= 90) bits = bits | (1 << i);
        }
        suffix += ID_ALPHABET.charAt(bits);
    }
    return suffix;
}

// Normalize an optional id filter: "" when left out, else the 18-character id.
export function optionalId(value: string | undefined, argument: string): string {
    return value === undefined ? "" : normalizeId(value, argument);
}

/**
 * Require non-blank text.
 * @param value The text as given.
 * @param argument Argument name for the error message.
 * @returns The text, trimmed.
 */
export function requireText(value: string, argument: string): string {
    const text = value.trim();
    if (text.length === 0) throw validationError("invalid_input", argument + " cannot be empty");
    return text;
}

/** A record's values, validated: the body to send and the check's sorted field keys. */
export interface PreparedValues {
    /** The JSON body: the program's names and values. */
    body: Record<string, unknown>;
    /** Sorted, distinct, lowercased field names: the check's `fields`. */
    keys: string[];
}

/**
 * Validate a record's values: every name an API name, written once in any
 * case, and every number finite.
 * @param values The values as given.
 * @param argument Argument name for the error message.
 * @param refuseOwner Refuse `OwnerId`, which changes only through `changeOwner`.
 * @returns The body and the sorted field keys.
 */
export function prepareValues(values: FieldWrite[], argument: string, refuseOwner: boolean): PreparedValues {
    const body: Record<string, unknown> = {};
    const keys: string[] = [];
    for (const write of values) {
        const field = normalizeName(write.field, argument + " field");
        if (field.key === "id") throw validationError("invalid_input", argument + " cannot set Id; pass the record id as its own argument");
        if (refuseOwner && field.key === "ownerid") {
            throw validationError("owner_field", argument + " cannot set OwnerId; ownership changes only through changeOwner");
        }
        if (keys.includes(field.key)) throw validationError("duplicate_field", argument + " sets " + field.name + " twice");
        const value = write.value;
        if (typeof value === "number" && !isFinite(value)) {
            throw validationError("invalid_input", argument + " field " + field.name + " must be a finite number");
        }
        body[field.name] = value;
        keys.push(field.key);
    }
    keys.sort();
    return { body: body, keys: keys };
}

// An id given among the values (such as `AccountId`, in any case), normalized, or "" when absent or null.
export function idAmong(values: FieldWrite[], key: string, argument: string): string {
    for (const write of values) {
        const name = write.field.trim();
        if (name.toLowerCase() !== key) continue;
        const value = write.value;
        if (value === null) return "";
        if (typeof value !== "string") throw validationError("invalid_id", argument + " " + name + " must be a record id");
        return normalizeId(value, argument + " " + name);
    }
    return "";
}

// Sorted, distinct strings.
export function sortedDistinct(items: string[]): string[] {
    const out: string[] = [];
    for (const item of items) {
        if (!out.includes(item)) out.push(item);
    }
    out.sort();
    return out;
}

/**
 * Validate a page request.
 * @param page The page options as given.
 * @returns The page size and the cursor, if any.
 */
export function pageWindow(page: PageOptions): PageWindow {
    const size = page.pageSize ?? DEFAULT_PAGE_SIZE;
    if (!Number.isInteger(size) || size < MIN_PAGE_SIZE || size > MAX_PAGE_SIZE) {
        throw validationError("invalid_input", "pageSize must be an integer from 200 to 2000, got " + size.toString());
    }
    const cursor = page.cursor;
    if (cursor === undefined) return { pageSize: size, cursor: null };
    const text = cursor.trim();
    if (!isCursor(text)) throw validationError("invalid_cursor", "cursor must be a page's nextCursor, got " + JSON.stringify(cursor));
    return { pageSize: size, cursor: text };
}

// A query locator, such as `01gD0000002HU6KIAW-2000`: an id, a dash and a record offset.
export function isCursor(text: string): boolean {
    const dash = text.indexOf("-");
    if (dash !== 15 && dash !== 18) return false;
    if (!isDigits(text.slice(dash + 1))) return false;
    for (let i = 0; i < dash; i += 1) {
        const code = text.charCodeAt(i);
        const ok = (code >= 65 && code <= 90) || (code >= 97 && code <= 122) || (code >= 48 && code <= 57);
        if (!ok) return false;
    }
    return true;
}

/**
 * Validate an ISO 8601 date-time with a zone, such as `2026-11-01T00:00:00Z`.
 * @param value The date-time as given.
 * @param argument Argument name for the error message.
 * @returns The trimmed date-time.
 */
export function requireDateTime(value: string, argument: string): string {
    const text = value.trim();
    if (!isDateTime(text)) {
        throw validationError("invalid_date", argument + " must be a date-time with a zone such as 2026-11-01T09:30:00Z, got " + JSON.stringify(value));
    }
    return text;
}

/**
 * Validate a date, `YYYY-MM-DD`.
 * @param value The date as given.
 * @param argument Argument name for the error message.
 * @returns The trimmed date.
 */
export function requireDate(value: string, argument: string): string {
    const text = value.trim();
    if (!isDate(text)) throw validationError("invalid_date", argument + " must be a date YYYY-MM-DD, got " + JSON.stringify(value));
    return text;
}

export function isDate(text: string): boolean {
    if (text.length !== 10 || text.charAt(4) !== "-" || text.charAt(7) !== "-") return false;
    if (!isDigits(text.slice(0, 4)) || !isDigits(text.slice(5, 7)) || !isDigits(text.slice(8, 10))) return false;
    const month = Number(text.slice(5, 7));
    const day = Number(text.slice(8, 10));
    return month >= 1 && month <= 12 && day >= 1 && day <= 31;
}

export function isDateTime(text: string): boolean {
    if (text.length < 20 || text.charAt(10) !== "T" || !isDate(text.slice(0, 10))) return false;
    const clock = text.slice(11, 19);
    if (clock.charAt(2) !== ":" || clock.charAt(5) !== ":") return false;
    if (!isDigits(clock.slice(0, 2)) || !isDigits(clock.slice(3, 5)) || !isDigits(clock.slice(6, 8))) return false;
    if (Number(clock.slice(0, 2)) > 23 || Number(clock.slice(3, 5)) > 59 || Number(clock.slice(6, 8)) > 59) return false;
    let rest = text.slice(19);
    if (rest.startsWith(".")) {
        let end = 1;
        while (end < rest.length && isDigits(rest.charAt(end))) end += 1;
        if (end === 1 || end > 4) return false;
        rest = rest.slice(end);
    }
    if (rest === "Z") return true;
    if (rest.length !== 6 || (rest.charAt(0) !== "+" && rest.charAt(0) !== "-") || rest.charAt(3) !== ":") return false;
    return isDigits(rest.slice(1, 3)) && isDigits(rest.slice(4, 6));
}

export function isDigits(text: string): boolean {
    if (text.length === 0) return false;
    for (let i = 0; i < text.length; i += 1) {
        const code = text.charCodeAt(i);
        if (code < 48 || code > 57) return false;
    }
    return true;
}
