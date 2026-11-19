// Collaboration: activities, notes, files, Chatter and email. The input and
// result types, and the request bodies and response mappers the operations in
// `src/lib.ts` use. Internal: `src/lib.ts` re-exports the types.

import { validationError } from "./errors";
import { FieldWrite } from "./request";
import { arrayAt, flagAt, numberAt, objectAt, textAt } from "./response";

/** What kind of activity `logActivity` records. */
export type ActivityType = "call" | "email" | "task";

/** A completed activity to log, such as a call. */
export interface ActivityInput {
    /**
     * The task's subtype (`TaskSubtype`): `Call`, `Email` or `Task`. A meeting is
     * an event: `createEvent` takes past times too. The task's `Type` picklist is
     * not set, since orgs hide it by default; set it through `custom` where shown.
     */
    type: ActivityType;
    /** One line, such as `Call with Dana about renewal`. */
    subject: string;
    /** The record it is about: an account, opportunity, case or other (`WhatId`). */
    whatId?: string;
    /** The person: a contact or lead (`WhoId`). */
    whoId?: string;
    /** Notes (`Description`). */
    description?: string;
    /** When it happened, `YYYY-MM-DD` (`ActivityDate`); default none. */
    date?: string;
    /** Other fields by API name. */
    custom?: FieldWrite[];
}

/** A calendar event to create. */
export interface EventInput {
    /** One line (`Subject`). */
    subject: string;
    /** Start, a date-time with a zone (`StartDateTime`). */
    start: string;
    /** End, a date-time with a zone (`EndDateTime`). */
    end: string;
    /** The record it is about (`WhatId`). */
    whatId?: string;
    /** The person: a contact or lead (`WhoId`). */
    whoId?: string;
    /** Where (`Location`). */
    location?: string;
    /** Notes (`Description`). */
    description?: string;
    /** Other fields by API name. */
    custom?: FieldWrite[];
}

/** Options of `listActivities`. */
export interface ActivityOptions {
    /** True for open activities (upcoming tasks and events); default the activity history. */
    open?: boolean;
    /** At most this many, 1 to 200; default 50. */
    limit?: number;
}

/** A task or event in a record's activity history or open activities. */
export interface Activity {
    /** Task or event id. */
    id: string;
    /** True for a task, false for an event. */
    isTask: boolean;
    /** `Subject`. */
    subject: string | null;
    /** `ActivityDate`, `YYYY-MM-DD`. */
    activityDate: string | null;
    /** `ActivitySubtype`, such as `Call`, `Email` or `Event`. */
    subtype: string | null;
    /** A task's `Status`. */
    status: string | null;
    /** A task's `Priority`. */
    priority: string | null;
    /** `IsClosed`. */
    isClosed: boolean;
    /** `OwnerId`. */
    ownerId: string | null;
    /** `WhoId`: the contact or lead. */
    whoId: string | null;
    /** `WhatId`: the record it is about. */
    whatId: string | null;
    /** An event's `StartDateTime`. */
    start: string | null;
    /** An event's `EndDateTime`. */
    end: string | null;
    /** An event's `Location`. */
    location: string | null;
    /** `Description`. */
    description: string | null;
}

/** Options of `uploadFile`. */
export interface UploadOptions {
    /** The file's title in Salesforce; default its file name. */
    title?: string;
}

/** A file uploaded to Salesforce. */
export interface UploadedFile {
    /** The file (`ContentDocument`) id, for `downloadFile`. */
    contentDocumentId: string;
    /** The uploaded version's (`ContentVersion`) id. */
    contentVersionId: string;
}

/** Options of `downloadFile`. */
export interface FileDownloadOptions {
    /** Replace a file already at the path; default false. */
    overwrite?: boolean;
}

/** A file downloaded from Salesforce. */
export interface DownloadedFile {
    /** Where it was written. */
    path: string;
    /** Its size in bytes. */
    bytes: number;
    /** Its title in Salesforce. */
    title: string;
    /** Its extension, such as `pdf`. */
    fileExtension: string | null;
}

/** Options of `postToFeed`. */
export interface PostOptions {
    /** Users to mention (`@`), by user id; each is notified. */
    mentions?: string[];
}

/** Options of `readFeed`. */
export interface FeedOptions {
    /** Posts per page, 1 to 100; default 25. */
    pageSize?: number;
    /** A page's `nextCursor`, for the next page. */
    cursor?: string;
}

/** A Chatter post. */
export interface FeedPost {
    /** Feed element id. */
    id: string;
    /** Such as `TextPost`, `TrackedChange` or `ContentPost`. */
    type: string;
    /** When it was posted. */
    createdDate: string;
    /** Who posted it. */
    authorId: string | null;
    /** Their name. */
    authorName: string | null;
    /** The post's text, mentions included. */
    text: string;
    /** How many comments it has. */
    commentCount: number;
    /** How many likes it has. */
    likeCount: number;
}

/** A page of Chatter posts, newest first. */
export interface FeedPage {
    /** This page's posts. */
    posts: FeedPost[];
    /** Pass back as `cursor` for the next page; null on the last page. */
    nextCursor: string | null;
}

/** An email to send as the token's user. */
export interface EmailInput {
    /** Recipients' addresses, 1 to 5. */
    to: string[];
    /** Subject line. */
    subject: string;
    /** Plain-text body. */
    body: string;
    /** A record to log the email on as a task: an account, opportunity, case or other record a task can be on, not a contact, lead or user. */
    relatedTo?: string;
}

const ACTIVITY_FIELDS =
    "Id, IsTask, Subject, ActivityDate, ActivitySubtype, Status, Priority, IsClosed, OwnerId, WhoId, WhatId, StartDateTime, EndDateTime, Location, Description";
const MAX_EMAIL_RECIPIENTS = 5;
const MIN_FEED_PAGE = 1;
const MAX_FEED_PAGE = 100;
const DEFAULT_FEED_PAGE = 25;
const MAX_FEED_CURSOR = 512;

// The typed fields `logActivity` sets, by its type.
export function activityFields(type: string): FieldWrite[] {
    if (type === "call") return [{ field: "TaskSubtype", value: "Call" }];
    if (type === "email") return [{ field: "TaskSubtype", value: "Email" }];
    if (type === "task") return [{ field: "TaskSubtype", value: "Task" }];
    throw validationError("invalid_input", "type must be call, email or task, got " + JSON.stringify(type));
}

// The SOQL that reads a record's activity history or open activities.
export function activitySoql(object: string, id: string, open: boolean, limit: number): string {
    const relationship = open ? "OpenActivities" : "ActivityHistories";
    // Salesforce requires these sort orders and a limit for users without "View All Data".
    const order = open ? "ActivityDate ASC NULLS LAST, LastModifiedDate DESC" : "ActivityDate DESC, LastModifiedDate DESC";
    return "SELECT Id, (SELECT " + ACTIVITY_FIELDS + " FROM " + relationship + " ORDER BY " + order + " LIMIT " + limit.toString() +
        ") FROM " + object + " WHERE Id = '" + id + "'";
}

// `listActivities`' limit, 1 to 200.
export function activityLimit(limit: number | undefined): number {
    const value = limit ?? 50;
    if (!Number.isInteger(value) || value < 1 || value > 200) {
        throw validationError("invalid_input", "limit must be an integer from 1 to 200, got " + value.toString());
    }
    return value;
}

/**
 * The activities in a relationship subquery's result.
 * @param parent The parent record as Salesforce returned it.
 * @param open True for `OpenActivities`.
 * @returns The activities.
 */
export function activitiesFrom(parent: Record<string, unknown>, open: boolean): Activity[] {
    const nested = objectAt(parent, open ? "OpenActivities" : "ActivityHistories");
    const activities: Activity[] = [];
    for (const item of arrayAt(nested, "records")) activities.push(activityFrom(item));
    return activities;
}

function activityFrom(raw: unknown): Activity {
    return {
        id: textAt(raw, "Id") ?? "",
        isTask: flagAt(raw, "IsTask"),
        subject: textAt(raw, "Subject"),
        activityDate: textAt(raw, "ActivityDate"),
        subtype: textAt(raw, "ActivitySubtype"),
        status: textAt(raw, "Status"),
        priority: textAt(raw, "Priority"),
        isClosed: flagAt(raw, "IsClosed"),
        ownerId: textAt(raw, "OwnerId"),
        whoId: textAt(raw, "WhoId"),
        whatId: textAt(raw, "WhatId"),
        start: textAt(raw, "StartDateTime"),
        end: textAt(raw, "EndDateTime"),
        location: textAt(raw, "Location"),
        description: textAt(raw, "Description"),
    };
}

// A note's `Content`: the text as HTML paragraphs, base64-encoded as ContentNote requires.
export function noteContent(body: string): string {
    let html = "";
    for (const line of body.split("\n")) html += "<p>" + escapeHtml(line.endsWith("\r") ? line.slice(0, -1) : line) + "</p>";
    return new TextEncoder().encode(html).toBase64();
}

export function escapeHtml(text: string): string {
    let out = "";
    for (let i = 0; i < text.length; i += 1) {
        const char = text.charAt(i);
        if (char === "&") out += "&amp;";
        else if (char === "<") out += "&lt;";
        else if (char === ">") out += "&gt;";
        else if (char === "\"") out += "&quot;";
        else if (char === "'") out += "&#39;";
        else out += char;
    }
    return out;
}

// The last segment of a VFS path.
export function fileName(path: string): string {
    const slash = path.lastIndexOf("/");
    return slash >= 0 ? path.slice(slash + 1) : path;
}

// A Chatter post's body: the text, then each mention.
export function feedBody(text: string, mentions: string[]): Record<string, unknown> {
    const segments: Record<string, unknown>[] = [{ type: "Text", text: text }];
    for (const id of mentions) {
        segments.push({ type: "Text", text: " " });
        segments.push({ type: "Mention", id: id });
    }
    return { messageSegments: segments };
}

// `readFeed`'s page size, 1 to 100.
export function feedPageSize(size: number | undefined): number {
    const value = size ?? DEFAULT_FEED_PAGE;
    if (!Number.isInteger(value) || value < MIN_FEED_PAGE || value > MAX_FEED_PAGE) {
        throw validationError("invalid_input", "pageSize must be an integer from 1 to 100, got " + value.toString());
    }
    return value;
}

// A Chatter page token, such as `2026-11-18T10:00:00Z,0D5000000000001AAA`: letters,
// digits and `-:.,_+` only, so it cannot change the request's path or query.
export function feedCursor(cursor: string | undefined): string {
    if (cursor === undefined) return "";
    const text = cursor.trim();
    let ok = text.length > 0 && text.length <= MAX_FEED_CURSOR;
    for (let i = 0; ok && i < text.length; i += 1) {
        const char = text.charAt(i);
        const code = text.charCodeAt(i);
        const alnum = (code >= 65 && code <= 90) || (code >= 97 && code <= 122) || (code >= 48 && code <= 57);
        ok = alnum || char === "-" || char === ":" || char === "." || char === "," || char === "_" || char === "+";
    }
    if (!ok) throw validationError("invalid_cursor", "cursor must be a page's nextCursor, got " + JSON.stringify(cursor));
    return text;
}

/**
 * A page of a record feed.
 * @param page Connect's feed element page.
 * @returns The posts and the next page's cursor.
 */
export function feedPageFrom(page: Record<string, unknown>): FeedPage {
    const posts: FeedPost[] = [];
    for (const element of arrayAt(page, "elements")) {
        const actor = objectAt(element, "actor");
        const capabilities = objectAt(element, "capabilities");
        const comments = objectAt(objectAt(capabilities, "comments"), "page");
        const likes = objectAt(objectAt(capabilities, "chatterLikes"), "page");
        posts.push({
            id: textAt(element, "id") ?? "",
            type: textAt(element, "type") ?? "",
            createdDate: textAt(element, "createdDate") ?? "",
            authorId: textAt(actor, "id"),
            authorName: textAt(actor, "displayName") ?? textAt(actor, "name"),
            text: textAt(objectAt(element, "body"), "text") ?? "",
            commentCount: numberAt(comments, "total") ?? 0,
            likeCount: numberAt(likes, "total") ?? 0,
        });
    }
    const token = textAt(page, "nextPageToken");
    return { posts: posts, nextCursor: token !== null && token.length > 0 ? token : null };
}

/**
 * Validate a bare email address, `name@domain.tld`, and lowercase it.
 * @param value The address as given.
 * @returns The address, lowercased.
 */
export function emailAddress(value: string): string {
    const address = value.trim().toLowerCase();
    const at = address.indexOf("@");
    const refused = validationError("invalid_recipient", "to must hold bare addresses such as dana@example.com, got " + JSON.stringify(value));
    if (at < 1 || at !== address.lastIndexOf("@") || address.length > 254) throw refused;
    const domain = address.slice(at + 1);
    if (domain.indexOf(".") < 1 || domain.endsWith(".") || domain.indexOf("..") >= 0) throw refused;
    for (let i = 0; i < address.length; i += 1) {
        const char = address.charAt(i);
        const code = address.charCodeAt(i);
        if (code <= 32 || code >= 127 || char === "," || char === ";" || char === "<" || char === ">" || char === "\"" || char === "(" || char === ")") {
            throw refused;
        }
    }
    return address;
}

// The recipients, validated, lowercased and distinct, in the order given.
export function recipients(addresses: string[]): string[] {
    const out: string[] = [];
    for (const address of addresses) {
        const normalized = emailAddress(address);
        if (!out.includes(normalized)) out.push(normalized);
    }
    if (out.length < 1 || out.length > MAX_EMAIL_RECIPIENTS) {
        throw validationError("invalid_recipient", "to must hold 1 to 5 addresses, got " + out.length.toString());
    }
    return out;
}

// The domain of a validated address.
export function domainOf(address: string): string {
    return address.slice(address.indexOf("@") + 1);
}
