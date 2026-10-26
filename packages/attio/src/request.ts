// Request side of the Attio client: the input types, input normalization and
// validation, and the JSON builders for values, queries, upserts and notes.
// Internal: `src/lib.ts` re-exports the input types, and programs reach the
// helpers only through its operations.

import { validationError } from "./errors";
import { Attribute } from "./response";

const DEFAULT_QUERY_LIMIT = 50;
const MAX_QUERY_LIMIT = 500;
const DEFAULT_NOTE_LIMIT = 50;
const MAX_NOTE_LIMIT = 50;

/** The kinds of value an attribute write can carry. */
export type AttributeValueType =
    "text" | "number" | "currency" | "checkbox" | "date" | "select" | "email" | "domain" | "record" | "member";

/**
 * One value for one attribute. Set `type` and the field it names; leave the
 * other fields out. Repeat an attribute to give a multi-value attribute
 * several values. Status attributes are not written this way; see `setStatus`.
 *
 * | `type` | Field | For attribute type |
 * | --- | --- | --- |
 * | `text` | `text` | text |
 * | `number` | `number` | number, rating |
 * | `currency` | `amount` (the attribute's currency) | currency |
 * | `checkbox` | `checked` | checkbox |
 * | `date` | `date` (`YYYY-MM-DD`) | date, timestamp |
 * | `select` | `option` (title or option id) | select |
 * | `email` | `email` | email-address |
 * | `domain` | `domain` | domain |
 * | `record` | `targetObject` and `targetRecordId` | record-reference |
 * | `member` | `email` of a workspace member | actor-reference, such as `owner` |
 */
export interface AttributeValue {
    /** Attribute slug, such as `name`, `value` or `associated_company`. */
    attribute: string;
    /** Which kind of value this is, and so which field carries it. */
    type: AttributeValueType;
    /** Text, for `text`. */
    text?: string;
    /** Number, for `number` (Attio keeps up to four decimal places). */
    number?: number;
    /** Amount in the attribute's currency, for `currency`. */
    amount?: number;
    /** Whether the box is checked, for `checkbox`. */
    checked?: boolean;
    /** Date as `YYYY-MM-DD`, for `date`. */
    date?: string;
    /** Option title (exact) or option id, for `select`. */
    option?: string;
    /** Email address, for `email`, or a workspace member's email, for `member`; trimmed and lowercased. */
    email?: string;
    /** Domain or URL, for `domain`; normalized to a bare lowercase domain. */
    domain?: string;
    /** Object slug of the referenced record, for `record`, such as `companies`. */
    targetObject?: string;
    /** Record id of the referenced record, for `record`. */
    targetRecordId?: string;
}

/** The values to write: one entry per value, in any order. */
export type RecordValuesInput = AttributeValue[];

/** The fields of one value, copied out of the caller's entry. */
export interface ValueFields {
    type: string;
    text: string | undefined;
    number: number | undefined;
    amount: number | undefined;
    checked: boolean | undefined;
    date: string | undefined;
    option: string | undefined;
    email: string | undefined;
    domain: string | undefined;
    targetObject: string | undefined;
    targetRecordId: string | undefined;
}

/** The initial status of a new record. */
export interface StatusInput {
    /** Slug of the status attribute, such as `stage`. */
    attribute: string;
    /** The status title (case-insensitive) or status id. */
    title: string;
}

/** A record to create. */
export interface CreateRecordInput {
    /** Attribute values. Status attributes are refused here; use `status`. */
    values?: RecordValuesInput;
    /** At most one status to start in, such as `{ attribute: "stage", title: "Lead" }`. */
    status?: StatusInput;
}

/** A reference to one record. */
export interface RecordReference {
    /** Object slug, such as `companies`. */
    targetObject: string;
    /** Record id. */
    targetRecordId: string;
}

/** Comparison of one query condition. */
export type QueryOperator = "eq" | "contains" | "starts_with" | "ends_with" | "lt" | "lte" | "gt" | "gte";

/** The value a query condition compares with. A `RecordReference` matches a record-reference attribute (operator `eq` only). */
export type QueryValue = string | number | boolean | RecordReference;

/** One query condition. Conditions in a query are combined with AND. */
export interface QueryCondition {
    /** Attribute slug, such as `name`, `stage` or `associated_company`. */
    attribute: string;
    /** Comparison. `eq` works for every attribute type (status and select compare titles); text-like attributes also take `contains`, `starts_with` and `ends_with`; numbers, currencies and dates take `lt`, `lte`, `gt` and `gte`. */
    operator: QueryOperator;
    /** The value to compare with. */
    value: QueryValue;
    /** Optional property of the value to compare, such as `email_domain` of an email address, `root_domain` of a domain or `last_name` of a person's name. */
    field?: string;
}

/** One sort key. */
export interface QuerySort {
    /** Attribute slug to sort by. */
    attribute: string;
    /** `asc` or `desc`. */
    direction: "asc" | "desc";
    /** Optional property of the value to sort by, such as `last_name`. */
    field?: string;
}

/** A typed record query: AND-combined conditions, sorts, and limit/offset pagination. */
export interface RecordQuery {
    /** Conditions that must all match. Omit for every record. */
    filter?: QueryCondition[];
    /** Sort keys, applied in order. */
    sorts?: QuerySort[];
    /** Page size, 1 through 500. Defaults to 50. */
    limit?: number;
    /** Number of records to skip; pass a previous page's `nextOffset`. Defaults to 0. */
    offset?: number;
}

/** Limit/offset pagination for note lists. */
export interface PageOptions {
    /** Page size, 1 through 50. Defaults to 50. */
    limit?: number;
    /** Number of items to skip; pass a previous page's `nextOffset`. Defaults to 0. */
    offset?: number;
}

/** A person to find or create by email address. Absent fields are not sent and keep their current values. */
export interface PersonInput {
    /** Email address that identifies the person; trimmed and lowercased. */
    email: string;
    /** First name. Any name part replaces the person's whole current name. */
    firstName?: string;
    /** Last name. */
    lastName?: string;
    /** Full name; defaults to the first and last name joined by a space. */
    fullName?: string;
    /** Job title; replaces the current one. */
    jobTitle?: string;
    /** Description; replaces the current one. */
    description?: string;
    /** Record id of the person's company; replaces the current company link. */
    companyRecordId?: string;
}

/** A company to find or create by domain. Absent fields are not sent and keep their current values. */
export interface CompanyInput {
    /** Domain or URL that identifies the company; normalized to a bare lowercase domain. */
    domain: string;
    /** Company name; replaces the current one. */
    name?: string;
    /** Description; replaces the current one. */
    description?: string;
}

/** A note to attach to a record. */
export interface NoteInput {
    /** Plain-text title. */
    title: string;
    /** Body text. */
    content: string;
    /** How `content` is interpreted: `plaintext` (default) or `markdown`. */
    format?: "plaintext" | "markdown";
}

/** One attribute value converted to Attio's JSON, with the input kind kept for type checks. */
export interface PreparedValue {
    /** The input kind, such as `text` or `record`. */
    kind: string;
    /** Attio's JSON for the value, such as `{ "value": "Acme" }`. */
    json: Record<string, unknown>;
}

/** Every value for one attribute, in input order. */
export interface PreparedAttribute {
    /** Normalized attribute slug. */
    slug: string;
    /** Converted values. */
    values: PreparedValue[];
}

/** One query condition after validation. */
export interface PreparedCondition {
    attribute: string;
    field: string;
    operator: QueryOperator;
    value: string | number | boolean;
    reference: RecordReference | null;
}

/** One sort key after validation. */
export interface PreparedSort {
    attribute: string;
    direction: string;
    field: string;
}

/** A page window after validation. */
export interface PageWindow {
    limit: number;
    offset: number;
}

/**
 * Normalize an object or attribute slug: trimmed and lowercased, then only
 * `a-z`, `0-9` and `_`. A UUID is refused, so a policy written on slugs sees
 * every request.
 * @param value The slug as given.
 * @param name Argument name for the error message.
 * @returns The normalized slug.
 */
export function normalizeSlug(value: string, name: string): string {
    const slug = value.trim().toLowerCase();
    if (slug.length === 0) throw validationError("invalid_input", name + " cannot be empty");
    for (let i = 0; i < slug.length; i += 1) {
        const code = slug.charCodeAt(i);
        const ok = (code >= 97 && code <= 122) || (code >= 48 && code <= 57) || code === 95;
        if (!ok) {
            throw validationError(
                "invalid_slug",
                name + " must be an Attio api_slug such as `deals` or `stage` (letters, digits and underscores), got " + JSON.stringify(value),
            );
        }
    }
    return slug;
}

/**
 * Normalize a record id: trimmed, lowercased, and required to be a UUID.
 * @param value The id as given.
 * @param name Argument name for the error message.
 * @returns The lowercase UUID.
 */
export function normalizeRecordId(value: string, name: string): string {
    const id = value.trim().toLowerCase();
    if (!isUuid(id)) {
        throw validationError("invalid_record_id", name + " must be an Attio record id (a UUID), got " + JSON.stringify(value));
    }
    return id;
}

/**
 * Whether a lowercase string is a UUID (8-4-4-4-12 hexadecimal digits).
 * @param value Candidate, already lowercased.
 * @returns True for a UUID.
 */
export function isUuid(value: string): boolean {
    if (value.length !== 36) return false;
    for (let i = 0; i < value.length; i += 1) {
        const code = value.charCodeAt(i);
        if (i === 8 || i === 13 || i === 18 || i === 23) {
            if (code !== 45) return false;
        } else if (!((code >= 48 && code <= 57) || (code >= 97 && code <= 102))) {
            return false;
        }
    }
    return true;
}

/**
 * Normalize an email address: trimmed and lowercased, with one `@`, a
 * non-empty local part, a dotted domain, and no whitespace.
 * @param value The address as given.
 * @returns The normalized address.
 */
export function normalizeEmail(value: string): string {
    const email = value.trim().toLowerCase();
    const at = email.indexOf("@");
    const invalid = validationError("invalid_email", "email must be an address such as dana@acme.com, got " + JSON.stringify(value));
    if (at <= 0 || at !== email.lastIndexOf("@")) throw invalid;
    const domain = email.slice(at + 1);
    if (domain.indexOf(".") <= 0 || domain.endsWith(".") || domain.indexOf("..") >= 0) throw invalid;
    for (let i = 0; i < email.length; i += 1) {
        const code = email.charCodeAt(i);
        if (code <= 32 || code === 127 || code === 60 || code === 62 || code === 44 || code === 59) throw invalid;
    }
    return email;
}

/**
 * Normalize a company domain: trimmed and lowercased, without scheme, user
 * info, `www.`, port, path, query, fragment or trailing dot.
 * `https://www.Acme.com/pricing` becomes `acme.com`.
 * @param value The domain or URL as given.
 * @returns The bare domain.
 */
export function normalizeDomain(value: string): string {
    let domain = value.trim().toLowerCase();
    const scheme = domain.indexOf("://");
    if (scheme >= 0) domain = domain.slice(scheme + 3);
    if (domain.startsWith("//")) domain = domain.slice(2);
    domain = cutAt(domain, "/");
    domain = cutAt(domain, "?");
    domain = cutAt(domain, "#");
    const at = domain.lastIndexOf("@");
    if (at >= 0) {
        throw validationError(
            "invalid_domain",
            "domain must be a domain such as acme.com, not an email address or URL with credentials, got " + JSON.stringify(value),
        );
    }
    domain = cutAt(domain, ":");
    while (domain.endsWith(".")) domain = domain.slice(0, domain.length - 1);
    if (domain.startsWith("www.")) domain = domain.slice(4);
    const invalid = validationError("invalid_domain", "domain must be a domain such as acme.com, got " + JSON.stringify(value));
    if (domain.length === 0 || domain.indexOf(".") <= 0) throw invalid;
    for (const label of domain.split(".")) {
        if (label.length === 0 || label.startsWith("-") || label.endsWith("-")) throw invalid;
        for (let i = 0; i < label.length; i += 1) {
            const code = label.charCodeAt(i);
            const ok = (code >= 97 && code <= 122) || (code >= 48 && code <= 57) || code === 45 || code > 127;
            if (!ok) throw invalid;
        }
    }
    return domain;
}

function cutAt(value: string, marker: string): string {
    const index = value.indexOf(marker);
    return index < 0 ? value : value.slice(0, index);
}

/**
 * Require non-blank text and return it trimmed.
 * @param value The text as given.
 * @param name Argument name for the error message.
 * @returns The trimmed text.
 */
export function requireText(value: string, name: string): string {
    const text = value.trim();
    if (text.length === 0) throw validationError("invalid_input", name + " cannot be empty");
    return text;
}

/**
 * Validate a page window.
 * @param limit Requested page size, or undefined for the default.
 * @param offset Requested offset, or undefined for 0.
 * @param fallback Default page size.
 * @param max Largest page size Attio accepts.
 * @returns The validated limit and offset.
 */
export function pageWindow(limit: number | undefined, offset: number | undefined, fallback: number, max: number): PageWindow {
    const size = limit ?? fallback;
    if (!(size >= 1 && size <= max) || size !== Math.floor(size)) {
        throw validationError("invalid_page_size", "limit must be an integer from 1 through " + max.toString() + ", got " + size.toString());
    }
    const skip = offset ?? 0;
    if (!(skip >= 0) || skip !== Math.floor(skip)) {
        throw validationError("invalid_offset", "offset must be a non-negative integer, got " + skip.toString());
    }
    return { limit: size, offset: skip };
}

/**
 * Validate a record query's page window: limit 1 through 500 (default 50), offset 0 or more.
 * @param limit Requested page size.
 * @param offset Requested offset.
 * @returns The validated window.
 */
export function queryWindow(limit: number | undefined, offset: number | undefined): PageWindow {
    return pageWindow(limit, offset, DEFAULT_QUERY_LIMIT, MAX_QUERY_LIMIT);
}

/**
 * Validate a note list's page window: limit 1 through 50 (default 50), offset 0 or more.
 * @param limit Requested page size.
 * @param offset Requested offset.
 * @returns The validated window.
 */
export function noteWindow(limit: number | undefined, offset: number | undefined): PageWindow {
    return pageWindow(limit, offset, DEFAULT_NOTE_LIMIT, MAX_NOTE_LIMIT);
}

/**
 * Convert one value to Attio's JSON, validating and normalizing it. The field
 * `type` names must be set and every other value field must be absent.
 * @param slug Attribute slug, for error messages.
 * @param fields The value's fields.
 * @returns The value kind and its Attio JSON.
 */
export function prepareValue(slug: string, fields: ValueFields): PreparedValue {
    const type = fields.type;
    const expected = valueFieldsOf(type);
    if (expected.length === 0) {
        throw validationError(
            "invalid_value",
            slug + ": type must be one of text, number, currency, checkbox, date, select, email, domain, record, member, got " + JSON.stringify(type),
        );
    }
    for (const name of presentFields(fields)) {
        if (!expected.includes(name)) {
            throw validationError("invalid_value", slug + ": a " + type + " value takes " + expected.join(" and ") + ", not " + name);
        }
    }
    const text = fields.text;
    const number = fields.number;
    const amount = fields.amount;
    const checked = fields.checked;
    const date = fields.date;
    const option = fields.option;
    const email = fields.email;
    const domain = fields.domain;
    const targetObject = fields.targetObject;
    const targetRecordId = fields.targetRecordId;
    if (type === "text" && text !== undefined) return { kind: "text", json: { value: text } };
    if (type === "number" && number !== undefined) {
        requireFinite(slug, number);
        return { kind: "number", json: { value: number } };
    }
    if (type === "currency" && amount !== undefined) {
        requireFinite(slug, amount);
        return { kind: "currency", json: { currency_value: amount } };
    }
    if (type === "checkbox" && checked !== undefined) return { kind: "checkbox", json: { value: checked } };
    if (type === "date" && date !== undefined) {
        const day = date.trim();
        if (!isIsoDate(day)) {
            throw validationError("invalid_value", slug + ": a date value must be YYYY-MM-DD, got " + JSON.stringify(date));
        }
        return { kind: "date", json: { value: day } };
    }
    if (type === "select" && option !== undefined) {
        return { kind: "select", json: { option: requireText(option, slug + " option") } };
    }
    if (type === "email" && email !== undefined) return { kind: "email", json: { email_address: normalizeEmail(email) } };
    if (type === "domain" && domain !== undefined) return { kind: "domain", json: { domain: normalizeDomain(domain) } };
    if (type === "record" && targetObject !== undefined && targetRecordId !== undefined) {
        return {
            kind: "record",
            json: {
                target_object: normalizeSlug(targetObject, slug + " targetObject"),
                target_record_id: normalizeRecordId(targetRecordId, slug + " targetRecordId"),
            },
        };
    }
    if (type === "member" && email !== undefined) {
        return { kind: "member", json: { workspace_member_email_address: normalizeEmail(email) } };
    }
    throw validationError("invalid_value", slug + ": a " + type + " value needs " + expected.join(" and "));
}

function valueFieldsOf(type: string): string[] {
    if (type === "text") return ["text"];
    if (type === "number") return ["number"];
    if (type === "currency") return ["amount"];
    if (type === "checkbox") return ["checked"];
    if (type === "date") return ["date"];
    if (type === "select") return ["option"];
    if (type === "email") return ["email"];
    if (type === "domain") return ["domain"];
    if (type === "record") return ["targetObject", "targetRecordId"];
    if (type === "member") return ["email"];
    return [];
}

function presentFields(fields: ValueFields): string[] {
    const present: string[] = [];
    if (fields.text !== undefined) present.push("text");
    if (fields.number !== undefined) present.push("number");
    if (fields.amount !== undefined) present.push("amount");
    if (fields.checked !== undefined) present.push("checked");
    if (fields.date !== undefined) present.push("date");
    if (fields.option !== undefined) present.push("option");
    if (fields.email !== undefined) present.push("email");
    if (fields.domain !== undefined) present.push("domain");
    if (fields.targetObject !== undefined) present.push("targetObject");
    if (fields.targetRecordId !== undefined) present.push("targetRecordId");
    return present;
}

function requireFinite(slug: string, value: number): void {
    if (!isFinite(value)) throw validationError("invalid_value", slug + ": a number value must be finite");
}

function isIsoDate(value: string): boolean {
    if (value.length !== 10 || value.charCodeAt(4) !== 45 || value.charCodeAt(7) !== 45) return false;
    for (let i = 0; i < value.length; i += 1) {
        if (i === 4 || i === 7) continue;
        const code = value.charCodeAt(i);
        if (code < 48 || code > 57) return false;
    }
    const month = Number(value.slice(5, 7));
    const day = Number(value.slice(8, 10));
    return month >= 1 && month <= 12 && day >= 1 && day <= 31;
}

/** One converted value with the attribute it is for. */
export interface PendingValue {
    /** Normalized attribute slug. */
    slug: string;
    /** The converted value. */
    value: PreparedValue;
}

/**
 * Attribute slugs, sorted and without duplicates.
 * @param slugs Normalized slugs in input order, one per value.
 * @returns A sorted copy holding each slug once.
 */
export function uniqueSlugs(slugs: string[]): string[] {
    const sorted: string[] = [];
    for (const slug of slugs) {
        if (!sorted.includes(slug)) sorted.push(slug);
    }
    sorted.sort();
    return sorted;
}

/**
 * Group converted values by attribute, sorted by slug, keeping each
 * attribute's values in input order.
 * @param pending Converted values in input order.
 * @returns One prepared attribute per slug.
 */
export function groupValues(pending: PendingValue[]): PreparedAttribute[] {
    const slugs: string[] = [];
    for (const item of pending) slugs.push(item.slug);
    const attributes: PreparedAttribute[] = [];
    for (const slug of uniqueSlugs(slugs)) {
        const values: PreparedValue[] = [];
        for (const item of pending) {
            if (item.slug === slug) values.push(item.value);
        }
        attributes.push({ slug: slug, values: values });
    }
    return attributes;
}

/**
 * Attio's `values` object for prepared attributes: each slug maps to an array of value objects.
 * @param attributes Prepared attributes.
 * @returns The `values` object.
 */
export function buildValues(attributes: PreparedAttribute[]): Record<string, unknown> {
    const values: Record<string, unknown> = {};
    for (const attribute of attributes) {
        const list: unknown[] = [];
        for (const value of attribute.values) list.push(value.json);
        values[attribute.slug] = list;
    }
    return values;
}

/**
 * Attio attribute types each value kind may be written to.
 * @param kind A prepared value's kind.
 * @returns The compatible Attio attribute types.
 */
export function compatibleTypes(kind: string): string[] {
    if (kind === "text") return ["text"];
    if (kind === "number") return ["number", "rating"];
    if (kind === "currency") return ["currency"];
    if (kind === "checkbox") return ["checkbox"];
    if (kind === "date") return ["date", "timestamp"];
    if (kind === "select") return ["select"];
    if (kind === "email") return ["email-address"];
    if (kind === "domain") return ["domain"];
    if (kind === "record") return ["record-reference"];
    if (kind === "member") return ["actor-reference"];
    return [];
}

/**
 * Check prepared values against the object's live attribute definitions:
 * every slug must be an active, writable attribute, status attributes are
 * refused (a status changes only through `setStatus` or `input.status`), a
 * single-value attribute takes one value, and each value kind must suit the
 * attribute type.
 * @param object Object slug, for error messages.
 * @param attributes Prepared attributes.
 * @param definitions The object's non-archived attributes.
 * @param statusHint What to use instead of writing a status attribute.
 */
export function requirePlainAttributes(object: string, attributes: PreparedAttribute[], definitions: Attribute[], statusHint: string): void {
    for (const attribute of attributes) {
        const slug = attribute.slug;
        const definition = findDefinition(definitions, slug);
        if (definition === null) {
            throw validationError("unknown_attribute", object + " has no active attribute " + JSON.stringify(slug) + "; listAttributes shows the valid slugs");
        }
        if (definition.type === "status") {
            throw validationError("status_attribute", slug + " is a status attribute of " + object + "; " + statusHint);
        }
        if (!definition.isWritable) {
            throw validationError("read_only_attribute", slug + " on " + object + " cannot be written through the API");
        }
        if (!definition.isMultiselect && attribute.values.length > 1) {
            throw validationError(
                "invalid_value",
                slug + " on " + object + " holds one value, got " + attribute.values.length.toString(),
            );
        }
        for (const value of attribute.values) {
            if (!compatibleTypes(value.kind).includes(definition.type)) {
                throw validationError(
                    "invalid_value",
                    slug + " on " + object + " is a " + definition.type + " attribute; a " + value.kind + " value cannot be written to it",
                );
            }
        }
    }
}

/**
 * Find an attribute definition by slug.
 * @param definitions Attribute definitions.
 * @param slug Attribute slug.
 * @returns The definition, or null when absent.
 */
export function findDefinition(definitions: Attribute[], slug: string): Attribute | null {
    for (const definition of definitions) {
        if (definition.slug === slug) return definition;
    }
    return null;
}

/**
 * Validate one query condition and copy it.
 * @param condition The caller's condition.
 * @returns The validated condition.
 */
export function prepareCondition(condition: QueryCondition): PreparedCondition {
    const { attribute, operator, value, field } = condition;
    const slug = normalizeSlug(attribute, "filter attribute");
    const property = field === undefined ? "" : normalizeSlug(field, "filter field");
    if (!isOperator(operator)) {
        throw validationError(
            "invalid_filter",
            "filter operator must be one of eq, contains, starts_with, ends_with, lt, lte, gt, gte, got " + JSON.stringify(operator),
        );
    }
    if (typeof value === "string" || typeof value === "number" || typeof value === "boolean") {
        if (typeof value === "number" && !isFinite(value)) {
            throw validationError("invalid_filter", "filter on " + slug + ": a number must be finite");
        }
        if ((operator === "contains" || operator === "starts_with" || operator === "ends_with") && typeof value !== "string") {
            throw validationError("invalid_filter", "filter on " + slug + ": " + operator + " needs a string value");
        }
        if ((operator === "lt" || operator === "lte" || operator === "gt" || operator === "gte") && typeof value === "boolean") {
            throw validationError("invalid_filter", "filter on " + slug + ": " + operator + " needs a number or a date string");
        }
        return { attribute: slug, field: property, operator: operator, value: value, reference: null };
    }
    if (operator !== "eq") {
        throw validationError("invalid_filter", "filter on " + slug + ": a record reference can only be compared with eq");
    }
    if (property.length > 0) {
        throw validationError("invalid_filter", "filter on " + slug + ": a record reference takes no field");
    }
    const { targetObject, targetRecordId } = value;
    const reference: RecordReference = {
        targetObject: normalizeSlug(targetObject, "filter targetObject"),
        targetRecordId: normalizeRecordId(targetRecordId, "filter targetRecordId"),
    };
    return { attribute: slug, field: "", operator: "eq", value: "", reference: reference };
}

function isOperator(value: string): boolean {
    return value === "eq" || value === "contains" || value === "starts_with" || value === "ends_with" ||
        value === "lt" || value === "lte" || value === "gt" || value === "gte";
}

/**
 * Validate one sort key and copy it.
 * @param sort The caller's sort key.
 * @returns The validated sort key.
 */
export function prepareSort(sort: QuerySort): PreparedSort {
    const { attribute, direction, field } = sort;
    const slug = normalizeSlug(attribute, "sort attribute");
    if (direction !== "asc" && direction !== "desc") {
        throw validationError("invalid_sort", "sort direction must be asc or desc, got " + JSON.stringify(direction));
    }
    return { attribute: slug, direction: direction, field: field === undefined ? "" : normalizeSlug(field, "sort field") };
}

/**
 * Attio's JSON for one condition: shorthand equality, an operator, or an operator on a property.
 * @param condition A validated condition.
 * @returns The filter object for this condition.
 */
export function buildCondition(condition: PreparedCondition): Record<string, unknown> {
    const filter: Record<string, unknown> = {};
    if (condition.reference !== null) {
        filter[condition.attribute] = {
            target_object: condition.reference.targetObject,
            target_record_id: condition.reference.targetRecordId,
        };
        return filter;
    }
    if (condition.operator === "eq" && condition.field.length === 0) {
        filter[condition.attribute] = condition.value;
        return filter;
    }
    const comparison: Record<string, unknown> = {};
    comparison["$" + condition.operator] = condition.value;
    if (condition.field.length === 0) {
        filter[condition.attribute] = comparison;
        return filter;
    }
    const property: Record<string, unknown> = {};
    property[condition.field] = comparison;
    filter[condition.attribute] = property;
    return filter;
}

/**
 * The body of `POST /v2/objects/{object}/records/query`.
 * @param conditions Validated conditions, combined with `$and`.
 * @param sorts Validated sort keys.
 * @param window Validated page window.
 * @returns The request body; `filter` and `sorts` are omitted when empty.
 */
export function buildQueryBody(conditions: PreparedCondition[], sorts: PreparedSort[], window: PageWindow): Record<string, unknown> {
    const body: Record<string, unknown> = { limit: window.limit, offset: window.offset };
    if (conditions.length === 1) {
        body["filter"] = buildCondition(conditions[0]);
    } else if (conditions.length > 1) {
        const all: unknown[] = [];
        for (const condition of conditions) all.push(buildCondition(condition));
        body["filter"] = { "$and": all };
    }
    if (sorts.length > 0) {
        const list: unknown[] = [];
        for (const sort of sorts) {
            const entry: Record<string, unknown> = { direction: sort.direction, attribute: sort.attribute };
            if (sort.field.length > 0) entry["field"] = sort.field;
            list.push(entry);
        }
        body["sorts"] = list;
    }
    return body;
}

/**
 * The `values` for a person upsert matched on `email_addresses`: the email and
 * only the fields given.
 * @param email Normalized email.
 * @param firstName First name, if given.
 * @param lastName Last name, if given.
 * @param fullName Full name, if given.
 * @param jobTitle Job title, if given.
 * @param description Description, if given.
 * @param companyRecordId Normalized company record id, if given.
 * @returns The `values` object.
 */
export function buildPersonValues(
    email: string,
    firstName: string | undefined,
    lastName: string | undefined,
    fullName: string | undefined,
    jobTitle: string | undefined,
    description: string | undefined,
    companyRecordId: string | undefined,
): Record<string, unknown> {
    const values: Record<string, unknown> = { email_addresses: [{ email_address: email }] };
    if (firstName !== undefined || lastName !== undefined || fullName !== undefined) {
        const first = (firstName ?? "").trim();
        const last = (lastName ?? "").trim();
        const joined = (first + " " + last).trim();
        const full = fullName === undefined ? joined : fullName.trim();
        if (full.length === 0) throw validationError("invalid_input", "a person's name cannot be empty");
        values["name"] = [{ first_name: first, last_name: last, full_name: full }];
    }
    if (jobTitle !== undefined) values["job_title"] = [{ value: jobTitle }];
    if (description !== undefined) values["description"] = [{ value: description }];
    if (companyRecordId !== undefined) {
        values["company"] = [{ target_object: "companies", target_record_id: companyRecordId }];
    }
    return values;
}

/**
 * The `values` for a company upsert matched on `domains`: the domain and only the fields given.
 * @param domain Normalized domain.
 * @param name Company name, if given.
 * @param description Description, if given.
 * @returns The `values` object.
 */
export function buildCompanyValues(domain: string, name: string | undefined, description: string | undefined): Record<string, unknown> {
    const values: Record<string, unknown> = { domains: [{ domain: domain }] };
    if (name !== undefined) values["name"] = [{ value: requireText(name, "name") }];
    if (description !== undefined) values["description"] = [{ value: description }];
    return values;
}

/**
 * The query string of `GET /v2/notes` for one parent record.
 * @param parentObject Normalized parent object slug.
 * @param parentRecordId Normalized parent record id.
 * @param window Validated page window.
 * @returns Query string beginning with `?`.
 */
export function buildNotesQuery(parentObject: string, parentRecordId: string, window: PageWindow): string {
    return "?parent_object=" + parentObject + "&parent_record_id=" + parentRecordId +
        "&limit=" + window.limit.toString() + "&offset=" + window.offset.toString();
}
