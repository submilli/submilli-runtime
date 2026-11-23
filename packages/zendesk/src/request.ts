// Request side of the Zendesk client: the input types shared by operations,
// and the validation of ids, text, dates, tags, custom field values, pages and
// search terms. Internal: `src/lib.ts` re-exports the input types, and
// programs reach the helpers only through its operations.

import { encodeComponent } from "submilli:url";
import { validationError } from "./errors";

const DEFAULT_PAGE_SIZE = 100;
const MAX_PAGE_SIZE = 100;
const MAX_CURSOR = 1024;
const MAX_SEARCH_RESULTS = 1000;
const MAX_TAG = 100;

/** A ticket's status. */
export type TicketStatus = "new" | "open" | "pending" | "hold" | "solved" | "closed";
/** A ticket's priority. */
export type TicketPriority = "low" | "normal" | "high" | "urgent";
/** A ticket's type. */
export type TicketType = "question" | "incident" | "problem" | "task";

const STATUSES = ["new", "open", "pending", "hold", "solved", "closed"];
const PRIORITIES = ["low", "normal", "high", "urgent"];
const TYPES = ["question", "incident", "problem", "task"];

/**
 * A custom field's value: text (also dates `YYYY-MM-DD` and drop-down tags), a
 * number, a checkbox, a multi-select's tags, or null to clear it.
 */
export type CustomValue = string | number | boolean | string[] | null;

/** One custom field to write: its id, from `listTicketFields`, and its value. */
export interface CustomFieldWrite {
    /** The ticket field's id. */
    id: string;
    /** The value; null clears it. */
    value: CustomValue;
}

/** Which page to read. */
export interface PageOptions {
    /** Items per page, 1 to 100 (default 100). */
    pageSize?: number;
    /** `nextCursor` from the previous page; omit it for the first page. Pass the same other arguments with it. */
    cursor?: string;
}

/** A validated page request. */
export interface PageWindow {
    pageSize: number;
    cursor: string | null;
}

/**
 * Validate a Zendesk id: decimal digits, a safe integer.
 * @param value The id as given.
 * @param argument Argument name for the error message.
 * @returns The id without leading zeros.
 */
export function normalizeId(value: string, argument: string): string {
    const text = value.trim();
    const refused = validationError("invalid_id", argument + " must be a Zendesk id (digits, such as \"35436\"), got " + JSON.stringify(value));
    if (text.length === 0 || text.length > 16 || !isDigits(text)) throw refused;
    const number = Number(text);
    if (!Number.isSafeInteger(number) || number <= 0) throw refused;
    return number.toString();
}

// `normalizeId` for an optional argument; "" when it is not given.
export function optionalId(value: string | undefined, argument: string): string {
    return value === undefined ? "" : normalizeId(value, argument);
}

export function isDigits(text: string): boolean {
    if (text.length === 0) return false;
    for (let i = 0; i < text.length; i += 1) {
        const code = text.charCodeAt(i);
        if (code < 48 || code > 57) return false;
    }
    return true;
}

export function requireText(value: string, argument: string): string {
    const text = value.trim();
    if (text.length === 0) throw validationError("invalid_input", argument + " cannot be empty");
    return text;
}

export function ticketStatus(value: string, argument: string): TicketStatus {
    const text = value.trim().toLowerCase();
    if (!STATUSES.includes(text)) throw validationError("invalid_input", argument + " must be one of " + STATUSES.join(", ") + ", got " + JSON.stringify(value));
    return text as TicketStatus;
}

export function ticketPriority(value: string, argument: string): TicketPriority {
    const text = value.trim().toLowerCase();
    if (!PRIORITIES.includes(text)) throw validationError("invalid_input", argument + " must be one of " + PRIORITIES.join(", ") + ", got " + JSON.stringify(value));
    return text as TicketPriority;
}

export function ticketType(value: string, argument: string): TicketType {
    const text = value.trim().toLowerCase();
    if (!TYPES.includes(text)) throw validationError("invalid_input", argument + " must be one of " + TYPES.join(", ") + ", got " + JSON.stringify(value));
    return text as TicketType;
}

/**
 * Validate a tag: Zendesk lowercases tags and splits them at spaces, so a tag
 * is one word without quotes or commas.
 * @param value The tag as given.
 * @param argument Argument name for the error message.
 * @returns The lowercased tag.
 */
export function normalizeTag(value: string, argument: string): string {
    const tag = value.trim().toLowerCase();
    let ok = tag.length > 0 && tag.length <= MAX_TAG;
    for (let i = 0; i < tag.length; i += 1) {
        const code = tag.charCodeAt(i);
        if (code <= 32 || code === 34 || code === 44 || code === 127) ok = false;
    }
    if (!ok) throw validationError("invalid_tag", argument + " must be a tag: one word, without spaces, quotes or commas, got " + JSON.stringify(value));
    return tag;
}

/**
 * Validate a date `YYYY-MM-DD` or a date-time with a zone, such as
 * `2026-11-01T09:00:00Z`.
 * @param value The date as given.
 * @param argument Argument name for the error message.
 * @returns The trimmed date.
 */
export function requireDate(value: string, argument: string): string {
    const text = value.trim();
    let ok = text.length >= 10 && text.length <= 30 && isDigits(text.slice(0, 4)) && text.charAt(4) === "-"
        && isDigits(text.slice(5, 7)) && text.charAt(7) === "-" && isDigits(text.slice(8, 10));
    if (ok && text.length > 10) {
        ok = text.charAt(10) === "T";
        for (let i = 11; i < text.length; i += 1) {
            const char = text.charAt(i);
            if (!isDigits(char) && char !== ":" && char !== "." && char !== "Z" && char !== "+" && char !== "-") ok = false;
        }
    }
    if (!ok) throw validationError("invalid_date", argument + " must be a date such as 2026-11-01 or 2026-11-01T09:00:00Z, got " + JSON.stringify(value));
    return text;
}

/**
 * Validate a custom field's value: finite numbers, and a multi-select's tags.
 * @param value The value as given.
 * @param argument Argument name for the error message.
 * @returns The value to send.
 */
export function customValue(value: CustomValue, argument: string): CustomValue {
    if (typeof value === "number" && !Number.isFinite(value)) throw validationError("invalid_input", argument + " must be a finite number");
    if (Array.isArray(value)) {
        const tags: string[] = [];
        for (const tag of value) tags.push(normalizeTag(tag, argument));
        return tags;
    }
    return value;
}

export function sortedDistinct(items: string[]): string[] {
    const out: string[] = [];
    for (const item of items) {
        if (!out.includes(item)) out.push(item);
    }
    out.sort();
    return out;
}

/**
 * Validate a page request for Zendesk's cursor pagination.
 * @param page The page options as given.
 * @returns The page size and the cursor, if any.
 */
export function pageWindow(page: PageOptions): PageWindow {
    const { pageSize, cursor } = page;
    const size = pageSize ?? DEFAULT_PAGE_SIZE;
    if (!Number.isInteger(size) || size < 1 || size > MAX_PAGE_SIZE) {
        throw validationError("invalid_input", "pageSize must be an integer from 1 to 100, got " + size.toString());
    }
    if (cursor === undefined) return { pageSize: size, cursor: null };
    const text = cursor.trim();
    if (!isCursor(text)) throw validationError("invalid_cursor", "cursor must be a page's nextCursor, got " + JSON.stringify(cursor));
    return { pageSize: size, cursor: text };
}

// Zendesk's `after_cursor`: opaque, URL-safe base64-like text.
export function isCursor(text: string): boolean {
    if (text.length === 0 || text.length > MAX_CURSOR) return false;
    for (let i = 0; i < text.length; i += 1) {
        const code = text.charCodeAt(i);
        const ok = (code >= 65 && code <= 90) || (code >= 97 && code <= 122) || (code >= 48 && code <= 57)
            || code === 43 || code === 47 || code === 61 || code === 95 || code === 45 || code === 46;
        if (!ok) return false;
    }
    return true;
}

// The query string of a cursor-paged list: `page[size]` and, after the first page, `page[after]`.
export function pageQuery(window: PageWindow): string {
    const size = "page%5Bsize%5D=" + window.pageSize.toString();
    return window.cursor === null ? size : size + "&page%5Bafter%5D=" + encodeComponent(window.cursor);
}

/** A validated page of search results: Zendesk's search pages by number, up to 1,000 results. */
export interface SearchWindow {
    pageSize: number;
    page: number;
}

/**
 * Validate a page request for Zendesk's search, whose cursor is the next page's
 * number. Search returns at most 1,000 results.
 * @param page The page options as given.
 * @returns The page size and number.
 */
export function searchWindow(page: PageOptions): SearchWindow {
    const { pageSize, cursor } = page;
    const size = pageSize ?? DEFAULT_PAGE_SIZE;
    if (!Number.isInteger(size) || size < 1 || size > MAX_PAGE_SIZE) {
        throw validationError("invalid_input", "pageSize must be an integer from 1 to 100, got " + size.toString());
    }
    if (cursor === undefined) return { pageSize: size, page: 1 };
    const text = cursor.trim();
    const number = isDigits(text) && text.length <= 4 ? Number(text) : 0;
    // Page `number` starts after `(number - 1) * size` results, which must stay under 1,000.
    if (number < 2 || (number - 1) * size >= MAX_SEARCH_RESULTS) {
        throw validationError("invalid_cursor", "cursor must be a page's nextCursor, got " + JSON.stringify(cursor));
    }
    return { pageSize: size, page: number };
}

/**
 * Quote free text as a search phrase. Zendesk's search has no escape inside a
 * phrase, so double quotes and control characters are refused.
 * @param value The text as given.
 * @param argument Argument name for the error message.
 * @returns The quoted phrase.
 */
export function searchPhrase(value: string, argument: string): string {
    const text = requireText(value, argument);
    for (let i = 0; i < text.length; i += 1) {
        const code = text.charCodeAt(i);
        if (code < 32 || code === 34 || code === 127) {
            throw validationError("invalid_input", argument + " cannot contain double quotes or control characters, got " + JSON.stringify(value));
        }
    }
    return "\"" + text + "\"";
}
