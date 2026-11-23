// A synchronous, typed client for the Zendesk Support API: tickets, their
// comments (public replies and internal notes) and attachments, users,
// organizations and groups, views and macros, Help Center articles, and
// satisfaction ratings and ticket metrics. Requests go to the account chosen
// when the package is installed (the `accountUrl` build setting); the access
// token is read from the ZENDESK_ACCESS_TOKEN secret inside the package, is
// never an argument and is never returned.
//
// Every operation normalizes its input, checks the normalized values, and sends
// exactly those values. Facts a check needs that the caller does not supply (a
// ticket's group, organization, brand, assignee, requester and status; a user's
// role and organization) are read from Zendesk first, never taken from the
// caller; a ticket that does not exist is checked with "" for them, so a denial
// does not reveal whether it exists. `updateTicket` refuses the fields those
// facts come from, so a scoped rule cannot be bypassed by moving the ticket
// (`assignTicket` is the one checked way to reassign). Ticket writes send
// Zendesk's `safe_update` with the `updated_at` read for the check, so a ticket
// changed since is refused (409) rather than written under a check that saw
// other facts. A public reply emails the requester, so it is its own
// capability, apart from an internal note.
//
// Operations copy caller values (lists, objects) inline before their check,
// element by element: the compiler's check discipline refuses passing a
// caller's value to a helper that runs before `check()`.

import { get, post, put, Response } from "submilli:http";
import { exists, read, stat } from "submilli:fs";
import { check } from "submilli:security";
import { encodeComponent } from "submilli:url";
import { accountUrl } from "./config";
import { validationError, ZendeskError } from "./errors";
import { accessToken } from "./account";
import {
    CustomFieldWrite,
    CustomValue,
    customValue,
    normalizeId,
    normalizeTag,
    optionalId,
    PageOptions,
    pageQuery,
    pageWindow,
    requireDate,
    requireText,
    searchPhrase,
    searchWindow,
    SearchWindow,
    sortedDistinct,
    TicketPriority,
    ticketPriority,
    TicketStatus,
    ticketStatus,
    TicketType,
    ticketType,
} from "./request";
import {
    afterCursor,
    arrayAt,
    Article,
    articleFrom,
    ArticleHit,
    articleHitFrom,
    Brand,
    brandFrom,
    Category,
    categoryFrom,
    Comment,
    commentFrom,
    failureFrom,
    Group,
    groupFrom,
    idAt,
    Macro,
    macroFrom,
    MacroPreview,
    macroPreviewFrom,
    MergeJob,
    mergeJobFrom,
    metricsFrom,
    numberAt,
    objectAt,
    Organization,
    organizationFrom,
    Page,
    ratingFrom,
    retryAfterSeconds,
    SatisfactionRating,
    SearchPage,
    Section,
    sectionFrom,
    textAt,
    textsAt,
    Ticket,
    TicketField,
    ticketFieldFrom,
    ticketFrom,
    TicketMetrics,
    Upload,
    uploadFrom,
    User,
    userFrom,
    View,
    viewFrom,
} from "./response";

export { ZendeskError } from "./errors";
export type { CustomFieldWrite, CustomValue, PageOptions, TicketPriority, TicketStatus, TicketType } from "./request";
export type {
    Article,
    ArticleHit,
    Attachment,
    Brand,
    Category,
    Comment,
    CustomField,
    FieldOption,
    Group,
    Macro,
    MacroAction,
    MacroComment,
    MacroPreview,
    MergeJob,
    Minutes,
    Organization,
    Page,
    SatisfactionRating,
    SearchPage,
    Section,
    Ticket,
    TicketField,
    TicketMetrics,
    Upload,
    User,
    View,
} from "./response";

const ACCOUNT = accountUrl + "/";
const USER_AGENT = "submilli-zendesk/0.1.0";
const MAX_UPLOAD_BYTES = 50 * 1000 * 1000;
const MAX_MERGE = 5;

// ---------------------------------------------------------------------------
// Input types
// ---------------------------------------------------------------------------

/** Which tickets `findTickets` and `countTickets` match. Every filter given must match. */
export interface TicketFilter {
    /** Any of these statuses. */
    status?: TicketStatus[];
    /** In this group. */
    groupId?: string;
    /** Assigned to this agent; null for unassigned tickets. */
    assigneeId?: string | null;
    /** Requested by this user. */
    requesterId?: string;
    /** In this organization. */
    organizationId?: string;
    /** In this brand. */
    brandId?: string;
    /** Carrying this tag. One tag: Zendesk's search matches repeated tag terms as
     * any of them, so a check on the tags given could not hold for the results. */
    tag?: string;
    /** Created on or after this date, `YYYY-MM-DD` or a date-time with a zone. */
    createdAfter?: string;
    /** Created before this date. */
    createdBefore?: string;
    /** Changed on or after this date. */
    updatedAfter?: string;
    /** Changed before this date. */
    updatedBefore?: string;
    /** Containing this phrase in the subject, description or comments. */
    text?: string;
}

/** How `findTickets` orders and pages. */
export interface FindOptions extends PageOptions {
    /** Default `updated_at`. */
    sortBy?: "created_at" | "updated_at" | "priority" | "status" | "ticket_type";
    /** Default `desc`. */
    sortOrder?: "asc" | "desc";
}

/** A new ticket. */
export interface TicketInput {
    /** The subject line. */
    subject: string;
    /** The first comment. */
    body: string;
    /** True when `body` is HTML. */
    html?: boolean;
    /** False for an internal first comment; default true, which the requester sees. */
    public?: boolean;
    /** Default the token's user. */
    requesterId?: string;
    /** The group to put it in. */
    groupId?: string;
    /** The agent to assign it to. */
    assigneeId?: string;
    /** The organization; default the requester's. */
    organizationId?: string;
    /** The brand; default the account's default. */
    brandId?: string;
    /** `low`, `normal`, `high` or `urgent`. */
    priority?: TicketPriority;
    /** `question`, `incident`, `problem` or `task`. */
    type?: TicketType;
    /** Tags to set. */
    tags?: string[];
    /** Custom fields to set. */
    customFields?: CustomFieldWrite[];
    /** A task's due date-time. */
    dueAt?: string;
    /** `token`s from `uploadAttachment`. */
    attachments?: string[];
}

/** Changes to a ticket. A field left out keeps its value; null clears it. */
export interface TicketUpdate {
    /** A new subject. */
    subject?: string;
    /** A new status. */
    status?: TicketStatus;
    /** A new priority; null clears it. */
    priority?: TicketPriority | null;
    /** A new type; null clears it. */
    type?: TicketType | null;
    /** Replaces every tag. */
    tags?: string[];
    /** Adds these tags, keeping the others, also those added meanwhile. */
    addTags?: string[];
    /** Removes these tags. */
    removeTags?: string[];
    /** Custom fields to set. */
    customFields?: CustomFieldWrite[];
    /** A task's due date-time; null clears it. */
    dueAt?: string | null;
}

/** Where `assignTicket` moves a ticket. */
export interface Assignment {
    /** The group to move it to. */
    groupId?: string;
    /** The agent to assign; null to unassign. The agent must be in the ticket's group. */
    assigneeId?: string | null;
}

/** A comment's options. */
export interface CommentOptions {
    /** True when the body is HTML. */
    html?: boolean;
    /** `token`s from `uploadAttachment`. */
    attachments?: string[];
}

/** A reply's options. */
export interface ReplyOptions extends CommentOptions {
    /** Set the ticket's status with the reply, such as `pending` or `solved`. */
    status?: TicketStatus;
}

/** A merge's comments, added to the target and to each source. */
export interface MergeOptions {
    /** A comment added to the target ticket. */
    targetComment?: string;
    /** A comment added to each source ticket. */
    sourceComment?: string;
    /** True to make the comments public; default false. */
    public?: boolean;
}

/** An upload's name and type. */
export interface UploadOptions {
    /** Default the file's name. */
    fileName?: string;
    /** Default `application/octet-stream`. */
    contentType?: string;
}

/** One custom user field to write: its key and its value; null clears it. */
export interface UserFieldWrite {
    /** The field's key, as in `User.userFields`. */
    key: string;
    /** The value. */
    value: CustomValue;
}

/** A new end user. */
export interface UserInput {
    /** Its name. */
    name: string;
    /** Primary email address. */
    email?: string;
    /** Phone number. */
    phone?: string;
    /** The organization to add the user to. */
    organizationId?: string;
    /** Tags to set. */
    tags?: string[];
    /** Notes about it, which only agents see. */
    notes?: string;
    /** Details, such as an address, which only agents see. */
    details?: string;
    /** Custom user fields to set. */
    userFields?: UserFieldWrite[];
}

/** Changes to a user. A field left out keeps its value. */
export interface UserUpdate {
    /** Its name. */
    name?: string;
    /** A new phone number; null clears it. */
    phone?: string | null;
    /** Replaces every tag. */
    tags?: string[];
    /** Notes about it, which only agents see. */
    notes?: string;
    /** Details, such as an address, which only agents see. */
    details?: string;
    /** Custom user fields to set. */
    userFields?: UserFieldWrite[];
}

/** Which articles `searchArticles` reads. */
export interface ArticleSearchOptions extends PageOptions {
    /** Such as `en-us`; default every locale. */
    locale?: string;
    /** Only this section's articles. */
    sectionId?: string;
}

/** Which Help Center locale to read. */
export interface LocaleOptions {
    /** Such as `en-us`; default the Help Center's default. */
    locale?: string;
}

/** Which sections `listSections` reads. */
export interface SectionOptions extends LocaleOptions {
    /** Only this category's sections. */
    categoryId?: string;
}

/** Which ratings `listSatisfactionRatings` reads. */
export interface RatingOptions extends PageOptions {
    /** `good`, `bad`, `offered`, `unoffered`, `received` (good or bad), or default every score. */
    score?: "good" | "bad" | "offered" | "unoffered" | "received";
}

/** Which objects raw `search` finds. */
export interface SearchOptions extends PageOptions {
    /** Default the type the query names, else every type. */
    type?: "ticket" | "user" | "organization" | "group";
}

// ---------------------------------------------------------------------------
// Tickets
// ---------------------------------------------------------------------------

/**
 * Get a ticket; null when it does not exist.
 * @param id Ticket id.
 * @returns The ticket, or null.
 * @capability zendesk.com/tickets.get { group: string, organization: string, brand: string, assignee: string, requester: string, status: string }
 */
export function getTicket(id: string): Ticket | null {
    const ticketId = normalizeId(id, "id");
    const raw = readTicket(ticketId);
    const facts = factsOf(raw);
    check("zendesk.com/tickets.get", {
        group: facts.group, organization: facts.organization, brand: facts.brand,
        assignee: facts.assignee, requester: facts.requester, status: facts.status,
    });
    return raw === null ? null : ticketFrom(raw);
}

/**
 * Find tickets, most recently changed first. Search reads at most 1,000 results.
 * @param filter Every filter given must match.
 * @param options Order and page.
 * @returns One page of tickets and how many match.
 * @capability zendesk.com/tickets.find { group: string, organization: string, brand: string, assignee: string, requester: string, status: string[], tag: string }
 */
export function findTickets(filter: TicketFilter = {}, options: FindOptions = {}): SearchPage<Ticket> {
    const { status, groupId, assigneeId, requesterId, organizationId, brandId, tag, createdAfter, createdBefore, updatedAfter, updatedBefore, text } = filter;
    const statuses: string[] = [];
    for (const item of status ?? []) statuses.push(ticketStatus(item, "status"));
    const tagged = tag === undefined ? "" : normalizeTag(tag, "tag");
    const { sortBy, sortOrder, pageSize, cursor } = options;
    const window = searchWindow({ pageSize: pageSize, cursor: cursor });
    const group = optionalId(groupId, "groupId");
    const assignee = assigneeId === null ? "none" : optionalId(assigneeId, "assigneeId");
    const requester = optionalId(requesterId, "requesterId");
    const organization = optionalId(organizationId, "organizationId");
    const brand = optionalId(brandId, "brandId");
    const terms = ticketTerms(sortedDistinct(statuses), group, assignee, requester, organization, brand, tagged);
    dateTerm(terms, "created>=", createdAfter, "createdAfter");
    dateTerm(terms, "created<", createdBefore, "createdBefore");
    dateTerm(terms, "updated>=", updatedAfter, "updatedAfter");
    dateTerm(terms, "updated<", updatedBefore, "updatedBefore");
    if (text !== undefined) terms.push(searchPhrase(text, "text"));
    const order = sortOrder ?? "desc";
    const sort = sortBy ?? "updated_at";
    if (order !== "asc" && order !== "desc") throw validationError("invalid_input", "sortOrder must be asc or desc");
    if (!["created_at", "updated_at", "priority", "status", "ticket_type"].includes(sort)) {
        throw validationError("invalid_input", "sortBy must be created_at, updated_at, priority, status or ticket_type");
    }
    check("zendesk.com/tickets.find", {
        group: group, organization: organization, brand: brand, assignee: assignee, requester: requester,
        status: sortedDistinct(statuses), tag: tagged,
    });
    const answer = searchGet(terms.join(" "), window, "&sort_by=" + sort + "&sort_order=" + order);
    const items: Ticket[] = [];
    for (const raw of arrayAt(answer, "results")) items.push(ticketFrom(raw));
    return searchPageOf(answer, items, window);
}

/**
 * Count tickets without reading them.
 * @param filter Every filter given must match, as in `findTickets` (`text` included).
 * @returns How many tickets match.
 * @capability zendesk.com/tickets.count { group: string, organization: string, brand: string, assignee: string, requester: string, status: string[], tag: string }
 */
export function countTickets(filter: TicketFilter = {}): number {
    const { status, groupId, assigneeId, requesterId, organizationId, brandId, tag, createdAfter, createdBefore, updatedAfter, updatedBefore, text } = filter;
    const statuses: string[] = [];
    for (const item of status ?? []) statuses.push(ticketStatus(item, "status"));
    const tagged = tag === undefined ? "" : normalizeTag(tag, "tag");
    const group = optionalId(groupId, "groupId");
    const assignee = assigneeId === null ? "none" : optionalId(assigneeId, "assigneeId");
    const requester = optionalId(requesterId, "requesterId");
    const organization = optionalId(organizationId, "organizationId");
    const brand = optionalId(brandId, "brandId");
    const terms = ticketTerms(sortedDistinct(statuses), group, assignee, requester, organization, brand, tagged);
    dateTerm(terms, "created>=", createdAfter, "createdAfter");
    dateTerm(terms, "created<", createdBefore, "createdBefore");
    dateTerm(terms, "updated>=", updatedAfter, "updatedAfter");
    dateTerm(terms, "updated<", updatedBefore, "updatedBefore");
    if (text !== undefined) terms.push(searchPhrase(text, "text"));
    check("zendesk.com/tickets.count", {
        group: group, organization: organization, brand: brand, assignee: assignee, requester: requester,
        status: sortedDistinct(statuses), tag: tagged,
    });
    const answer = body(zdGet("search/count.json?query=" + encodeComponent(terms.join(" "))));
    return numberAt(answer, "count") ?? 0;
}

/**
 * Create a ticket. A public first comment is also checked as a reply, since the
 * requester sees it.
 * @param input The ticket.
 * @returns The new ticket.
 * @capability zendesk.com/tickets.create { group: string, organization: string, brand: string, assignee: string, requester: string, public: boolean, fields: string[] }
 * @capability zendesk.com/comments.reply { group: string, organization: string, brand: string, assignee: string, requester: string, status: string, previousStatus: string }
 */
export function createTicket(input: TicketInput): Ticket {
    const {
        subject, body: text, html, public: visible, requesterId, groupId, assigneeId, organizationId, brandId,
        priority, type, tags, customFields, dueAt, attachments,
    } = input;
    const tagList: string[] = [];
    for (const tag of tags ?? []) tagList.push(normalizeTag(tag, "tags"));
    const custom: CustomFieldWrite[] = [];
    for (const field of customFields ?? []) {
        const { id, value } = field;
        let copied: CustomValue = null;
        if (value === null || typeof value === "string" || typeof value === "number" || typeof value === "boolean") {
            copied = value;
        } else {
            const options: string[] = [];
            for (const option of value) options.push(option);
            copied = options;
        }
        custom.push({ id: id, value: copied });
    }
    const tokens: string[] = [];
    for (const token of attachments ?? []) tokens.push(requireText(token, "attachments"));
    const isPublic = visible !== false;
    const requester = optionalId(requesterId, "requesterId");
    const group = optionalId(groupId, "groupId");
    const assignee = optionalId(assigneeId, "assigneeId");
    const organization = optionalId(organizationId, "organizationId");
    const brand = optionalId(brandId, "brandId");
    const ticket: Record<string, unknown> = {
        subject: requireText(subject, "subject"),
        comment: commentBody(requireText(text, "body"), html === true, isPublic, tokens),
    };
    const fields: string[] = ["subject"];
    if (requester.length > 0) ticket["requester_id"] = Number(requester);
    if (group.length > 0) ticket["group_id"] = Number(group);
    if (assignee.length > 0) ticket["assignee_id"] = Number(assignee);
    if (organization.length > 0) ticket["organization_id"] = Number(organization);
    if (brand.length > 0) ticket["brand_id"] = Number(brand);
    if (priority !== undefined) {
        ticket["priority"] = ticketPriority(priority, "priority");
        fields.push("priority");
    }
    if (type !== undefined) {
        ticket["type"] = ticketType(type, "type");
        fields.push("type");
    }
    if (tags !== undefined) {
        ticket["tags"] = tagList;
        fields.push("tags");
    }
    if (dueAt !== undefined) {
        ticket["due_at"] = requireDate(dueAt, "dueAt");
        fields.push("due_at");
    }
    if (custom.length > 0) {
        ticket["custom_fields"] = customWrites(custom, fields);
    }
    check("zendesk.com/tickets.create", {
        group: group, organization: organization, brand: brand, assignee: assignee, requester: requester,
        public: isPublic, fields: sortedDistinct(fields),
    });
    if (isPublic) {
        check("zendesk.com/comments.reply", {
            group: group, organization: organization, brand: brand, assignee: assignee, requester: requester,
            // Zendesk opens a ticket created with an assignee.
            status: assignee.length > 0 ? "open" : "new", previousStatus: "",
        });
    }
    const answer = body(zdPost("tickets.json", { ticket: ticket }));
    return ticketFrom(objectAt(answer, "ticket"));
}

/**
 * Change a ticket's subject, status, priority, type, tags, custom fields or due
 * date. Its group, assignee, requester, organization and brand do not change
 * here (`assignTicket` moves a ticket), so the check's facts stay the ticket's.
 * @param id Ticket id.
 * @param changes The fields to change.
 * @returns The ticket as changed.
 * @capability zendesk.com/tickets.update { group: string, organization: string, brand: string, assignee: string, requester: string, status: string, previousStatus: string, fields: string[] }
 */
export function updateTicket(id: string, changes: TicketUpdate): Ticket {
    const ticketId = normalizeId(id, "id");
    const { subject, status, priority, type, tags, addTags, removeTags, customFields, dueAt } = changes;
    const tagList: string[] = [];
    for (const tag of tags ?? []) tagList.push(normalizeTag(tag, "tags"));
    const added: string[] = [];
    for (const tag of addTags ?? []) added.push(normalizeTag(tag, "addTags"));
    const removed: string[] = [];
    for (const tag of removeTags ?? []) removed.push(normalizeTag(tag, "removeTags"));
    const custom: CustomFieldWrite[] = [];
    for (const field of customFields ?? []) {
        const { id: fieldId, value } = field;
        let copied: CustomValue = null;
        if (value === null || typeof value === "string" || typeof value === "number" || typeof value === "boolean") {
            copied = value;
        } else {
            const options: string[] = [];
            for (const option of value) options.push(option);
            copied = options;
        }
        custom.push({ id: fieldId, value: copied });
    }
    const ticket: Record<string, unknown> = {};
    const fields: string[] = [];
    if (subject !== undefined) {
        ticket["subject"] = requireText(subject, "subject");
        fields.push("subject");
    }
    const newStatus = status === undefined ? "" : ticketStatus(status, "status");
    if (newStatus.length > 0) {
        ticket["status"] = newStatus;
        fields.push("status");
    }
    if (priority !== undefined) {
        ticket["priority"] = priority === null ? null : ticketPriority(priority, "priority");
        fields.push("priority");
    }
    if (type !== undefined) {
        ticket["type"] = type === null ? null : ticketType(type, "type");
        fields.push("type");
    }
    if (tags !== undefined) {
        if (added.length > 0 || removed.length > 0) throw validationError("invalid_input", "give tags, or addTags and removeTags, not both");
        ticket["tags"] = tagList;
        fields.push("tags");
    }
    if (added.length > 0 || removed.length > 0) fields.push("tags");
    if (dueAt !== undefined) {
        ticket["due_at"] = dueAt === null ? null : requireDate(dueAt, "dueAt");
        fields.push("due_at");
    }
    if (custom.length > 0) ticket["custom_fields"] = customWrites(custom, fields);
    if (fields.length === 0) throw validationError("invalid_input", "changes cannot be empty");
    const raw = readTicket(ticketId);
    const facts = factsOf(raw);
    check("zendesk.com/tickets.update", {
        group: facts.group, organization: facts.organization, brand: facts.brand, assignee: facts.assignee,
        requester: facts.requester, status: newStatus, previousStatus: facts.status, fields: sortedDistinct(fields),
    });
    // Zendesk ignores `additional_tags` and `remove_tags` on a single ticket's
    // update, so the whole list is written, computed from the tags read for the
    // check; `safe_update` refuses it if they changed since.
    if (raw !== null && (added.length > 0 || removed.length > 0)) {
        const next: string[] = [];
        for (const tag of textsAt(raw, "tags")) if (!removed.includes(tag) && !next.includes(tag)) next.push(tag);
        for (const tag of added) if (!removed.includes(tag) && !next.includes(tag)) next.push(tag);
        ticket["tags"] = next;
    }
    return writeTicket(ticketId, raw, ticket);
}

/**
 * Move a ticket to another group or agent, or unassign it.
 * @param id Ticket id.
 * @param assignment The group, the agent, or both.
 * @returns The ticket as changed.
 * @capability zendesk.com/tickets.assign { group: string, organization: string, brand: string, assignee: string, requester: string, status: string, toGroup: string, toAssignee: string }
 */
export function assignTicket(id: string, assignment: Assignment): Ticket {
    const ticketId = normalizeId(id, "id");
    const { groupId, assigneeId } = assignment;
    const toGroup = optionalId(groupId, "groupId");
    const toAssignee = assigneeId === null ? "none" : optionalId(assigneeId, "assigneeId");
    if (toGroup.length === 0 && toAssignee.length === 0) throw validationError("invalid_input", "give groupId, assigneeId or both");
    const ticket: Record<string, unknown> = {};
    if (toGroup.length > 0) ticket["group_id"] = Number(toGroup);
    if (toAssignee === "none") ticket["assignee_id"] = null;
    else if (toAssignee.length > 0) ticket["assignee_id"] = Number(toAssignee);
    const raw = readTicket(ticketId);
    const facts = factsOf(raw);
    // An agent outside the ticket's group would move it to the agent's default
    // group, which the check never saw: the group is sent as it is, so Zendesk
    // refuses the agent instead, and a ticket with no group needs one named.
    if (toGroup.length === 0 && toAssignee.length > 0 && toAssignee !== "none") {
        if (raw !== null && facts.group.length === 0) {
            throw validationError("invalid_input", "the ticket has no group: give groupId with assigneeId");
        }
        if (facts.group.length > 0) ticket["group_id"] = Number(facts.group);
    }
    check("zendesk.com/tickets.assign", {
        group: facts.group, organization: facts.organization, brand: facts.brand, assignee: facts.assignee,
        requester: facts.requester, status: facts.status, toGroup: toGroup, toAssignee: toAssignee,
    });
    return writeTicket(ticketId, raw, ticket);
}

/**
 * Merge tickets into one: each source is closed with a comment pointing to the
 * target, and its comments are copied there. Zendesk merges in a background job.
 * @param targetId The ticket to keep.
 * @param sourceIds Up to 5 tickets to merge into it.
 * @param options The comments added to the target and the sources.
 * @returns The merge job.
 * @capability zendesk.com/tickets.merge { role: string, group: string, organization: string, brand: string, assignee: string, requester: string, status: string }
 * @capability zendesk.com/comments.reply { group: string, organization: string, brand: string, assignee: string, requester: string, status: string, previousStatus: string }
 */
export function mergeTickets(targetId: string, sourceIds: string[], options: MergeOptions = {}): MergeJob {
    const target = normalizeId(targetId, "targetId");
    const sources: string[] = [];
    for (const source of sourceIds) sources.push(normalizeId(source, "sourceIds"));
    const { targetComment, sourceComment, public: visible } = options;
    if (sources.length === 0 || sources.length > MAX_MERGE) throw validationError("invalid_input", "sourceIds must name 1 to 5 tickets");
    if (sources.includes(target)) throw validationError("invalid_input", "sourceIds cannot include the target");
    const request: Record<string, unknown> = { ids: numbers(sortedDistinct(sources)) };
    if (targetComment !== undefined) request["target_comment"] = requireText(targetComment, "targetComment");
    if (sourceComment !== undefined) request["source_comment"] = requireText(sourceComment, "sourceComment");
    request["target_comment_is_public"] = visible === true;
    request["source_comment_is_public"] = visible === true;
    const targetFacts = factsOf(readTicket(target));
    check("zendesk.com/tickets.merge", {
        role: "target", group: targetFacts.group, organization: targetFacts.organization, brand: targetFacts.brand,
        assignee: targetFacts.assignee, requester: targetFacts.requester, status: targetFacts.status,
    });
    // Public merge comments reach every requester, so each is also a reply.
    if (visible === true) {
        check("zendesk.com/comments.reply", {
            group: targetFacts.group, organization: targetFacts.organization, brand: targetFacts.brand,
            assignee: targetFacts.assignee, requester: targetFacts.requester, status: targetFacts.status, previousStatus: targetFacts.status,
        });
    }
    for (const source of sortedDistinct(sources)) {
        const facts = factsOf(readTicket(source));
        check("zendesk.com/tickets.merge", {
            role: "source", group: facts.group, organization: facts.organization, brand: facts.brand,
            assignee: facts.assignee, requester: facts.requester, status: facts.status,
        });
        if (visible === true) {
            check("zendesk.com/comments.reply", {
                group: facts.group, organization: facts.organization, brand: facts.brand,
                assignee: facts.assignee, requester: facts.requester, status: "closed", previousStatus: facts.status,
            });
        }
    }
    return mergeJobFrom(body(zdPost("tickets/" + target + "/merge.json", request)));
}

/**
 * A ticket's timings: replies, reopens, and minutes to first reply and to resolution.
 * @param id Ticket id.
 * @returns The metrics, or null when the ticket does not exist.
 * @capability zendesk.com/tickets.metrics { group: string, organization: string, brand: string, assignee: string, requester: string, status: string }
 */
export function ticketMetrics(id: string): TicketMetrics | null {
    const ticketId = normalizeId(id, "id");
    const raw = readTicket(ticketId);
    const facts = factsOf(raw);
    check("zendesk.com/tickets.metrics", {
        group: facts.group, organization: facts.organization, brand: facts.brand,
        assignee: facts.assignee, requester: facts.requester, status: facts.status,
    });
    if (raw === null) return null;
    const answer = zdFind("tickets/" + ticketId + "/metrics.json");
    return answer === null ? null : metricsFrom(objectAt(body(answer), "ticket_metric"), ticketId);
}

// ---------------------------------------------------------------------------
// Conversation
// ---------------------------------------------------------------------------

/**
 * A ticket's comments, oldest first: public replies and internal notes.
 * @param id Ticket id.
 * @param page The page.
 * @returns One page of comments.
 * @capability zendesk.com/comments.list { group: string, organization: string, brand: string, assignee: string, requester: string, status: string }
 */
export function listComments(id: string, page: PageOptions = {}): Page<Comment> {
    const ticketId = normalizeId(id, "id");
    const { pageSize, cursor } = page;
    const window = pageWindow({ pageSize: pageSize, cursor: cursor });
    const facts = factsOf(readTicket(ticketId));
    check("zendesk.com/comments.list", {
        group: facts.group, organization: facts.organization, brand: facts.brand,
        assignee: facts.assignee, requester: facts.requester, status: facts.status,
    });
    const answer = body(zdGet("tickets/" + ticketId + "/comments.json?" + pageQuery(window)));
    const items: Comment[] = [];
    for (const raw of arrayAt(answer, "comments")) items.push(commentFrom(raw));
    return { items: items, nextCursor: afterCursor(answer) };
}

/**
 * Add an internal note: only agents see it.
 * @param id Ticket id.
 * @param text The note.
 * @param options HTML and attachments.
 * @returns The ticket as changed.
 * @capability zendesk.com/comments.note { group: string, organization: string, brand: string, assignee: string, requester: string, status: string }
 */
export function addNote(id: string, text: string, options: CommentOptions = {}): Ticket {
    const ticketId = normalizeId(id, "id");
    const note = requireText(text, "text");
    const { html, attachments } = options;
    const tokens: string[] = [];
    for (const token of attachments ?? []) tokens.push(requireText(token, "attachments"));
    const raw = readTicket(ticketId);
    const facts = factsOf(raw);
    check("zendesk.com/comments.note", {
        group: facts.group, organization: facts.organization, brand: facts.brand,
        assignee: facts.assignee, requester: facts.requester, status: facts.status,
    });
    return writeTicket(ticketId, raw, { comment: commentBody(note, html === true, false, tokens) });
}

/**
 * Reply publicly: the requester and anyone copied see it, and Zendesk emails them.
 * @param id Ticket id.
 * @param text The reply.
 * @param options HTML, attachments, and a status to set with the reply.
 * @returns The ticket as changed.
 * @capability zendesk.com/comments.reply { group: string, organization: string, brand: string, assignee: string, requester: string, status: string, previousStatus: string }
 */
export function reply(id: string, text: string, options: ReplyOptions = {}): Ticket {
    const ticketId = normalizeId(id, "id");
    const message = requireText(text, "text");
    const { html, attachments, status } = options;
    const tokens: string[] = [];
    for (const token of attachments ?? []) tokens.push(requireText(token, "attachments"));
    const newStatus = status === undefined ? "" : ticketStatus(status, "status");
    const ticket: Record<string, unknown> = { comment: commentBody(message, html === true, true, tokens) };
    if (newStatus.length > 0) ticket["status"] = newStatus;
    const raw = readTicket(ticketId);
    const facts = factsOf(raw);
    check("zendesk.com/comments.reply", {
        group: facts.group, organization: facts.organization, brand: facts.brand,
        assignee: facts.assignee, requester: facts.requester, status: newStatus, previousStatus: facts.status,
    });
    return writeTicket(ticketId, raw, ticket);
}

/**
 * Upload a file from the VFS, to attach to a reply, a note or a new ticket.
 * @param path The VFS file, at most 50 MB.
 * @param options Its name and content type.
 * @returns The token to pass in `attachments`, and the attachment.
 * @capability zendesk.com/attachments.upload { path: string, fileName: string, contentType: string }
 */
export function uploadAttachment(path: string, options: UploadOptions = {}): Upload {
    const file = requireText(path, "path");
    const { fileName, contentType } = options;
    const slash = file.lastIndexOf("/");
    const name = fileName === undefined ? file.slice(slash + 1) : requireText(fileName, "fileName");
    if (name.length === 0) throw validationError("invalid_input", "path must name a file");
    const type = contentType === undefined ? "application/octet-stream" : requireText(contentType, "contentType");
    check("zendesk.com/attachments.upload", { path: file, fileName: name, contentType: type });
    const metadata = exists(file) ? stat(file) : undefined;
    if (metadata === undefined || metadata.kind !== "file") throw validationError("file_not_found", "not a VFS file: " + file);
    if (metadata.size > MAX_UPLOAD_BYTES) throw validationError("file_too_large", "files above 50 MB cannot be uploaded");
    const sent = headers();
    sent.set("Content-Type", type);
    const answer = requireOk(post(ACCOUNT + "api/v2/uploads.json?filename=" + encodeComponent(name), read(file), sent));
    return uploadFrom(body(answer));
}

// ---------------------------------------------------------------------------
// People
// ---------------------------------------------------------------------------

/**
 * Get a user; null when it does not exist.
 * @param id User id.
 * @returns The user, or null.
 * @capability zendesk.com/users.get { role: string, organization: string }
 */
export function getUser(id: string): User | null {
    const userId = normalizeId(id, "id");
    const answer = zdFind("users/" + userId + ".json");
    const raw = answer === null ? null : objectAt(body(answer), "user");
    check("zendesk.com/users.get", { role: textAt(raw, "role") ?? "", organization: idAt(raw, "organization_id") ?? "" });
    return raw === null ? null : userFrom(raw);
}

/**
 * Find users by name or email, or by Zendesk's user search syntax.
 * @param query Such as `jane@acme.com` or `Jane`.
 * @param page The page; search reads at most 1,000 results.
 * @returns One page of users.
 * @capability zendesk.com/users.find { query: string }
 */
export function findUsers(query: string, page: PageOptions = {}): SearchPage<User> {
    const text = requireText(query, "query");
    const { pageSize, cursor } = page;
    const window = searchWindow({ pageSize: pageSize, cursor: cursor });
    check("zendesk.com/users.find", { query: text });
    const answer = body(zdGet("users/search.json?query=" + encodeComponent(text) + numberedPage(window)));
    const items: User[] = [];
    for (const raw of arrayAt(answer, "users")) items.push(userFrom(raw));
    return searchPageOf(answer, items, window);
}

/**
 * Create an end user, such as a customer to open a ticket for. Agents and
 * admins are created in Zendesk.
 * @param input The user.
 * @returns The new user.
 * @capability zendesk.com/users.create { organization: string, fields: string[] }
 */
export function createUser(input: UserInput): User {
    const { name, email, phone, organizationId, tags, notes, details, userFields } = input;
    const tagList: string[] = [];
    for (const tag of tags ?? []) tagList.push(normalizeTag(tag, "tags"));
    const custom: Record<string, CustomValue> = {};
    for (const field of userFields ?? []) {
        const { key, value } = field;
        let copied: CustomValue = null;
        if (value === null || typeof value === "string" || typeof value === "number" || typeof value === "boolean") {
            copied = value;
        } else {
            const options: string[] = [];
            for (const option of value) options.push(option);
            copied = options;
        }
        const name = requireText(key, "userFields");
        if (name in custom) throw validationError("invalid_input", "userFields names " + name + " twice");
        custom[name] = customValue(copied, "userFields");
    }
    const organization = optionalId(organizationId, "organizationId");
    const user: Record<string, unknown> = { name: requireText(name, "name"), role: "end-user" };
    const fields: string[] = ["name"];
    if (email !== undefined) {
        user["email"] = requireText(email, "email");
        fields.push("email");
    }
    if (phone !== undefined) {
        user["phone"] = requireText(phone, "phone");
        fields.push("phone");
    }
    if (organization.length > 0) user["organization_id"] = Number(organization);
    if (tags !== undefined) {
        user["tags"] = tagList;
        fields.push("tags");
    }
    if (notes !== undefined) {
        user["notes"] = notes;
        fields.push("notes");
    }
    if (details !== undefined) {
        user["details"] = details;
        fields.push("details");
    }
    const keys = Object.keys(custom);
    if (keys.length > 0) {
        user["user_fields"] = custom;
        for (const key of keys) fields.push("user_fields." + key);
    }
    check("zendesk.com/users.create", { organization: organization, fields: sortedDistinct(fields) });
    return userFrom(objectAt(body(zdPost("users.json", { user: user })), "user"));
}

/**
 * Change a user's name, phone, tags, notes, details or custom fields. Role,
 * email and organization do not change here.
 * @param id User id.
 * @param changes The fields to change.
 * @returns The user as changed.
 * @capability zendesk.com/users.update { role: string, organization: string, fields: string[] }
 */
export function updateUser(id: string, changes: UserUpdate): User {
    const userId = normalizeId(id, "id");
    const { name, phone, tags, notes, details, userFields } = changes;
    const tagList: string[] = [];
    for (const tag of tags ?? []) tagList.push(normalizeTag(tag, "tags"));
    const custom: Record<string, CustomValue> = {};
    for (const field of userFields ?? []) {
        const { key, value } = field;
        let copied: CustomValue = null;
        if (value === null || typeof value === "string" || typeof value === "number" || typeof value === "boolean") {
            copied = value;
        } else {
            const options: string[] = [];
            for (const option of value) options.push(option);
            copied = options;
        }
        const name = requireText(key, "userFields");
        if (name in custom) throw validationError("invalid_input", "userFields names " + name + " twice");
        custom[name] = customValue(copied, "userFields");
    }
    const user: Record<string, unknown> = {};
    const fields: string[] = [];
    if (name !== undefined) {
        user["name"] = requireText(name, "name");
        fields.push("name");
    }
    if (phone !== undefined) {
        user["phone"] = phone === null ? null : requireText(phone, "phone");
        fields.push("phone");
    }
    if (tags !== undefined) {
        user["tags"] = tagList;
        fields.push("tags");
    }
    if (notes !== undefined) {
        user["notes"] = notes;
        fields.push("notes");
    }
    if (details !== undefined) {
        user["details"] = details;
        fields.push("details");
    }
    const keys = Object.keys(custom);
    if (keys.length > 0) {
        user["user_fields"] = custom;
        for (const key of keys) fields.push("user_fields." + key);
    }
    if (fields.length === 0) throw validationError("invalid_input", "changes cannot be empty");
    const answer = zdFind("users/" + userId + ".json");
    const raw = answer === null ? null : objectAt(body(answer), "user");
    check("zendesk.com/users.update", {
        role: textAt(raw, "role") ?? "", organization: idAt(raw, "organization_id") ?? "", fields: sortedDistinct(fields),
    });
    if (raw === null) throw new ZendeskError("RecordNotFound", "Zendesk user " + userId + " was not found", [], 404, false, null);
    return userFrom(objectAt(body(zdPut("users/" + userId + ".json", { user: user })), "user"));
}

/**
 * Get an organization; null when it does not exist.
 * @param id Organization id.
 * @returns The organization, or null.
 * @capability zendesk.com/organizations.get { organization: string }
 */
export function getOrganization(id: string): Organization | null {
    const organizationId = normalizeId(id, "id");
    check("zendesk.com/organizations.get", { organization: organizationId });
    const answer = zdFind("organizations/" + organizationId + ".json");
    return answer === null ? null : organizationFrom(objectAt(body(answer), "organization"));
}

/**
 * Find organizations whose name starts with this text.
 * @param name At least two characters.
 * @param page The page.
 * @returns One page of organizations.
 * @capability zendesk.com/organizations.find { name: string }
 */
export function findOrganizations(name: string, page: PageOptions = {}): SearchPage<Organization> {
    const text = requireText(name, "name");
    if (text.length < 2) throw validationError("invalid_input", "name must have at least two characters");
    const { pageSize, cursor } = page;
    const window = searchWindow({ pageSize: pageSize, cursor: cursor });
    check("zendesk.com/organizations.find", { name: text });
    const answer = body(zdGet("organizations/autocomplete.json?name=" + encodeComponent(text) + numberedPage(window)));
    const items: Organization[] = [];
    for (const raw of arrayAt(answer, "organizations")) items.push(organizationFrom(raw));
    return searchPageOf(answer, items, window);
}

/**
 * The account's groups of agents.
 * @param page The page.
 * @returns One page of groups.
 * @capability zendesk.com/groups.list {}
 */
export function listGroups(page: PageOptions = {}): Page<Group> {
    const { pageSize, cursor } = page;
    const window = pageWindow({ pageSize: pageSize, cursor: cursor });
    check("zendesk.com/groups.list", {});
    const answer = body(zdGet("groups.json?" + pageQuery(window)));
    const items: Group[] = [];
    for (const raw of arrayAt(answer, "groups")) items.push(groupFrom(raw));
    return { items: items, nextCursor: afterCursor(answer) };
}

/**
 * The agents in a group.
 * @param groupId Group id.
 * @param page The page.
 * @returns One page of agents.
 * @capability zendesk.com/groups.members { group: string }
 */
export function listAgents(groupId: string, page: PageOptions = {}): Page<User> {
    const group = normalizeId(groupId, "groupId");
    const { pageSize, cursor } = page;
    const window = pageWindow({ pageSize: pageSize, cursor: cursor });
    check("zendesk.com/groups.members", { group: group });
    const answer = body(zdGet("groups/" + group + "/users.json?" + pageQuery(window)));
    const items: User[] = [];
    for (const raw of arrayAt(answer, "users")) items.push(userFrom(raw));
    return { items: items, nextCursor: afterCursor(answer) };
}

// ---------------------------------------------------------------------------
// Views and macros
// ---------------------------------------------------------------------------

/**
 * The views the token's user can use, such as "Unassigned tickets".
 * @param page The page.
 * @returns One page of active views.
 * @capability zendesk.com/views.list {}
 */
export function listViews(page: PageOptions = {}): Page<View> {
    const { pageSize, cursor } = page;
    const window = pageWindow({ pageSize: pageSize, cursor: cursor });
    check("zendesk.com/views.list", {});
    const answer = body(zdGet("views.json?active=true&" + pageQuery(window)));
    const items: View[] = [];
    for (const raw of arrayAt(answer, "views")) items.push(viewFrom(raw));
    return { items: items, nextCursor: afterCursor(answer) };
}

/**
 * A view's tickets, in the view's order. Each ticket is also checked as
 * `getTicket` is, so a rule that limits which tickets may be read holds through
 * a view.
 * @param viewId View id.
 * @param page The page.
 * @returns One page of tickets.
 * @capability zendesk.com/views.read { view: string }
 * @capability zendesk.com/tickets.get { group: string, organization: string, brand: string, assignee: string, requester: string, status: string }
 */
export function viewTickets(viewId: string, page: PageOptions = {}): Page<Ticket> {
    const view = normalizeId(viewId, "viewId");
    const { pageSize, cursor } = page;
    const window = pageWindow({ pageSize: pageSize, cursor: cursor });
    check("zendesk.com/views.read", { view: view });
    const answer = body(zdGet("views/" + view + "/tickets.json?" + pageQuery(window)));
    const items: Ticket[] = [];
    for (const raw of arrayAt(answer, "tickets")) {
        const facts = factsOf(raw as Record<string, unknown>);
        check("zendesk.com/tickets.get", {
            group: facts.group, organization: facts.organization, brand: facts.brand,
            assignee: facts.assignee, requester: facts.requester, status: facts.status,
        });
        items.push(ticketFrom(raw));
    }
    return { items: items, nextCursor: afterCursor(answer) };
}

/**
 * How many tickets a view holds. Zendesk caches the count; it may lag.
 * @param viewId View id.
 * @returns The count, or null when Zendesk is still computing it.
 * @capability zendesk.com/views.read { view: string }
 */
export function countView(viewId: string): number | null {
    const view = normalizeId(viewId, "viewId");
    check("zendesk.com/views.read", { view: view });
    const answer = body(zdGet("views/" + view + "/count.json"));
    return numberAt(objectAt(answer, "view_count"), "value");
}

/**
 * The macros the token's user can apply.
 * @param page The page.
 * @returns One page of active macros.
 * @capability zendesk.com/macros.list {}
 */
export function listMacros(page: PageOptions = {}): Page<Macro> {
    const { pageSize, cursor } = page;
    const window = pageWindow({ pageSize: pageSize, cursor: cursor });
    check("zendesk.com/macros.list", {});
    const answer = body(zdGet("macros/active.json?" + pageQuery(window)));
    const items: Macro[] = [];
    for (const raw of arrayAt(answer, "macros")) items.push(macroFrom(raw));
    return { items: items, nextCursor: afterCursor(answer) };
}

/**
 * What a macro would change on a ticket, without changing it.
 * @param ticketId Ticket id.
 * @param macroId Macro id.
 * @returns The changes and the comment it would add.
 * @capability zendesk.com/macros.preview { macro: string, group: string, organization: string, brand: string, assignee: string, requester: string, status: string }
 */
export function previewMacro(ticketId: string, macroId: string): MacroPreview {
    const ticket = normalizeId(ticketId, "ticketId");
    const macro = normalizeId(macroId, "macroId");
    const facts = factsOf(readTicket(ticket));
    check("zendesk.com/macros.preview", {
        macro: macro, group: facts.group, organization: facts.organization, brand: facts.brand,
        assignee: facts.assignee, requester: facts.requester, status: facts.status,
    });
    return macroPreviewFrom(body(zdGet("tickets/" + ticket + "/macros/" + macro + "/apply.json")));
}

/**
 * Apply a macro to a ticket and save the result. The check names the fields the
 * macro changes (`group` and `assignee` when it moves the ticket); a comment it
 * adds is also checked as a reply or a note.
 * @param ticketId Ticket id.
 * @param macroId Macro id.
 * @returns The ticket as changed.
 * @capability zendesk.com/macros.apply { macro: string, group: string, organization: string, brand: string, assignee: string, requester: string, status: string, fields: string[] }
 * @capability zendesk.com/comments.reply { group: string, organization: string, brand: string, assignee: string, requester: string, status: string, previousStatus: string }
 * @capability zendesk.com/comments.note { group: string, organization: string, brand: string, assignee: string, requester: string, status: string }
 */
export function applyMacro(ticketId: string, macroId: string): Ticket {
    const ticket = normalizeId(ticketId, "ticketId");
    const macro = normalizeId(macroId, "macroId");
    const raw = readTicket(ticket);
    if (raw === null) {
        check("zendesk.com/macros.apply", {
            macro: macro, group: "", organization: "", brand: "", assignee: "", requester: "", status: "", fields: [],
        });
        throw new ZendeskError("RecordNotFound", "Zendesk ticket " + ticket + " was not found", [], 404, false, null);
    }
    const facts = factsOf(raw);
    const preview = objectAt(body(zdGet("tickets/" + ticket + "/macros/" + macro + "/apply.json")), "result");
    const changes = macroChanges(raw, objectAt(preview, "ticket"));
    const added = macroPreviewFrom({ result: preview }).comment;
    check("zendesk.com/macros.apply", {
        macro: macro, group: facts.group, organization: facts.organization, brand: facts.brand,
        assignee: facts.assignee, requester: facts.requester, status: facts.status, fields: changes.fields,
    });
    const newStatus = textAt(changes.body, "status") ?? facts.status;
    if (added !== null && added.public) {
        check("zendesk.com/comments.reply", {
            group: facts.group, organization: facts.organization, brand: facts.brand,
            assignee: facts.assignee, requester: facts.requester, status: newStatus, previousStatus: facts.status,
        });
    }
    if (added !== null && !added.public) {
        check("zendesk.com/comments.note", {
            group: facts.group, organization: facts.organization, brand: facts.brand,
            assignee: facts.assignee, requester: facts.requester, status: facts.status,
        });
    }
    const write = changes.body;
    if (added !== null) {
        const comment: Record<string, unknown> = { public: added.public };
        if (added.htmlBody.length > 0) comment["html_body"] = added.htmlBody;
        else comment["body"] = added.body;
        write["comment"] = comment;
    }
    if (Object.keys(write).length === 0) return ticketFrom(raw);
    return writeTicket(ticket, raw, write);
}

/**
 * The account's ticket fields, standard and custom, with drop-down options.
 * @returns Every ticket field.
 * @capability zendesk.com/metadata.read { what: string }
 */
export function listTicketFields(): TicketField[] {
    check("zendesk.com/metadata.read", { what: "ticket_fields" });
    const answer = body(zdGet("ticket_fields.json"));
    const items: TicketField[] = [];
    for (const raw of arrayAt(answer, "ticket_fields")) items.push(ticketFieldFrom(raw));
    return items;
}

/**
 * The account's brands.
 * @returns Every brand.
 * @capability zendesk.com/metadata.read { what: string }
 */
export function listBrands(): Brand[] {
    check("zendesk.com/metadata.read", { what: "brands" });
    const answer = body(zdGet("brands.json"));
    const items: Brand[] = [];
    for (const raw of arrayAt(answer, "brands")) items.push(brandFrom(raw));
    return items;
}

// ---------------------------------------------------------------------------
// Help Center
// ---------------------------------------------------------------------------

/**
 * Search Help Center articles, best match first.
 * @param query Words to find.
 * @param options Locale, section and page.
 * @returns One page of articles with matching excerpts.
 * @capability zendesk.com/articles.search { query: string, locale: string, section: string }
 */
export function searchArticles(query: string, options: ArticleSearchOptions = {}): SearchPage<ArticleHit> {
    const text = requireText(query, "query");
    const { locale, sectionId, pageSize, cursor } = options;
    const window = searchWindow({ pageSize: pageSize, cursor: cursor });
    const language = locale === undefined ? "" : localeOf(locale);
    const section = optionalId(sectionId, "sectionId");
    check("zendesk.com/articles.search", { query: text, locale: language, section: section });
    let path = "help_center/articles/search.json?query=" + encodeComponent(text) + numberedPage(window);
    if (language.length > 0) path = path + "&locale=" + language;
    if (section.length > 0) path = path + "&section=" + section;
    const answer = body(zdGet(path));
    const items: ArticleHit[] = [];
    for (const raw of arrayAt(answer, "results")) items.push(articleHitFrom(raw));
    return searchPageOf(answer, items, window);
}

/**
 * Get a Help Center article; null when it does not exist.
 * @param id Article id.
 * @param options The locale to read.
 * @returns The article, or null.
 * @capability zendesk.com/articles.get { section: string, locale: string }
 */
export function getArticle(id: string, options: LocaleOptions = {}): Article | null {
    const articleId = normalizeId(id, "id");
    const { locale } = options;
    const language = locale === undefined ? "" : localeOf(locale);
    const prefix = language.length > 0 ? "help_center/" + language + "/articles/" : "help_center/articles/";
    const answer = zdFind(prefix + articleId + ".json");
    const raw = answer === null ? null : objectAt(body(answer), "article");
    check("zendesk.com/articles.get", { section: idAt(raw, "section_id") ?? "", locale: textAt(raw, "locale") ?? language });
    return raw === null ? null : articleFrom(raw);
}

/**
 * The Help Center's sections.
 * @param options A category and a locale.
 * @returns Every section, up to 100.
 * @capability zendesk.com/articles.browse {}
 */
export function listSections(options: SectionOptions = {}): Section[] {
    const { locale, categoryId } = options;
    const language = locale === undefined ? "" : localeOf(locale);
    const category = optionalId(categoryId, "categoryId");
    check("zendesk.com/articles.browse", {});
    const scope = language.length > 0 ? "help_center/" + language + "/" : "help_center/";
    const path = category.length > 0 ? scope + "categories/" + category + "/sections.json" : scope + "sections.json";
    const answer = body(zdGet(path + "?per_page=100"));
    const items: Section[] = [];
    for (const raw of arrayAt(answer, "sections")) items.push(sectionFrom(raw));
    return items;
}

/**
 * The Help Center's categories.
 * @param options A locale.
 * @returns Every category, up to 100.
 * @capability zendesk.com/articles.browse {}
 */
export function listCategories(options: LocaleOptions = {}): Category[] {
    const { locale } = options;
    const language = locale === undefined ? "" : localeOf(locale);
    check("zendesk.com/articles.browse", {});
    const scope = language.length > 0 ? "help_center/" + language + "/" : "help_center/";
    const answer = body(zdGet(scope + "categories.json?per_page=100"));
    const items: Category[] = [];
    for (const raw of arrayAt(answer, "categories")) items.push(categoryFrom(raw));
    return items;
}

// ---------------------------------------------------------------------------
// Reporting and raw search
// ---------------------------------------------------------------------------

/**
 * Customers' satisfaction ratings, newest first.
 * @param options A score and the page.
 * @returns One page of ratings.
 * @capability zendesk.com/ratings.list { score: string }
 */
export function listSatisfactionRatings(options: RatingOptions = {}): Page<SatisfactionRating> {
    const { score, pageSize, cursor } = options;
    const window = pageWindow({ pageSize: pageSize, cursor: cursor });
    const wanted = score ?? "";
    if (wanted.length > 0 && !["good", "bad", "offered", "unoffered", "received"].includes(wanted)) {
        throw validationError("invalid_input", "score must be good, bad, offered, unoffered or received");
    }
    check("zendesk.com/ratings.list", { score: wanted });
    let path = "satisfaction_ratings.json?" + pageQuery(window) + "&sort=-created_at";
    if (wanted.length > 0) path = path + "&score=" + wanted;
    const answer = body(zdGet(path));
    const items: SatisfactionRating[] = [];
    for (const raw of arrayAt(answer, "satisfaction_ratings")) items.push(ratingFrom(raw));
    return { items: items, nextCursor: afterCursor(answer) };
}

/**
 * Search with Zendesk's query language, such as `type:ticket status<solved
 * priority:urgent`. Results are Zendesk's objects, untyped, with
 * `result_type`. Prefer `findTickets`: a raw query's scope cannot be checked.
 * @param query The query.
 * @param options A type and the page; search reads at most 1,000 results.
 * @returns One page of results.
 * @capability zendesk.com/search.raw { query: string, type: string }
 */
export function search(query: string, options: SearchOptions = {}): SearchPage<Record<string, unknown>> {
    const text = requireText(query, "query");
    const { type, pageSize, cursor } = options;
    const window = searchWindow({ pageSize: pageSize, cursor: cursor });
    const kind = type ?? "";
    if (kind.length > 0 && !["ticket", "user", "organization", "group"].includes(kind)) {
        throw validationError("invalid_input", "type must be ticket, user, organization or group");
    }
    check("zendesk.com/search.raw", { query: text, type: kind });
    const full = kind.length > 0 ? "type:" + kind + " " + text : text;
    const answer = searchGet(full, window, "");
    const items: Record<string, unknown>[] = [];
    for (const raw of arrayAt(answer, "results")) {
        if (raw !== null && typeof raw === "object" && !Array.isArray(raw)) items.push(raw as Record<string, unknown>);
    }
    return searchPageOf(answer, items, window);
}

// ---------------------------------------------------------------------------
// Internal
// ---------------------------------------------------------------------------

// What a check knows about a ticket, read from Zendesk; "" for a ticket that
// does not exist and for fields it leaves empty.
interface TicketFacts {
    group: string;
    organization: string;
    brand: string;
    assignee: string;
    requester: string;
    status: string;
}

function factsOf(raw: Record<string, unknown> | null): TicketFacts {
    return {
        group: idAt(raw, "group_id") ?? "",
        organization: idAt(raw, "organization_id") ?? "",
        brand: idAt(raw, "brand_id") ?? "",
        assignee: idAt(raw, "assignee_id") ?? "",
        requester: idAt(raw, "requester_id") ?? "",
        status: textAt(raw, "status") ?? "",
    };
}

function readTicket(id: string): Record<string, unknown> | null {
    const answer = zdFind("tickets/" + id + ".json");
    return answer === null ? null : objectAt(body(answer), "ticket");
}

// Writes a ticket's changes, refused by Zendesk if the ticket changed after
// `raw` was read for the check. A ticket that did not exist was checked with
// empty facts and is reported missing.
function writeTicket(id: string, raw: Record<string, unknown> | null, ticket: Record<string, unknown>): Ticket {
    if (raw === null) throw new ZendeskError("RecordNotFound", "Zendesk ticket " + id + " was not found", [], 404, false, null);
    ticket["safe_update"] = true;
    ticket["updated_stamp"] = textAt(raw, "updated_at") ?? "";
    return ticketFrom(objectAt(body(zdPut("tickets/" + id + ".json", { ticket: ticket })), "ticket"));
}

function commentBody(text: string, html: boolean, visible: boolean, tokens: string[]): Record<string, unknown> {
    const comment: Record<string, unknown> = { public: visible };
    if (html) comment["html_body"] = text;
    else comment["body"] = text;
    if (tokens.length > 0) comment["uploads"] = tokens;
    return comment;
}

// Custom field writes, validated; adds `custom.<id>` to `fields`.
function customWrites(custom: CustomFieldWrite[], fields: string[]): Record<string, unknown>[] {
    const out: Record<string, unknown>[] = [];
    const seen: string[] = [];
    for (const write of custom) {
        const id = normalizeId(write.id, "customFields");
        if (seen.includes(id)) throw validationError("invalid_input", "customFields names field " + id + " twice");
        seen.push(id);
        out.push({ id: Number(id), value: customValue(write.value, "customFields") });
        fields.push("custom." + id);
    }
    return out;
}

// The writable ticket fields a macro changes, and their names for the check.
interface MacroChanges {
    body: Record<string, unknown>;
    fields: string[];
}

const MACRO_FIELDS: string[][] = [
    ["subject", "subject"], ["status", "status"], ["priority", "priority"], ["type", "type"], ["tags", "tags"],
    ["group_id", "group"], ["assignee_id", "assignee"], ["due_at", "due_at"], ["problem_id", "problem"],
];

// What a macro's preview may change besides MACRO_FIELDS and custom fields:
// the comment, and values Zendesk derives from what is written.
const MACRO_DERIVED: string[] = ["comment", "custom_fields", "fields", "raw_subject", "updated_at", "generated_timestamp", "custom_status_id"];

function macroChanges(current: Record<string, unknown>, after: Record<string, unknown>): MacroChanges {
    const unsupported: string[] = [];
    for (const key of Object.keys(after)) {
        if (MACRO_DERIVED.includes(key) || MACRO_FIELDS.some((pair) => pair[0] === key)) continue;
        if (JSON.stringify(after[key] ?? null) !== JSON.stringify(current[key] ?? null)) unsupported.push(key);
    }
    if (unsupported.length > 0) {
        throw validationError("unsupported_macro", "the macro also changes " + sortedDistinct(unsupported).join(", ") + ", which applyMacro cannot write; apply it in Zendesk");
    }
    const changed: Record<string, unknown> = {};
    const fields: string[] = [];
    for (const pair of MACRO_FIELDS) {
        const key = pair[0];
        if (!(key in after)) continue;
        if (JSON.stringify(after[key] ?? null) === JSON.stringify(current[key] ?? null)) continue;
        changed[key] = after[key];
        fields.push(pair[1]);
    }
    const before: Record<string, string> = {};
    for (const field of arrayAt(current, "custom_fields")) {
        const id = idAt(field, "id");
        if (id !== null) before[id] = JSON.stringify((field as Record<string, unknown>)["value"] ?? null);
    }
    const custom: Record<string, unknown>[] = [];
    for (const field of arrayAt(after, "custom_fields")) {
        const id = idAt(field, "id");
        if (id === null) continue;
        const value = (field as Record<string, unknown>)["value"] ?? null;
        if (JSON.stringify(value) === (before[id] ?? "null")) continue;
        custom.push({ id: Number(id), value: value });
        fields.push("custom." + id);
    }
    if (custom.length > 0) changed["custom_fields"] = custom;
    return { body: changed, fields: sortedDistinct(fields) };
}

function ticketTerms(
    statuses: string[], group: string, assignee: string, requester: string, organization: string, brand: string, tag: string,
): string[] {
    const terms: string[] = ["type:ticket"];
    for (const status of statuses) terms.push("status:" + status);
    if (group.length > 0) terms.push("group:" + group);
    if (assignee.length > 0) terms.push("assignee:" + assignee);
    if (requester.length > 0) terms.push("requester:" + requester);
    if (organization.length > 0) terms.push("organization:" + organization);
    if (brand.length > 0) terms.push("brand:" + brand);
    if (tag.length > 0) terms.push("tags:" + tag);
    return terms;
}

function dateTerm(terms: string[], keyword: string, value: string | undefined, argument: string): void {
    if (value !== undefined) terms.push(keyword + requireDate(value, argument));
}

function localeOf(value: string): string {
    const locale = value.trim().toLowerCase();
    let ok = locale.length >= 2 && locale.length <= 10;
    for (let i = 0; i < locale.length; i += 1) {
        const code = locale.charCodeAt(i);
        if (!((code >= 97 && code <= 122) || code === 45)) ok = false;
    }
    if (!ok) throw validationError("invalid_input", "locale must be such as en-us, got " + JSON.stringify(value));
    return locale;
}

function numbers(ids: string[]): number[] {
    const out: number[] = [];
    for (const id of ids) out.push(Number(id));
    return out;
}

function searchGet(query: string, window: SearchWindow, sort: string): unknown {
    return body(zdGet("search.json?query=" + encodeComponent(query) + sort + numberedPage(window)));
}

function numberedPage(window: SearchWindow): string {
    return "&per_page=" + window.pageSize.toString() + "&page=" + window.page.toString();
}

// A numbered page's next cursor: the next page's number, while Zendesk has one
// and it starts within the first 1,000 results.
function searchPageOf<T>(answer: unknown, items: T[], window: SearchWindow): SearchPage<T> {
    const count = numberAt(answer, "count") ?? items.length;
    const more = textAt(answer, "next_page") !== null && window.page * window.pageSize < 1000;
    return { items: items, count: count, nextCursor: more ? (window.page + 1).toString() : null };
}

function body(response: Response): unknown {
    if (response.body.length === 0) return null;
    try {
        return JSON.parse(response.body);
    } catch (error) {
        throw new ZendeskError("invalid_response", "Zendesk answered with something other than JSON", [], response.status, false, null);
    }
}

// One call site per verb, each under the account's URL: `ACCOUNT` and then a
// path without a leading slash, so the compiler derives the account's host into
// the grants.
function zdGet(path: string): Response {
    return requireOk(get(ACCOUNT + "api/v2/" + path, headers()));
}

function zdPost(path: string, payload: Record<string, unknown>): Response {
    return requireOk(post(ACCOUNT + "api/v2/" + path, payload, headers()));
}

function zdPut(path: string, payload: Record<string, unknown>): Response {
    return requireOk(put(ACCOUNT + "api/v2/" + path, payload, headers()));
}

// A successful answer, or null for 404; any other error throws.
function zdFind(path: string): Response | null {
    const response = get(ACCOUNT + "api/v2/" + path, headers());
    if (response.ok) return response;
    if (response.status === 404) return null;
    throw errorOf(response);
}

function headers(): Map<string, string> {
    const map = new Map<string, string>();
    map.set("Authorization", "Bearer " + accessToken());
    map.set("Accept", "application/json");
    map.set("User-Agent", USER_AGENT);
    return map;
}

function requireOk(response: Response): Response {
    if (response.ok) return response;
    throw errorOf(response);
}

function errorOf(response: Response): ZendeskError {
    const failure = failureFrom(response.status, response.statusText, response.body);
    const wait = retryAfterSeconds(response.headers.get("retry-after") ?? "");
    return new ZendeskError(failure.errorCode, failure.message, failure.details, response.status, failure.retryable, wait);
}
