// Response side of the Attio client: the curated result types, the raw API
// shapes they are decoded from, status resolution, and HTTP error codes and
// messages. Internal: `src/lib.ts` re-exports the result types, and programs
// reach the helpers only through its operations.

import { validationError } from "./errors";

/** An Attio object, such as `people`, `companies`, `deals` or a custom object. */
export interface AttioObject {
    /** Object id (UUID). */
    objectId: string;
    /** The api_slug used by every operation, such as `deals`. */
    slug: string;
    /** Singular display name, such as "Deal". */
    singularNoun: string;
    /** Plural display name, such as "Deals". */
    pluralNoun: string;
    /** ISO creation timestamp. */
    createdAt: string;
}

/** An attribute of an object. */
export interface Attribute {
    /** Attribute id (UUID). */
    attributeId: string;
    /** The api_slug used as a key in record values, such as `stage`. */
    slug: string;
    /** Display title. */
    title: string;
    /** Description, or "" when none. */
    description: string;
    /** Attio attribute type: `text`, `number`, `checkbox`, `currency`, `date`, `timestamp`, `rating`, `status`, `select`, `record-reference`, `actor-reference`, `location`, `domain`, `email-address`, `phone-number`, `interaction` or `personal-name`. */
    type: string;
    /** Whether the attribute holds several values. */
    isMultiselect: boolean;
    /** Whether values can be written through the API. */
    isWritable: boolean;
    /** Whether a value is required when creating a record. */
    isRequired: boolean;
    /** Whether values must be unique across records. */
    isUnique: boolean;
    /** Currency code of a currency attribute, such as "USD", or "". */
    currencyCode: string;
}

/** A status of a status attribute, such as a deal stage. */
export interface Status {
    /** Status id (UUID). */
    statusId: string;
    /** Display title, such as "In Progress". */
    title: string;
    /** Whether Attio celebrates reaching this status, typically a won stage. */
    celebrationEnabled: boolean;
}

/** A member of the Attio workspace. */
export interface Member {
    /** Workspace member id (UUID). */
    memberId: string;
    /** Email address, lowercased. */
    email: string;
    /** First name. */
    firstName: string;
    /** Last name. */
    lastName: string;
    /** `admin`, `member` or `suspended`. */
    accessLevel: string;
}

/** One current value of an attribute, in readable form. */
export interface RecordValue {
    /** Attio attribute type of the value, such as `text`, `status` or `record-reference`. */
    type: string;
    /**
     * The value as text: the text itself, a status or select title, an email
     * address, a domain, a person's full name, a number, a currency amount such
     * as "1200 USD", "true"/"false", a date or timestamp, a phone number, an
     * address, the referenced record's id, or a workspace member's id (match it
     * with `listMembers`).
     */
    value: string;
    /** The number, for number, rating and currency values. */
    number?: number;
    /** Currency code, for currency values. */
    currencyCode?: string;
    /** Status id or select option id. */
    optionId?: string;
    /** Object slug of the referenced record, for record references. */
    targetObject?: string;
    /** Id of the referenced record, for record references. */
    targetRecordId?: string;
    /** `workspace-member`, `api-token` or `system`, for actor references such as `owner`. */
    actorType?: string;
    /** Actor id, for actor references. */
    actorId?: string;
}

/** A record with its current values. */
export interface AttioRecord {
    /** Object slug, such as `deals`. */
    object: string;
    /** Record id (UUID). */
    recordId: string;
    /** Link to the record in the Attio web app. */
    webUrl: string;
    /** ISO creation timestamp. */
    createdAt: string;
    /** Current values keyed by attribute slug. Attributes without a value are left out. */
    values: Record<string, RecordValue[]>;
}

/** The outcome of `setStatus`. */
export interface StatusChange {
    /** The record after the change. */
    record: AttioRecord;
    /** The status title now set. */
    status: string;
    /** The title before the change, or "" when the record had none. */
    previousStatus: string;
    /** False when the record already had this status, so nothing was written. */
    changed: boolean;
}

/** A note attached to a record. */
export interface Note {
    /** Note id (UUID). */
    noteId: string;
    /** Parent object slug. */
    parentObject: string;
    /** Parent record id. */
    parentRecordId: string;
    /** Title. */
    title: string;
    /** Body as plain text. */
    text: string;
    /** Body as Markdown. */
    markdown: string;
    /** ISO creation timestamp. */
    createdAt: string;
}

/** One page of a limit/offset list. */
export interface Page<T> {
    /** Items on this page. */
    items: T[];
    /** Offset of the next page, or null when this page is the last. */
    nextOffset: number | null;
    /** True when there are no more items after this page. */
    isComplete: boolean;
}

export interface ApiObjectId {
    object_id?: string | null;
}

export interface ApiObject {
    id: ApiObjectId;
    api_slug?: string | null;
    singular_noun?: string | null;
    plural_noun?: string | null;
    created_at?: string | null;
}

export interface ApiAttributeId {
    attribute_id?: string | null;
}

export interface ApiCurrencyConfig {
    default_currency_code?: string | null;
}

export interface ApiAttributeConfig {
    currency?: ApiCurrencyConfig | null;
}

export interface ApiAttribute {
    id: ApiAttributeId;
    api_slug: string;
    title?: string | null;
    description?: string | null;
    type: string;
    is_multiselect?: boolean | null;
    is_writable?: boolean | null;
    is_required?: boolean | null;
    is_unique?: boolean | null;
    is_archived?: boolean | null;
    config?: ApiAttributeConfig | null;
}

export interface ApiStatusId {
    status_id: string;
}

export interface ApiStatus {
    id: ApiStatusId;
    title: string;
    is_archived?: boolean | null;
    celebration_enabled?: boolean | null;
}

export interface ApiMemberId {
    workspace_member_id: string;
}

export interface ApiMember {
    id: ApiMemberId;
    first_name?: string | null;
    last_name?: string | null;
    email_address?: string | null;
    access_level?: string | null;
}

export interface ApiOptionId {
    option_id?: string | null;
}

export interface ApiOption {
    id?: ApiOptionId | null;
    title?: string | null;
}

export interface ApiStatusRef {
    id?: ApiStatusId | null;
    title?: string | null;
}

/** Every property an Attio attribute value may carry; each type uses a few. */
export interface ApiValue {
    attribute_type?: string | null;
    value?: unknown;
    currency_value?: number | null;
    currency_code?: string | null;
    option?: ApiOption | null;
    status?: ApiStatusRef | null;
    email_address?: string | null;
    domain?: string | null;
    full_name?: string | null;
    first_name?: string | null;
    last_name?: string | null;
    target_object?: string | null;
    target_record_id?: string | null;
    referenced_actor_type?: string | null;
    referenced_actor_id?: string | null;
    normalized_phone_number?: string | null;
    original_phone_number?: string | null;
    line_1?: string | null;
    line_2?: string | null;
    locality?: string | null;
    region?: string | null;
    postcode?: string | null;
    country_code?: string | null;
    interaction_type?: string | null;
    interacted_at?: string | null;
}

export interface ApiRecordId {
    record_id: string;
}

export interface ApiRecord {
    id: ApiRecordId;
    created_at?: string | null;
    web_url?: string | null;
    values?: Record<string, ApiValue[]> | null;
}

export interface ApiNoteId {
    note_id: string;
}

export interface ApiNote {
    id: ApiNoteId;
    parent_object?: string | null;
    parent_record_id?: string | null;
    title?: string | null;
    content_plaintext?: string | null;
    content_markdown?: string | null;
    created_at?: string | null;
}

export interface ApiErrorBody {
    status_code?: number | null;
    type?: string | null;
    code?: string | null;
    message?: string | null;
}

export interface ObjectList {
    data: ApiObject[];
}

export interface AttributeList {
    data: ApiAttribute[];
}

export interface StatusList {
    data: ApiStatus[];
}

export interface MemberList {
    data: ApiMember[];
}

export interface RecordEnvelope {
    data: ApiRecord;
}

export interface RecordList {
    data: ApiRecord[];
}

export interface NoteEnvelope {
    data: ApiNote;
}

export interface NoteList {
    data: ApiNote[];
}

/**
 * Map an object definition.
 * @param data Raw object.
 * @returns The curated object.
 */
export function objectFrom(data: ApiObject): AttioObject {
    return {
        objectId: data.id.object_id ?? "",
        slug: data.api_slug ?? "",
        singularNoun: data.singular_noun ?? "",
        pluralNoun: data.plural_noun ?? "",
        createdAt: data.created_at ?? "",
    };
}

/**
 * Map an attribute definition.
 * @param data Raw attribute.
 * @returns The curated attribute.
 */
export function attributeFrom(data: ApiAttribute): Attribute {
    return {
        attributeId: data.id.attribute_id ?? "",
        slug: data.api_slug,
        title: data.title ?? "",
        description: data.description ?? "",
        type: data.type,
        isMultiselect: data.is_multiselect === true,
        isWritable: data.is_writable !== false,
        isRequired: data.is_required === true,
        isUnique: data.is_unique === true,
        currencyCode: data.config?.currency?.default_currency_code ?? "",
    };
}

/**
 * Map a status.
 * @param data Raw status.
 * @returns The curated status.
 */
export function statusFrom(data: ApiStatus): Status {
    return { statusId: data.id.status_id, title: data.title, celebrationEnabled: data.celebration_enabled === true };
}

/**
 * Map a workspace member; the email is lowercased.
 * @param data Raw member.
 * @returns The curated member.
 */
export function memberFrom(data: ApiMember): Member {
    return {
        memberId: data.id.workspace_member_id,
        email: (data.email_address ?? "").toLowerCase(),
        firstName: data.first_name ?? "",
        lastName: data.last_name ?? "",
        accessLevel: data.access_level ?? "",
    };
}

/**
 * Map a note.
 * @param data Raw note.
 * @returns The curated note.
 */
export function noteFrom(data: ApiNote): Note {
    return {
        noteId: data.id.note_id,
        parentObject: data.parent_object ?? "",
        parentRecordId: data.parent_record_id ?? "",
        title: data.title ?? "",
        text: data.content_plaintext ?? "",
        markdown: data.content_markdown ?? "",
        createdAt: data.created_at ?? "",
    };
}

/**
 * Map a record to its readable form.
 * @param data Raw record.
 * @param object Object slug the record belongs to.
 * @returns The curated record.
 */
export function recordFrom(data: ApiRecord, object: string): AttioRecord {
    const values: Record<string, RecordValue[]> = {};
    const raw = data.values;
    if (raw !== null && raw !== undefined) {
        for (const slug of Object.keys(raw)) {
            const list: RecordValue[] = [];
            for (const value of raw[slug] ?? []) list.push(valueFrom(value));
            if (list.length > 0) values[slug] = list;
        }
    }
    return {
        object: object,
        recordId: data.id.record_id,
        webUrl: data.web_url ?? "",
        createdAt: data.created_at ?? "",
        values: values,
    };
}

/**
 * Map one attribute value to its readable form.
 * @param data Raw value.
 * @returns The readable value.
 */
export function valueFrom(data: ApiValue): RecordValue {
    const type = data.attribute_type ?? "";
    const status = data.status;
    if (status !== null && status !== undefined) {
        return { type: "status", value: status.title ?? "", optionId: status.id?.status_id ?? "" };
    }
    const option = data.option;
    if (option !== null && option !== undefined) {
        return { type: "select", value: option.title ?? "", optionId: option.id?.option_id ?? "" };
    }
    const targetRecordId = data.target_record_id;
    if (targetRecordId !== null && targetRecordId !== undefined) {
        return {
            type: "record-reference",
            value: targetRecordId,
            targetObject: data.target_object ?? "",
            targetRecordId: targetRecordId,
        };
    }
    const actorType = data.referenced_actor_type;
    if (actorType !== null && actorType !== undefined) {
        const actorId = data.referenced_actor_id ?? "";
        return {
            type: "actor-reference",
            value: actorId.length > 0 ? actorId : actorType,
            actorType: actorType,
            actorId: actorId,
        };
    }
    const currencyValue = data.currency_value;
    if (type === "currency" || (currencyValue !== null && currencyValue !== undefined)) {
        const amount = currencyValue ?? 0;
        const code = data.currency_code ?? "";
        return {
            type: "currency",
            value: code.length > 0 ? amount.toString() + " " + code : amount.toString(),
            number: amount,
            currencyCode: code,
        };
    }
    const email = data.email_address;
    if (email !== null && email !== undefined) return { type: "email-address", value: email };
    const domain = data.domain;
    if (domain !== null && domain !== undefined) return { type: "domain", value: domain };
    const fullName = data.full_name;
    if (fullName !== null && fullName !== undefined) return { type: "personal-name", value: fullName };
    const phone = data.normalized_phone_number ?? data.original_phone_number;
    if (phone !== null && phone !== undefined) return { type: "phone-number", value: phone };
    const interactedAt = data.interacted_at;
    if (interactedAt !== null && interactedAt !== undefined) {
        const kind = data.interaction_type ?? "";
        return { type: "interaction", value: kind.length > 0 ? kind + " " + interactedAt : interactedAt };
    }
    if (type === "location") return { type: "location", value: locationText(data) };
    const scalar = data.value;
    if (typeof scalar === "number") return { type: type, value: scalar.toString(), number: scalar };
    if (typeof scalar === "boolean") return { type: type, value: scalar ? "true" : "false" };
    if (typeof scalar === "string") return { type: type, value: scalar };
    return { type: type, value: "" };
}

function locationText(data: ApiValue): string {
    const parts: string[] = [];
    for (const part of [data.line_1, data.line_2, data.locality, data.region, data.postcode, data.country_code]) {
        if (part !== null && part !== undefined && part.length > 0) parts.push(part);
    }
    return parts.join(", ");
}

/**
 * The title of a record's current status for one attribute.
 * @param data Raw record.
 * @param attribute Status attribute slug.
 * @returns The current status id and title, or empty strings when the record has none.
 */
export function currentStatus(data: ApiRecord, attribute: string): Status {
    const values = data.values;
    if (values !== null && values !== undefined) {
        for (const value of values[attribute] ?? []) {
            const status = value.status;
            if (status !== null && status !== undefined) {
                return { statusId: status.id?.status_id ?? "", title: status.title ?? "", celebrationEnabled: false };
            }
        }
    }
    return { statusId: "", title: "", celebrationEnabled: false };
}

/**
 * Resolve a status by id or by title (case-insensitive) among an attribute's
 * live statuses.
 * @param statuses The attribute's non-archived statuses.
 * @param requested A status id or title.
 * @param object Object slug, for the error message.
 * @param attribute Attribute slug, for the error message.
 * @returns The matching status, with Attio's canonical title.
 */
export function findStatus(statuses: Status[], requested: string, object: string, attribute: string): Status {
    const wanted = requested.trim().toLowerCase();
    for (const status of statuses) {
        if (status.statusId.toLowerCase() === wanted) return status;
    }
    for (const status of statuses) {
        if (status.title.trim().toLowerCase() === wanted) return status;
    }
    const titles: string[] = [];
    for (const status of statuses) titles.push(JSON.stringify(status.title));
    throw validationError(
        "unknown_status",
        "no status " + JSON.stringify(requested) + " on " + object + "." + attribute + "; valid titles: " +
            (titles.length > 0 ? titles.join(", ") : "(none)"),
    );
}

/**
 * Map an HTTP status to a stable code, used when Attio's body has no `code`.
 * @param status HTTP status of the failed response.
 * @returns `bad_request`, `unauthorized`, `forbidden`, `not_found`, `conflict`, `rate_limit_exceeded`, `server_error` or `http_error`.
 */
export function attioErrorCode(status: number): string {
    if (status === 400) return "bad_request";
    if (status === 401) return "unauthorized";
    if (status === 403) return "forbidden";
    if (status === 404) return "not_found";
    if (status === 409) return "conflict";
    if (status === 429) return "rate_limit_exceeded";
    if (status >= 500) return "server_error";
    return "http_error";
}

/**
 * Parse Attio's error body, `{ status_code, type, code, message }`.
 * @param body Raw response body.
 * @returns The parsed fields, empty when the body is not a JSON object.
 */
export function parseErrorBody(body: string): ApiErrorBody {
    if (!body.trim().startsWith("{")) return {};
    try {
        return JSON.parse(body) as ApiErrorBody;
    } catch (_cause) {
        return {};
    }
}

/**
 * The fields of an AttioError for a failed response.
 * @param status HTTP status.
 * @param statusText HTTP reason phrase.
 * @param body Raw response body.
 * @returns Code, message, type and whether the request may be retried.
 */
export function failureFrom(status: number, statusText: string, body: string): FailureDetails {
    const parsed = parseErrorBody(body);
    const code = parsed.code !== null && parsed.code !== undefined && parsed.code.length > 0 ? parsed.code : attioErrorCode(status);
    const detail = parsed.message ?? "";
    const message = detail.length > 0
        ? "Attio " + status.toString() + " " + code + ": " + detail
        : "Attio request failed: HTTP " + status.toString() + " " + statusText;
    const retryable = status === 429 || code === "concurrent_write_conflict";
    return { code: code, message: message, type: parsed.type ?? "", retryable: retryable };
}

/** The parts of an AttioError derived from a failed response. */
export interface FailureDetails {
    code: string;
    message: string;
    type: string;
    retryable: boolean;
}

/**
 * The next-page fields of a limit/offset page.
 * @param offset Offset of this page.
 * @param limit Requested page size.
 * @param received Number of items Attio returned.
 * @returns `nextOffset` and `isComplete`; a short page is the last one.
 */
export function pageAfter(offset: number, limit: number, received: number): PageEnd {
    const complete = received < limit;
    return { nextOffset: complete ? null : offset + received, isComplete: complete };
}

/** The next-page fields of a page. */
export interface PageEnd {
    nextOffset: number | null;
    isComplete: boolean;
}
