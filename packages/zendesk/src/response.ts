// Response side of the Zendesk client: the typed results, their mapping from
// Zendesk's JSON, and the decoding of Zendesk's error answers. Internal:
// `src/lib.ts` re-exports the result types.

import { CustomValue } from "./request";

/** One page of results. */
export interface Page<T> {
    /** The items on this page. */
    items: T[];
    /** Pass as `cursor`, with the same other arguments, for the next page; null on the last page. */
    nextCursor: string | null;
}

/** One page of search results. */
export interface SearchPage<T> extends Page<T> {
    /** How many results match in all; search reads at most 1,000 of them. */
    count: number;
}

/** A custom field's value on a ticket, user or organization. */
export interface CustomField {
    /** The field's id. */
    id: string;
    /** Its value; null when empty. */
    value: CustomValue;
}

/** A ticket. Ids are strings; "none" fields are null. */
export interface Ticket {
    /** Its id. */
    id: string;
    /** The subject line. */
    subject: string;
    /** The first comment's text. */
    description: string;
    /** `new`, `open`, `pending`, `hold`, `solved` or `closed`. */
    status: string;
    /** `low`, `normal`, `high`, `urgent`, or null. */
    priority: string | null;
    /** `question`, `incident`, `problem`, `task`, or null. */
    type: string | null;
    /** Tags, lowercase. */
    tags: string[];
    /** The user who asked for help. */
    requesterId: string | null;
    /** The user who created it. */
    submitterId: string | null;
    /** The agent it is assigned to. */
    assigneeId: string | null;
    /** The group it is in. */
    groupId: string | null;
    /** The organization it belongs to. */
    organizationId: string | null;
    /** The brand it came in through. */
    brandId: string | null;
    /** For an incident, the problem ticket it is linked to. */
    problemId: string | null;
    /** Agents following it. */
    followerIds: string[];
    /** Users copied on its emails. */
    emailCcIds: string[];
    /** How the ticket was created, such as `email`, `web`, `api` or `chat`. */
    channel: string;
    /** Custom field values, by field id. */
    customFields: CustomField[];
    /** When it was created, ISO 8601. */
    createdAt: string;
    /** When it last changed, ISO 8601. */
    updatedAt: string;
    /** A task's due date-time, or null. */
    dueAt: string | null;
}

/** An attachment on a comment. */
export interface Attachment {
    /** Its id. */
    id: string;
    /** The file's name. */
    fileName: string;
    /** The file's MIME type. */
    contentType: string;
    /** Bytes. */
    size: number;
    /** Where Zendesk serves it; reading it needs the account's attachment host. */
    contentUrl: string;
}

/** A comment on a ticket: a public reply or an internal note. */
export interface Comment {
    /** Its id. */
    id: string;
    /** The user who wrote it. */
    authorId: string | null;
    /** The text, without formatting. */
    body: string;
    /** The HTML Zendesk renders. */
    htmlBody: string;
    /** True for a reply the requester sees; false for an internal note. */
    public: boolean;
    /** Files attached to it. */
    attachments: Attachment[];
    /** How it was added, such as `email`, `web` or `api`. */
    channel: string;
    /** When it was created, ISO 8601. */
    createdAt: string;
}

/** A user: an end user (a customer), an agent or an admin. */
export interface User {
    /** Its id. */
    id: string;
    /** Its name. */
    name: string;
    /** Primary email address. */
    email: string | null;
    /** Phone number. */
    phone: string | null;
    /** `end-user`, `agent` or `admin`. */
    role: string;
    /** The organization it belongs to. */
    organizationId: string | null;
    /** Tags, lowercase. */
    tags: string[];
    /** Notes about it, which only agents see. */
    notes: string;
    /** Details, such as an address, which only agents see. */
    details: string;
    /** IANA time zone, such as `Europe/Berlin`. */
    timeZone: string | null;
    /** Locale, such as `en-us`. */
    locale: string | null;
    /** True when the user may not sign in or submit tickets. */
    suspended: boolean;
    /** Custom user fields, by key. */
    userFields: Record<string, CustomValue>;
    /** When it was created, ISO 8601. */
    createdAt: string;
    /** When it last changed, ISO 8601. */
    updatedAt: string;
}

/** An organization, such as a customer company. */
export interface Organization {
    /** Its id. */
    id: string;
    /** Its name. */
    name: string;
    /** Email domains whose users join it automatically. */
    domainNames: string[];
    /** The group it is in. */
    groupId: string | null;
    /** Tags, lowercase. */
    tags: string[];
    /** Notes about it, which only agents see. */
    notes: string;
    /** Details, such as an address, which only agents see. */
    details: string;
    /** Custom organization fields, by key. */
    organizationFields: Record<string, CustomValue>;
    /** When it was created, ISO 8601. */
    createdAt: string;
    /** When it last changed, ISO 8601. */
    updatedAt: string;
}

/** A group of agents, such as Tier 1 or Billing. */
export interface Group {
    /** Its id. */
    id: string;
    /** Its name. */
    name: string;
    /** Its description. */
    description: string;
    /** True for the account's default. */
    default: boolean;
}

/** A view: a saved ticket list, such as "Unassigned tickets". */
export interface View {
    /** Its id. */
    id: string;
    /** Its title. */
    title: string;
    /** True while it is in use. */
    active: boolean;
    /** Its place in the list. */
    position: number;
    /** When it last changed, ISO 8601. */
    updatedAt: string;
}

/** A macro: a saved set of ticket changes and a comment. */
export interface Macro {
    /** Its id. */
    id: string;
    /** Its title. */
    title: string;
    /** Its description. */
    description: string;
    /** True while it is in use. */
    active: boolean;
    /** What it does, as Zendesk lists it: a field and a value. */
    actions: MacroAction[];
}

/** One change a macro makes. */
export interface MacroAction {
    /** The ticket field, such as `status`, `group_id`, `comment_value` or `custom_fields_360001234`. */
    field: string;
    /** The value, as Zendesk stores it. */
    value: CustomValue;
}

/** What applying a macro to a ticket would change, unsaved. */
export interface MacroPreview {
    /** The ticket's fields the macro changes, by Zendesk's field name, such as `status` or `group_id`. */
    changes: Record<string, unknown>;
    /** The comment it adds, or null. */
    comment: MacroComment | null;
}

/** The comment a macro adds. */
export interface MacroComment {
    /** The text. */
    body: string;
    /** The HTML. */
    htmlBody: string;
    /** True when the requester sees it. */
    public: boolean;
}

/** A ticket field, standard or custom. */
export interface TicketField {
    /** Its id. */
    id: string;
    /** `subject`, `status`, `tagger` (drop-down), `multiselect`, `checkbox`, `date`, `integer`, `decimal`, `text`, …. */
    type: string;
    /** Its title. */
    title: string;
    /** Its description. */
    description: string;
    /** True while it is in use. */
    active: boolean;
    /** True when agents must fill it before solving. */
    required: boolean;
    /** A drop-down's or multi-select's options: the label shown and the tag stored. */
    options: FieldOption[];
}

/** One option of a drop-down or multi-select field. */
export interface FieldOption {
    /** Its name. */
    name: string;
    /** Its value. */
    value: string;
}

/** A brand: a help desk the account runs for one product or company. */
export interface Brand {
    /** Its id. */
    id: string;
    /** Its name. */
    name: string;
    /** Its Zendesk subdomain. */
    subdomain: string;
    /** True for the account's default. */
    default: boolean;
    /** True while it is in use. */
    active: boolean;
}

/** A Help Center article. */
export interface Article {
    /** Its id. */
    id: string;
    /** Its title. */
    title: string;
    /** The article's HTML. */
    body: string;
    /** Locale, such as `en-us`. */
    locale: string;
    /** The section it is in. */
    sectionId: string | null;
    /** The user who wrote it. */
    authorId: string | null;
    /** True for an unpublished draft. */
    draft: boolean;
    /** Its public page. */
    htmlUrl: string;
    /** Its labels. */
    labelNames: string[];
    /** When it last changed, ISO 8601. */
    updatedAt: string;
}

/** A search hit: an article, with Zendesk's highlighted excerpt. */
export interface ArticleHit extends Article {
    /** Matching text, with `<em>` around the matches. */
    snippet: string;
}

/** A Help Center section. */
export interface Section {
    /** Its id. */
    id: string;
    /** Its name. */
    name: string;
    /** Its description. */
    description: string;
    /** The category it is in. */
    categoryId: string | null;
    /** Locale, such as `en-us`. */
    locale: string;
    /** Its public page. */
    htmlUrl: string;
}

/** A Help Center category. */
export interface Category {
    /** Its id. */
    id: string;
    /** Its name. */
    name: string;
    /** Its description. */
    description: string;
    /** Locale, such as `en-us`. */
    locale: string;
    /** Its public page. */
    htmlUrl: string;
}

/** A customer's satisfaction rating of a ticket. */
export interface SatisfactionRating {
    /** Its id. */
    id: string;
    /** `good`, `bad`, `offered` or `unoffered`. */
    score: string;
    /** What the customer wrote. */
    comment: string;
    /** Why, when the customer chose a reason. */
    reason: string | null;
    /** The ticket. */
    ticketId: string | null;
    /** The user who asked for help. */
    requesterId: string | null;
    /** The agent it is assigned to. */
    assigneeId: string | null;
    /** The group it is in. */
    groupId: string | null;
    /** When it was created, ISO 8601. */
    createdAt: string;
}

/** A ticket's timings, in minutes; null when not reached yet. */
export interface TicketMetrics {
    /** The ticket. */
    ticketId: string;
    /** Public replies by agents. */
    replies: number;
    /** Times reopened. */
    reopens: number;
    /** Minutes to the first public reply, in calendar and business time. */
    firstReplyMinutes: Minutes;
    /** Minutes to the first resolution. */
    firstResolutionMinutes: Minutes;
    /** Minutes to the final resolution. */
    fullResolutionMinutes: Minutes;
    /** Minutes the requester waited on the team. */
    requesterWaitMinutes: Minutes;
    /** Minutes the ticket was on hold. */
    onHoldMinutes: Minutes;
    /** When it was last solved. */
    solvedAt: string | null;
}

/** A duration in calendar and business minutes. */
export interface Minutes {
    /** In calendar time. */
    calendar: number | null;
    /** In business hours. */
    business: number | null;
}

/** A tag or attachment token for a comment, from `uploadAttachment`. */
export interface Upload {
    /** Pass in `attachments` of `reply`, `addNote` or `createTicket`. Valid for three days. */
    token: string;
    /** The uploaded file. */
    attachment: Attachment;
}

/** A merge in progress: Zendesk merges tickets in a background job. */
export interface MergeJob {
    /** Its id. */
    id: string;
    /** `queued`, `working`, `completed` or `failed`. */
    status: string;
}

export function ticketFrom(raw: unknown): Ticket {
    const via = objectAt(raw, "via");
    const fields: CustomField[] = [];
    for (const field of arrayAt(raw, "custom_fields")) {
        const id = idAt(field, "id");
        if (id !== null) fields.push({ id: id, value: valueAt(field, "value") });
    }
    return {
        id: idAt(raw, "id") ?? "",
        subject: textAt(raw, "subject") ?? "",
        description: textAt(raw, "description") ?? "",
        status: textAt(raw, "status") ?? "",
        priority: textAt(raw, "priority"),
        type: textAt(raw, "type"),
        tags: textsAt(raw, "tags"),
        requesterId: idAt(raw, "requester_id"),
        submitterId: idAt(raw, "submitter_id"),
        assigneeId: idAt(raw, "assignee_id"),
        groupId: idAt(raw, "group_id"),
        organizationId: idAt(raw, "organization_id"),
        brandId: idAt(raw, "brand_id"),
        problemId: idAt(raw, "problem_id"),
        followerIds: idsAt(raw, "follower_ids"),
        emailCcIds: idsAt(raw, "email_cc_ids"),
        channel: textAt(via, "channel") ?? "",
        customFields: fields,
        createdAt: textAt(raw, "created_at") ?? "",
        updatedAt: textAt(raw, "updated_at") ?? "",
        dueAt: textAt(raw, "due_at"),
    };
}

export function attachmentFrom(raw: unknown): Attachment {
    return {
        id: idAt(raw, "id") ?? "",
        fileName: textAt(raw, "file_name") ?? "",
        contentType: textAt(raw, "content_type") ?? "",
        size: numberAt(raw, "size") ?? 0,
        contentUrl: textAt(raw, "content_url") ?? "",
    };
}

export function commentFrom(raw: unknown): Comment {
    const attachments: Attachment[] = [];
    for (const item of arrayAt(raw, "attachments")) attachments.push(attachmentFrom(item));
    return {
        id: idAt(raw, "id") ?? "",
        authorId: idAt(raw, "author_id"),
        body: textAt(raw, "plain_body") ?? textAt(raw, "body") ?? "",
        htmlBody: textAt(raw, "html_body") ?? "",
        public: flagAt(raw, "public"),
        attachments: attachments,
        channel: textAt(objectAt(raw, "via"), "channel") ?? "",
        createdAt: textAt(raw, "created_at") ?? "",
    };
}

export function userFrom(raw: unknown): User {
    return {
        id: idAt(raw, "id") ?? "",
        name: textAt(raw, "name") ?? "",
        email: textAt(raw, "email"),
        phone: textAt(raw, "phone"),
        role: textAt(raw, "role") ?? "",
        organizationId: idAt(raw, "organization_id"),
        tags: textsAt(raw, "tags"),
        notes: textAt(raw, "notes") ?? "",
        details: textAt(raw, "details") ?? "",
        timeZone: textAt(raw, "iana_time_zone") ?? textAt(raw, "time_zone"),
        locale: textAt(raw, "locale"),
        suspended: flagAt(raw, "suspended"),
        userFields: valuesAt(raw, "user_fields"),
        createdAt: textAt(raw, "created_at") ?? "",
        updatedAt: textAt(raw, "updated_at") ?? "",
    };
}

export function organizationFrom(raw: unknown): Organization {
    return {
        id: idAt(raw, "id") ?? "",
        name: textAt(raw, "name") ?? "",
        domainNames: textsAt(raw, "domain_names"),
        groupId: idAt(raw, "group_id"),
        tags: textsAt(raw, "tags"),
        notes: textAt(raw, "notes") ?? "",
        details: textAt(raw, "details") ?? "",
        organizationFields: valuesAt(raw, "organization_fields"),
        createdAt: textAt(raw, "created_at") ?? "",
        updatedAt: textAt(raw, "updated_at") ?? "",
    };
}

export function groupFrom(raw: unknown): Group {
    return {
        id: idAt(raw, "id") ?? "",
        name: textAt(raw, "name") ?? "",
        description: textAt(raw, "description") ?? "",
        default: flagAt(raw, "default"),
    };
}

export function viewFrom(raw: unknown): View {
    return {
        id: idAt(raw, "id") ?? "",
        title: textAt(raw, "title") ?? "",
        active: flagAt(raw, "active"),
        position: numberAt(raw, "position") ?? 0,
        updatedAt: textAt(raw, "updated_at") ?? "",
    };
}

export function macroFrom(raw: unknown): Macro {
    const actions: MacroAction[] = [];
    for (const action of arrayAt(raw, "actions")) {
        actions.push({ field: textAt(action, "field") ?? "", value: valueAt(action, "value") });
    }
    return {
        id: idAt(raw, "id") ?? "",
        title: textAt(raw, "title") ?? "",
        description: textAt(raw, "description") ?? "",
        active: flagAt(raw, "active"),
        actions: actions,
    };
}

export function macroPreviewFrom(raw: unknown): MacroPreview {
    const result = objectAt(raw, "result");
    const ticket = objectAt(result, "ticket");
    const changes: Record<string, unknown> = {};
    let comment: MacroComment | null = null;
    for (const key of Object.keys(ticket)) {
        if (key === "comment") continue;
        changes[key] = ticket[key];
    }
    const raw_comment = ticket["comment"] ?? result["comment"];
    if (raw_comment !== undefined && raw_comment !== null && typeof raw_comment === "object") {
        const body = textAt(raw_comment, "body") ?? "";
        const html = textAt(raw_comment, "html_body") ?? "";
        if (body.length > 0 || html.length > 0) {
            const visibility = (raw_comment as Record<string, unknown>)["public"];
            comment = { body: body, htmlBody: html, public: visibility !== false };
        }
    }
    return { changes: changes, comment: comment };
}

export function ticketFieldFrom(raw: unknown): TicketField {
    const options: FieldOption[] = [];
    for (const option of arrayAt(raw, "custom_field_options")) {
        options.push({ name: textAt(option, "name") ?? "", value: textAt(option, "value") ?? "" });
    }
    return {
        id: idAt(raw, "id") ?? "",
        type: textAt(raw, "type") ?? "",
        title: textAt(raw, "title") ?? "",
        description: textAt(raw, "description") ?? "",
        active: flagAt(raw, "active"),
        required: flagAt(raw, "required"),
        options: options,
    };
}

export function brandFrom(raw: unknown): Brand {
    return {
        id: idAt(raw, "id") ?? "",
        name: textAt(raw, "name") ?? "",
        subdomain: textAt(raw, "subdomain") ?? "",
        default: flagAt(raw, "default"),
        active: flagAt(raw, "active"),
    };
}

export function articleFrom(raw: unknown): Article {
    return {
        id: idAt(raw, "id") ?? "",
        title: textAt(raw, "title") ?? "",
        body: textAt(raw, "body") ?? "",
        locale: textAt(raw, "locale") ?? "",
        sectionId: idAt(raw, "section_id"),
        authorId: idAt(raw, "author_id"),
        draft: flagAt(raw, "draft"),
        htmlUrl: textAt(raw, "html_url") ?? "",
        labelNames: textsAt(raw, "label_names"),
        updatedAt: textAt(raw, "updated_at") ?? "",
    };
}

export function articleHitFrom(raw: unknown): ArticleHit {
    const article = articleFrom(raw);
    return {
        id: article.id, title: article.title, body: article.body, locale: article.locale, sectionId: article.sectionId,
        authorId: article.authorId, draft: article.draft, htmlUrl: article.htmlUrl, labelNames: article.labelNames,
        updatedAt: article.updatedAt, snippet: textAt(raw, "snippet") ?? "",
    };
}

export function sectionFrom(raw: unknown): Section {
    return {
        id: idAt(raw, "id") ?? "",
        name: textAt(raw, "name") ?? "",
        description: textAt(raw, "description") ?? "",
        categoryId: idAt(raw, "category_id"),
        locale: textAt(raw, "locale") ?? "",
        htmlUrl: textAt(raw, "html_url") ?? "",
    };
}

export function categoryFrom(raw: unknown): Category {
    return {
        id: idAt(raw, "id") ?? "",
        name: textAt(raw, "name") ?? "",
        description: textAt(raw, "description") ?? "",
        locale: textAt(raw, "locale") ?? "",
        htmlUrl: textAt(raw, "html_url") ?? "",
    };
}

export function ratingFrom(raw: unknown): SatisfactionRating {
    return {
        id: idAt(raw, "id") ?? "",
        score: textAt(raw, "score") ?? "",
        comment: textAt(raw, "comment") ?? "",
        reason: textAt(raw, "reason"),
        ticketId: idAt(raw, "ticket_id"),
        requesterId: idAt(raw, "requester_id"),
        assigneeId: idAt(raw, "assignee_id"),
        groupId: idAt(raw, "group_id"),
        createdAt: textAt(raw, "created_at") ?? "",
    };
}

export function metricsFrom(raw: unknown, ticketId: string): TicketMetrics {
    return {
        ticketId: ticketId,
        replies: numberAt(raw, "replies") ?? 0,
        reopens: numberAt(raw, "reopens") ?? 0,
        firstReplyMinutes: minutesAt(raw, "reply_time_in_minutes"),
        firstResolutionMinutes: minutesAt(raw, "first_resolution_time_in_minutes"),
        fullResolutionMinutes: minutesAt(raw, "full_resolution_time_in_minutes"),
        requesterWaitMinutes: minutesAt(raw, "requester_wait_time_in_minutes"),
        onHoldMinutes: minutesAt(raw, "on_hold_time_in_minutes"),
        solvedAt: textAt(raw, "solved_at"),
    };
}

function minutesAt(raw: unknown, key: string): Minutes {
    const value = objectAt(raw, key);
    return { calendar: numberAt(value, "calendar"), business: numberAt(value, "business") };
}

export function uploadFrom(raw: unknown): Upload {
    const upload = objectAt(raw, "upload");
    return { token: textAt(upload, "token") ?? "", attachment: attachmentFrom(objectAt(upload, "attachment")) };
}

export function mergeJobFrom(raw: unknown): MergeJob {
    const job = objectAt(raw, "job_status");
    return { id: textAt(job, "id") ?? "", status: textAt(job, "status") ?? "" };
}

// The next page's cursor from a cursor-paged answer: `meta.after_cursor` while
// `meta.has_more` is true.
export function afterCursor(raw: unknown): string | null {
    const meta = objectAt(raw, "meta");
    if (!flagAt(meta, "has_more")) return null;
    return textAt(meta, "after_cursor");
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/** What an error answer says: Zendesk's code, a message and per-field details. */
export interface FailureDetails {
    /** Zendesk's code, or one mapped from the status. */
    errorCode: string;
    /** The error's message. */
    message: string;
    /** Per-field messages. */
    details: string[];
    /** True when the same call may succeed later. */
    retryable: boolean;
}

/**
 * Decode an error answer. Zendesk answers `{ error, description, details }`,
 * `{ error: { title, message } }`, or OAuth's `{ error, error_description }`.
 * @param status The HTTP status.
 * @param statusText The HTTP reason phrase.
 * @param body The answer's body.
 * @returns The error's code, message and details.
 */
export function failureFrom(status: number, statusText: string, body: string): FailureDetails {
    let parsed: unknown = null;
    try {
        parsed = JSON.parse(body);
    } catch (error) {
        parsed = null;
    }
    let code = textAt(parsed, "error") ?? "";
    let detail = textAt(parsed, "description") ?? textAt(parsed, "error_description") ?? "";
    const nested = objectAt(parsed, "error");
    if (code.length === 0 && Object.keys(nested).length > 0) {
        code = textAt(nested, "title") ?? "";
        detail = textAt(nested, "message") ?? detail;
    }
    if (code.length === 0) code = statusCode(status);
    const details: string[] = [];
    const fields = objectAt(parsed, "details");
    for (const field of Object.keys(fields)) {
        for (const item of arrayAt(fields, field)) {
            const text = textAt(item, "description") ?? "";
            if (text.length > 0) details.push(field + ": " + text);
        }
    }
    let message = detail.length > 0
        ? "Zendesk " + status.toString() + " " + code + ": " + detail
        : "Zendesk request failed: HTTP " + status.toString() + " " + statusText;
    if (details.length > 0) message = message + " (" + details.join("; ") + ")";
    const retryable = status === 429 || status === 503 || status === 409;
    return { errorCode: status === 409 ? "UpdateConflict" : code, message: message, details: details, retryable: retryable };
}

// A code for an error answer without one.
export function statusCode(status: number): string {
    if (status === 400) return "BadRequest";
    if (status === 401) return "Unauthorized";
    if (status === 403) return "Forbidden";
    if (status === 404) return "RecordNotFound";
    if (status === 409) return "UpdateConflict";
    if (status === 422) return "RecordInvalid";
    if (status === 429) return "TooManyRequests";
    if (status >= 500) return "ServerError";
    return "HTTP_" + status.toString();
}

// Seconds from a `Retry-After` header, or null when it is absent or not a number of seconds.
export function retryAfterSeconds(header: string): number | null {
    const text = header.trim();
    if (text.length === 0) return null;
    for (let i = 0; i < text.length; i += 1) {
        const code = text.charCodeAt(i);
        if (code < 48 || code > 57) return null;
    }
    return Number(text);
}

// ---------------------------------------------------------------------------
// Reading Zendesk's JSON where its shape is not worth a type: each returns the
// value at `key` when it has the expected kind, else an empty value.
// ---------------------------------------------------------------------------

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

// An id: Zendesk sends numbers; the package uses their decimal text.
export function idAt(raw: unknown, key: string): string | null {
    if (raw === null || raw === undefined || typeof raw !== "object" || Array.isArray(raw)) return null;
    const value = (raw as Record<string, unknown>)[key];
    if (typeof value === "number") return value.toString();
    if (typeof value === "string" && value.length > 0) return value;
    return null;
}

export function idsAt(raw: unknown, key: string): string[] {
    const out: string[] = [];
    for (const value of arrayAt(raw, key)) {
        if (typeof value === "number") out.push(value.toString());
        if (typeof value === "string") out.push(value);
    }
    return out;
}

export function textsAt(raw: unknown, key: string): string[] {
    const out: string[] = [];
    for (const value of arrayAt(raw, key)) {
        if (typeof value === "string") out.push(value);
    }
    return out;
}

export function valueAt(raw: unknown, key: string): CustomValue {
    if (raw === null || raw === undefined || typeof raw !== "object" || Array.isArray(raw)) return null;
    return customValueOf((raw as Record<string, unknown>)[key]);
}

function valuesAt(raw: unknown, key: string): Record<string, CustomValue> {
    const fields = objectAt(raw, key);
    const out: Record<string, CustomValue> = {};
    for (const name of Object.keys(fields)) out[name] = customValueOf(fields[name]);
    return out;
}

function customValueOf(value: unknown): CustomValue {
    if (typeof value === "string" || typeof value === "number" || typeof value === "boolean") return value;
    if (Array.isArray(value)) {
        const tags: string[] = [];
        for (const item of value) {
            if (typeof item === "string") tags.push(item);
        }
        return tags;
    }
    return null;
}
