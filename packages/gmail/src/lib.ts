import { get, post, delete, Response } from "submilli:http";
import { decodeComponent, encodeComponent, encodeQuery } from "submilli:url";
import { read, stat, write } from "submilli:fs";
import secrets from "submilli:secrets";
import { check } from "submilli:security";

const API = "https://gmail.googleapis.com/gmail/v1/users/me";
const MAX_BODY_BYTES = 1048576;
const MAX_ATTACHMENT_BYTES = 10485760;

/** A Gmail API or message-construction error with stable fields. */
export class GmailError extends Error {
    code: string;
    status: number;

    constructor(code: string, message: string, status: number) {
        super(message);
        this.name = "GmailError";
        this.code = code;
        this.status = status;
    }
}

/** Token pagination for Gmail list methods. */
export interface PageOptions {
    /** Maximum items per page; clamped to the endpoint's allowed range. */
    limit?: number;
    /** Continuation token from a previous page's `nextPageToken`. */
    pageToken?: string;
}

/** Search options for Gmail threads. */
export interface SearchOptions {
    /** Maximum threads per page; clamped to 1-100, default 20. */
    limit?: number;
    /** Continuation token from a previous page's `nextPageToken`. */
    pageToken?: string;
    /** Include threads from Spam and Trash in the results. */
    includeSpamTrash?: boolean;
}

/** One page of values and an explicit continuation token. */
export interface Page<T> {
    /** Values on this page. */
    items: T[];
    /** Token for the next page; empty string when there are no more pages. */
    nextPageToken: string;
}

/** Authenticated Gmail account profile. */
export interface Profile {
    /** The account's email address. */
    emailAddress: string;
    /** Total number of messages in the mailbox. */
    messagesTotal: number;
    /** Total number of threads in the mailbox. */
    threadsTotal: number;
    /** Current mailbox history ID (opaque numeric string). */
    historyId: string;
}

/** A decoded RFC 5322 header. */
export interface Header {
    /** Header name, e.g. "From", "Subject". */
    name: string;
    /** Header value with RFC 2047 encoded-words already decoded. */
    value: string;
}

/** Metadata for one MIME attachment. */
export interface Attachment {
    /** MIME part ID within the message payload. */
    partId: string;
    /** Attachment body ID; pass to `downloadAttachment`. May be empty for inline parts. */
    attachmentId: string;
    /** Original filename; may be empty. */
    filename: string;
    /** MIME type, lowercased, e.g. "application/pdf". */
    mimeType: string;
    /** Attachment body size in bytes. */
    size: number;
}

/** Normalized Gmail message with decoded text/HTML bodies and attachment metadata. */
export interface Message {
    /** Immutable message ID. */
    id: string;
    /** ID of the thread the message belongs to. */
    threadId: string;
    /** IDs of labels applied to the message, e.g. "INBOX", "UNREAD". */
    labelIds: string[];
    /** Short plain-text excerpt of the message body. */
    snippet: string;
    /** Internal receive time as epoch milliseconds in a string; empty if unknown. */
    internalDate: string;
    /** Decoded RFC 5322 headers from the message payload. */
    headers: Header[];
    /** Concatenated decoded text/plain body parts; empty if none. */
    text: string;
    /** Concatenated decoded text/html body parts; empty if none. */
    html: string;
    /** Metadata for the message's attachments. */
    attachments: Attachment[];
}

/** A thread and its messages. */
export interface Thread {
    /** Immutable thread ID. */
    id: string;
    /** Mailbox history ID of the thread's last change (opaque numeric string). */
    historyId: string;
    /** Messages in the thread, oldest first. */
    messages: Message[];
}

/** A lightweight thread search result. */
export interface ThreadRef {
    /** Thread ID; pass to `getThread` for the full thread. */
    id: string;
    /** Short plain-text excerpt of the thread; empty if not provided. */
    snippet: string;
    /** Mailbox history ID of the thread's last change; empty if not provided. */
    historyId: string;
}

/** A stored Gmail draft. */
export interface Draft {
    /** Draft ID; pass to `sendDraft` or `deleteDraft`. */
    id: string;
    /** The draft's message content. */
    message: Message;
}

/** A Gmail label. */
export interface Label {
    /** Label ID, e.g. "INBOX" or "Label_1"; use in `LabelChanges`. */
    id: string;
    /** Display name. */
    name: string;
    /** "system" or "user"; empty if not provided. */
    type: string;
    /** Message-list visibility: "show" or "hide"; empty if not provided. */
    messageListVisibility: string;
    /** Label-list visibility: "labelShow", "labelShowIfUnread", or "labelHide"; empty if not provided. */
    labelListVisibility: string;
}

/** A VFS file attached to outgoing mail. */
export interface OutgoingAttachment {
    /** VFS path of the file to attach. */
    path: string;
    /** Filename shown to recipients; defaults to the path's basename. */
    filename?: string;
    /** MIME type; defaults to "application/octet-stream". */
    mimeType?: string;
}

/** Input for a new email or draft. */
export interface EmailInput {
    /** Recipient addresses; at least one is required. */
    to: string[];
    /** Message subject. */
    subject: string;
    /** Plain-text body. Combined text and HTML bodies must stay under 1 MiB. */
    text: string;
    /** Cc recipient addresses. */
    cc?: string[];
    /** Bcc recipient addresses. */
    bcc?: string[];
    /** HTML body, sent as a multipart/alternative alongside the text body. */
    html?: string;
    /** From header value; defaults to the authenticated account. */
    from?: string;
    /** VFS files to attach; combined contents must stay under 10 MiB. */
    attachments?: OutgoingAttachment[];
}

/** Input for a reply or reply draft. */
export interface ReplyInput {
    /** ID of the message being replied to; recipients, subject, and threading headers derive from it. */
    messageId: string;
    /** Plain-text body of the reply. */
    text: string;
    /** HTML body, sent as a multipart/alternative alongside the text body. */
    html?: string;
    /** Also address the original To and Cc recipients, not just the sender. */
    replyAll?: boolean;
    /** VFS files to attach; combined contents must stay under 10 MiB. */
    attachments?: OutgoingAttachment[];
}

/** Labels added and removed in a modification. */
export interface LabelChanges {
    /** Label IDs to add. */
    addLabelIds?: string[];
    /** Label IDs to remove. */
    removeLabelIds?: string[];
}

/** A bounded unread-inbox triage result. */
export interface TriageResult {
    /** Unread inbox threads on this page. */
    threads: ThreadRef[];
    /** Token for the next page; empty string when there are no more pages. */
    nextPageToken: string;
}

interface ApiProfile {
    emailAddress: string;
    messagesTotal: number;
    threadsTotal: number;
    historyId: string;
}

interface ApiHeader {
    name: string;
    value: string;
}

interface ApiBody {
    attachmentId?: string;
    size?: number;
    data?: string;
}

interface ApiPart {
    partId?: string;
    mimeType?: string;
    filename?: string;
    headers?: ApiHeader[];
    body?: ApiBody;
    parts?: ApiChildPart[];
}

interface ApiChildPart {
    partId?: string;
    mimeType?: string;
    filename?: string;
    headers?: ApiHeader[];
    body?: ApiBody;
    parts?: ApiLeafPart[];
}

interface ApiLeafPart {
    partId?: string;
    mimeType?: string;
    filename?: string;
    headers?: ApiHeader[];
    body?: ApiBody;
}

interface ApiMessage {
    id: string;
    threadId: string;
    labelIds?: string[];
    snippet?: string;
    internalDate?: string;
    payload?: ApiPart;
}

interface ApiThread {
    id: string;
    historyId?: string;
    messages?: ApiMessage[];
    snippet?: string;
}

interface ApiDraft {
    id: string;
    message: ApiMessage;
}

interface ThreadListResponse {
    threads?: ApiThread[];
    nextPageToken?: string;
}

interface DraftListResponse {
    drafts?: ApiDraft[];
    nextPageToken?: string;
}

interface LabelListResponse {
    labels?: ApiLabel[];
}

interface ApiLabel {
    id: string;
    name: string;
    type?: string;
    messageListVisibility?: string;
    labelListVisibility?: string;
}

interface AttachmentResponse {
    data: string;
    size?: number;
}

interface RawMessageRequest {
    raw: string;
    threadId?: string;
}

interface DraftRequest {
    message: RawMessageRequest;
}

interface LabelModifyRequest {
    addLabelIds: string[];
    removeLabelIds: string[];
}

interface LabelCreateRequest {
    name: string;
    labelListVisibility: string;
    messageListVisibility: string;
}

interface ParsedPart {
    text: string;
    html: string;
    attachments: Attachment[];
}

interface GoogleErrorEnvelope {
    error?: GoogleErrorBody;
}

interface GoogleErrorBody {
    message?: string;
    status?: string;
    errors?: GoogleErrorDetail[];
}

interface GoogleErrorDetail {
    reason?: string;
}

/**
 * Return the authenticated Gmail profile.
 * @capability submilli/gmail.getProfile {}
 */
export function getProfile(): Profile {
    check("submilli/gmail.getProfile", {});
    const data = gmailGet("/profile", new Map<string, string>()).json() as ApiProfile;
    return {
        emailAddress: data.emailAddress,
        messagesTotal: data.messagesTotal,
        threadsTotal: data.threadsTotal,
        historyId: data.historyId,
    };
}

/**
 * Search Gmail threads using Gmail's query syntax.
 * @capability submilli/gmail.searchThreads {}
 */
export function searchThreads(query: string, options: SearchOptions | null = null): Page<ThreadRef> {
    check("submilli/gmail.searchThreads", {});
    const params = new Map<string, string>();
    params.set("q", query);
    params.set("maxResults", bounded(options === null ? null : options.limit, 20, 1, 100).toString());
    if (options !== null) {
        putQuery(params, "pageToken", options.pageToken);
        putBool(params, "includeSpamTrash", options.includeSpamTrash);
    }
    const data = gmailGet("/threads", params).json() as ThreadListResponse;
    const items: ThreadRef[] = [];
    if (data.threads !== null) {
        for (const item of data.threads) {
            items.push({ id: item.id, snippet: str(item.snippet), historyId: str(item.historyId) });
        }
    }
    return { items: items, nextPageToken: str(data.nextPageToken) };
}

/**
 * Return a bounded page of unread inbox threads.
 * @capability submilli/gmail.triage {}
 */
export function triage(options: PageOptions | null = null): TriageResult {
    check("submilli/gmail.triage", {});
    const search: SearchOptions = { limit: bounded(options === null ? null : options.limit, 20, 1, 50) };
    if (options !== null && options.pageToken !== null) search.pageToken = options.pageToken;
    const page = searchThreads("in:inbox is:unread", search);
    return { threads: page.items, nextPageToken: page.nextPageToken };
}

/**
 * Fetch and normalize one Gmail thread, returning null when absent.
 * @capability submilli/gmail.getThread {}
 */
export function getThread(threadId: string): Thread | null {
    check("submilli/gmail.getThread", {});
    const response = gmailRawGet("/threads/" + encodeComponent(threadId), formatFull());
    if (response.status === 404) return null;
    requireOk(response);
    return threadFrom(response.json() as ApiThread);
}

/**
 * Fetch and normalize one Gmail message, returning null when absent.
 * @capability submilli/gmail.getMessage {}
 */
export function getMessage(messageId: string): Message | null {
    check("submilli/gmail.getMessage", {});
    return fetchMessage(messageId);
}

/**
 * List one page of drafts.
 * @capability submilli/gmail.listDrafts {}
 */
export function listDrafts(page: PageOptions | null = null): Page<Draft> {
    check("submilli/gmail.listDrafts", {});
    const query = new Map<string, string>();
    applyPage(query, page, 20, 100);
    const data = gmailGet("/drafts", query).json() as DraftListResponse;
    const items: Draft[] = [];
    if (data.drafts !== null) {
        for (const draft of data.drafts) items.push({ id: draft.id, message: messageFrom(draft.message) });
    }
    return { items: items, nextPageToken: str(data.nextPageToken) };
}

/**
 * Fetch one draft, returning null when absent.
 * @capability submilli/gmail.getDraft {}
 */
export function getDraft(draftId: string): Draft | null {
    check("submilli/gmail.getDraft", {});
    const response = gmailRawGet("/drafts/" + encodeComponent(draftId), formatFull());
    if (response.status === 404) return null;
    requireOk(response);
    const draft = response.json() as ApiDraft;
    return { id: draft.id, message: messageFrom(draft.message) };
}

/**
 * Create a draft from structured headers, bodies, and VFS attachments.
 * @capability submilli/gmail.createDraft { recipients: string[] }
 */
export function createDraft(input: EmailInput): Draft {
    const recipients = input.to;
    check("submilli/gmail.createDraft", { recipients: recipients });
    const raw = composeEmail(composeInput(input));
    const response = post(API + "/drafts", { message: { raw: raw } }, authHeaders());
    requireOk(response);
    const draft = response.json() as ApiDraft;
    return { id: draft.id, message: messageFrom(draft.message) };
}

/**
 * Create a draft reply after resolving recipients and threading headers.
 * @capability submilli/gmail.createReplyDraft { recipients: string[] }
 */
export function createReplyDraft(input: ReplyInput): Draft {
    const resolved = replyEmail(input);
    const recipients = resolved.mail.to;
    check("submilli/gmail.createReplyDraft", { recipients: recipients });
    const raw = composeEmail(resolved.mail);
    const request: DraftRequest = { message: { raw: raw, threadId: resolved.threadId } };
    const response = post(API + "/drafts", request, authHeaders());
    requireOk(response);
    const draft = response.json() as ApiDraft;
    return { id: draft.id, message: messageFrom(draft.message) };
}

/**
 * Permanently delete a draft. Missing drafts are treated as already deleted.
 * @capability submilli/gmail.deleteDraft {}
 */
export function deleteDraft(draftId: string): void {
    check("submilli/gmail.deleteDraft", {});
    const response = delete(API + "/drafts/" + encodeComponent(draftId), authHeaders());
    if (response.status !== 404) requireOk(response);
}

/**
 * Send a new email.
 * @capability submilli/gmail.sendEmail { recipients: string[] }
 */
export function sendEmail(input: EmailInput): Message {
    const recipients = input.to;
    check("submilli/gmail.sendEmail", { recipients: recipients });
    const response = post(API + "/messages/send", { raw: composeEmail(composeInput(input)) }, authHeaders());
    requireOk(response);
    return messageFrom(response.json() as ApiMessage);
}

/**
 * Reply to a message after resolving reply or reply-all recipients.
 * @capability submilli/gmail.reply { recipients: string[] }
 */
export function reply(input: ReplyInput): Message {
    const resolved = replyEmail(input);
    const recipients = resolved.mail.to;
    check("submilli/gmail.reply", { recipients: recipients });
    const request: RawMessageRequest = { raw: composeEmail(resolved.mail), threadId: resolved.threadId };
    const response = post(API + "/messages/send", request, authHeaders());
    requireOk(response);
    return messageFrom(response.json() as ApiMessage);
}

/**
 * Send an existing draft.
 * @capability submilli/gmail.sendDraft {}
 */
export function sendDraft(draftId: string): Message {
    check("submilli/gmail.sendDraft", {});
    const response = post(API + "/drafts/send", { id: draftId }, authHeaders());
    requireOk(response);
    return messageFrom(response.json() as ApiMessage);
}

/**
 * List Gmail system and user labels.
 * @capability submilli/gmail.listLabels {}
 */
export function listLabels(): Label[] {
    check("submilli/gmail.listLabels", {});
    const data = gmailGet("/labels", new Map<string, string>()).json() as LabelListResponse;
    const labels: Label[] = [];
    if (data.labels !== null) for (const item of data.labels) labels.push(labelFrom(item));
    return labels;
}

/**
 * Create a visible user label.
 * @capability submilli/gmail.createLabel { name: string }
 */
export function createLabel(name: string): Label {
    check("submilli/gmail.createLabel", { name: name });
    const body: LabelCreateRequest = {
        name: name,
        labelListVisibility: "labelShow",
        messageListVisibility: "show",
    };
    const response = post(API + "/labels", body, authHeaders());
    requireOk(response);
    return labelFrom(response.json() as ApiLabel);
}

/**
 * Add and remove labels from every message in a thread.
 * @capability submilli/gmail.modifyThreadLabels {}
 */
export function modifyThreadLabels(threadId: string, changes: LabelChanges): Thread {
    check("submilli/gmail.modifyThreadLabels", {});
    const response = post(
        API + "/threads/" + encodeComponent(threadId) + "/modify",
        labelChanges(changes),
        authHeaders(),
    );
    requireOk(response);
    return threadFrom(response.json() as ApiThread);
}

/**
 * Add and remove labels from one message.
 * @capability submilli/gmail.modifyMessageLabels {}
 */
export function modifyMessageLabels(messageId: string, changes: LabelChanges): Message {
    check("submilli/gmail.modifyMessageLabels", {});
    const response = post(
        API + "/messages/" + encodeComponent(messageId) + "/modify",
        labelChanges(changes),
        authHeaders(),
    );
    requireOk(response);
    return messageFrom(response.json() as ApiMessage);
}

/**
 * Decode one Gmail attachment into the VFS.
 * @capability submilli/gmail.downloadAttachment { path: string }
 */
export function downloadAttachment(messageId: string, attachmentId: string, path: string): void {
    check("submilli/gmail.downloadAttachment", { path: path });
    const endpoint = "/messages/" + encodeComponent(messageId) + "/attachments/" + encodeComponent(attachmentId);
    const data = gmailGet(endpoint, new Map<string, string>()).json() as AttachmentResponse;
    write(path, Uint8Array.fromBase64(data.data, { alphabet: "base64url" }));
}

interface ComposeInput {
    to: string[];
    subject: string;
    text: string;
    cc?: string[];
    bcc?: string[];
    html?: string;
    from?: string;
    attachments?: OutgoingAttachment[];
    inReplyTo?: string;
    references?: string;
}

interface ResolvedEmail {
    mail: ComposeInput;
    threadId: string;
}

function replyEmail(input: ReplyInput): ResolvedEmail {
    const original = fetchMessage(input.messageId);
    if (original === null) throw new GmailError("not_found", "Gmail message was not found", 404);
    const sender = headerValue(original.headers, "From");
    const replyTo = headerValue(original.headers, "Reply-To");
    const originalTo = addresses(headerValue(original.headers, "To"));
    const originalCc = addresses(headerValue(original.headers, "Cc"));
    const recipients: string[] = [];
    appendUnique(recipients, addresses(replyTo.length > 0 ? replyTo : sender));
    let cc: string[] = [];
    if (input.replyAll === true) {
        appendUnique(recipients, originalTo);
        appendUnique(originalCc, recipients);
        cc = originalCc;
    }
    let subject = headerValue(original.headers, "Subject");
    if (!subject.toLowerCase().startsWith("re:")) subject = "Re: " + subject;
    let references = headerValue(original.headers, "References");
    const messageId = headerValue(original.headers, "Message-ID");
    if (messageId.length > 0) references = references.length > 0 ? references + " " + messageId : messageId;
    const mail: ComposeInput = {
        to: recipients,
        subject: subject,
        text: input.text,
        inReplyTo: messageId,
        references: references,
    };
    if (cc.length > 0) mail.cc = cc;
    if (input.html !== null) mail.html = input.html;
    if (input.attachments !== null) mail.attachments = input.attachments;
    return { mail: mail, threadId: original.threadId };
}

function composeInput(input: EmailInput): ComposeInput {
    const result: ComposeInput = { to: input.to, subject: input.subject, text: input.text };
    if (input.cc !== null) result.cc = input.cc;
    if (input.bcc !== null) result.bcc = input.bcc;
    if (input.html !== null) result.html = input.html;
    if (input.from !== null) result.from = input.from;
    if (input.attachments !== null) result.attachments = input.attachments;
    return result;
}

function composeEmail(input: ComposeInput): string {
    if (input.to.length === 0) throw new GmailError("missing_recipient", "at least one recipient is required", 0);
    validateHeader(input.subject);
    for (const value of input.to) validateHeader(value);
    let headers = "To: " + input.to.join(", ") + "\r\n";
    if (input.cc !== null && input.cc.length > 0) headers += "Cc: " + input.cc.join(", ") + "\r\n";
    if (input.bcc !== null && input.bcc.length > 0) headers += "Bcc: " + input.bcc.join(", ") + "\r\n";
    if (input.from !== null) {
        validateHeader(input.from);
        headers += "From: " + input.from + "\r\n";
    }
    headers += "Subject: " + encodeHeader(input.subject) + "\r\n";
    if (input.inReplyTo !== null && input.inReplyTo.length > 0) headers += "In-Reply-To: " + input.inReplyTo + "\r\n";
    if (input.references !== null && input.references.length > 0) headers += "References: " + input.references + "\r\n";
    headers += "MIME-Version: 1.0\r\n";
    const textBytes = new TextEncoder().encode(input.text);
    const html = input.html;
    let bodyBytes = textBytes.length;
    if (html !== null) bodyBytes += new TextEncoder().encode(html).length;
    if (bodyBytes > MAX_BODY_BYTES) throw new GmailError("body_too_large", "combined text and HTML bodies exceed 1 MiB", 0);
    const attachments: OutgoingAttachment[] = input.attachments !== null ? input.attachments : [];
    const boundary = "submilli_" + Temporal.Now.instant().epochMilliseconds.toString();
    if (attachments.length === 0 && html === null) {
        const message = headers + "Content-Type: text/plain; charset=UTF-8\r\nContent-Transfer-Encoding: base64\r\n\r\n" + textBytes.toBase64();
        return encodeRawMessage(message);
    }
    let mime = headers + "Content-Type: multipart/mixed; boundary=\"" + boundary + "\"\r\n\r\n";
    const alternative = boundary + "_alternative";
    if (html !== null) {
        mime += "--" + boundary + "\r\nContent-Type: multipart/alternative; boundary=\"" + alternative + "\"\r\n\r\n";
        mime += "--" + alternative + "\r\nContent-Type: text/plain; charset=UTF-8\r\nContent-Transfer-Encoding: base64\r\n\r\n";
        mime += textBytes.toBase64() + "\r\n";
        mime += "--" + alternative + "\r\nContent-Type: text/html; charset=UTF-8\r\nContent-Transfer-Encoding: base64\r\n\r\n";
        mime += new TextEncoder().encode(html).toBase64() + "\r\n--" + alternative + "--\r\n";
    } else {
        mime += "--" + boundary + "\r\nContent-Type: text/plain; charset=UTF-8\r\nContent-Transfer-Encoding: base64\r\n\r\n";
        mime += textBytes.toBase64() + "\r\n";
    }
    let total = 0;
    for (const attachment of attachments) {
        const metadata = stat(attachment.path);
        if (metadata === null || metadata.kind !== "file") throw new GmailError("attachment_not_found", "attachment is not a VFS file: " + attachment.path, 0);
        total += metadata.size;
        if (total > MAX_ATTACHMENT_BYTES) throw new GmailError("attachments_too_large", "combined attachment contents exceed 10 MiB", 0);
        const bytes = read(attachment.path);
        if (bytes === null) throw new GmailError("attachment_unreadable", "attachment exceeds the VFS whole-read limit", 0);
        const filename = attachment.filename !== null ? attachment.filename : basename(attachment.path);
        const mimeType = attachment.mimeType !== null ? attachment.mimeType : "application/octet-stream";
        validateHeader(filename);
        validateHeader(mimeType);
        mime += "--" + boundary + "\r\nContent-Type: " + mimeType + "; name=\"" + filename + "\"\r\n";
        mime += "Content-Disposition: attachment; filename=\"" + filename + "\"\r\nContent-Transfer-Encoding: base64\r\n\r\n";
        mime += bytes.toBase64() + "\r\n";
    }
    return encodeRawMessage(mime + "--" + boundary + "--\r\n");
}

function fetchMessage(messageId: string): Message | null {
    const response = gmailRawGet("/messages/" + encodeComponent(messageId), formatFull());
    if (response.status === 404) return null;
    requireOk(response);
    return messageFrom(response.json() as ApiMessage);
}

function messageFrom(item: ApiMessage): Message {
    let parsed: ParsedPart = { text: "", html: "", attachments: [] };
    let headers: Header[] = [];
    if (item.payload !== null) {
        parsed = parsePart(item.payload);
        headers = headersFrom(item.payload.headers);
    }
    return {
        id: item.id,
        threadId: item.threadId,
        labelIds: item.labelIds !== null ? item.labelIds : [],
        snippet: str(item.snippet),
        internalDate: str(item.internalDate),
        headers: headers,
        text: parsed.text,
        html: parsed.html,
        attachments: parsed.attachments,
    };
}

function threadFrom(item: ApiThread): Thread {
    const messages: Message[] = [];
    if (item.messages !== null) for (const message of item.messages) messages.push(messageFrom(message));
    return { id: item.id, historyId: str(item.historyId), messages: messages };
}

function parsePart(part: ApiPart): ParsedPart {
    let text = "";
    let html = "";
    const attachments: Attachment[] = [];
    const children = part.parts;
    if (children !== null) {
        for (const child of children) {
            const parsed = parseChildPart(child);
            text += parsed.text;
            html += parsed.html;
            for (const attachment of parsed.attachments) attachments.push(attachment);
        }
    }
    const mimeType = str(part.mimeType).toLowerCase();
    const filename = str(part.filename);
    const body = part.body;
    if (body !== null) {
        const attachmentId = str(body.attachmentId);
        const bodySize = body.size;
        const bodyData = body.data;
        if (filename.length > 0 || attachmentId.length > 0) {
            attachments.push({
                partId: str(part.partId),
                attachmentId: attachmentId,
                filename: filename,
                mimeType: mimeType,
                size: bodySize !== null ? bodySize : 0,
            });
        } else if (bodyData !== null) {
            const decoded = new TextDecoder().decode(Uint8Array.fromBase64(bodyData, { alphabet: "base64url" }));
            if (mimeType.startsWith("text/plain")) text += decoded;
            if (mimeType.startsWith("text/html")) html += decoded;
        }
    }
    return { text: text, html: html, attachments: attachments };
}

function parseChildPart(part: ApiChildPart): ParsedPart {
    let parsed = parsePartBody(
        part.partId,
        part.mimeType,
        part.filename,
        part.body,
    );
    const children = part.parts;
    if (children !== null) {
        for (const child of children) {
            const leaf = parseLeafPart(child);
            parsed.text += leaf.text;
            parsed.html += leaf.html;
            for (const attachment of leaf.attachments) parsed.attachments.push(attachment);
        }
    }
    return parsed;
}

function parseLeafPart(part: ApiLeafPart): ParsedPart {
    return parsePartBody(part.partId, part.mimeType, part.filename, part.body);
}

function parsePartBody(
    partIdValue: string | null,
    mimeTypeValue: string | null,
    filenameValue: string | null,
    body: ApiBody | null,
): ParsedPart {
    let text = "";
    let html = "";
    const attachments: Attachment[] = [];
    const mimeType = str(mimeTypeValue).toLowerCase();
    const filename = str(filenameValue);
    if (body !== null) {
        const attachmentId = str(body.attachmentId);
        const bodySize = body.size;
        const bodyData = body.data;
        if (filename.length > 0 || attachmentId.length > 0) {
            attachments.push({
                partId: str(partIdValue),
                attachmentId: attachmentId,
                filename: filename,
                mimeType: mimeType,
                size: bodySize !== null ? bodySize : 0,
            });
        } else if (bodyData !== null) {
            const decoded = new TextDecoder().decode(Uint8Array.fromBase64(bodyData, { alphabet: "base64url" }));
            if (mimeType.startsWith("text/plain")) text += decoded;
            if (mimeType.startsWith("text/html")) html += decoded;
        }
    }
    return { text: text, html: html, attachments: attachments };
}

function headersFrom(values: ApiHeader[] | null): Header[] {
    const headers: Header[] = [];
    if (values !== null) for (const header of values) headers.push({ name: header.name, value: decodeHeader(header.value) });
    return headers;
}

function decodeHeader(value: string): string {
    const upper = value.toUpperCase();
    const base64Marker = "=?UTF-8?B?";
    const quotedMarker = "=?UTF-8?Q?";
    const base64Start = upper.indexOf(base64Marker);
    const quotedStart = upper.indexOf(quotedMarker);
    let start = base64Start;
    let marker = base64Marker;
    if (start < 0 || (quotedStart >= 0 && quotedStart < start)) {
        start = quotedStart;
        marker = quotedMarker;
    }
    if (start < 0) return value;
    const encodedStart = start + marker.length;
    const end = value.indexOf("?=", encodedStart);
    if (end < 0) return value;
    const encoded = value.slice(encodedStart, end);
    let decoded = "";
    if (marker === base64Marker) {
        decoded = new TextDecoder().decode(Uint8Array.fromBase64(encoded));
    } else {
        decoded = decodeComponent(encoded.replaceAll("_", " ").replaceAll("=", "%"));
    }
    return value.slice(0, start) + decoded + decodeHeader(value.slice(end + 2));
}

function encodeHeader(value: string): string {
    validateHeader(value);
    for (let i = 0; i < value.length; i += 1) {
        if (value.charCodeAt(i) > 127) return "=?UTF-8?B?" + new TextEncoder().encode(value).toBase64() + "?=";
    }
    return value;
}

function headerValue(headers: Header[], name: string): string {
    const expected = name.toLowerCase();
    for (const header of headers) if (header.name.toLowerCase() === expected) return header.value;
    return "";
}

function addresses(value: string): string[] {
    const out: string[] = [];
    for (const part of value.split(",")) {
        const address = part.trim();
        if (address.length > 0) out.push(address);
    }
    return out;
}

function appendUnique(target: string[], values: string[]): void {
    for (const value of values) if (!target.includes(value)) target.push(value);
}

function labelFrom(item: ApiLabel): Label {
    return {
        id: item.id,
        name: item.name,
        type: str(item.type),
        messageListVisibility: str(item.messageListVisibility),
        labelListVisibility: str(item.labelListVisibility),
    };
}

function labelChanges(changes: LabelChanges): LabelModifyRequest {
    return {
        addLabelIds: changes.addLabelIds !== null ? changes.addLabelIds : [],
        removeLabelIds: changes.removeLabelIds !== null ? changes.removeLabelIds : [],
    };
}

function formatFull(): Map<string, string> {
    const query = new Map<string, string>();
    query.set("format", "full");
    return query;
}

function gmailGet(path: string, query: Map<string, string>): Response {
    return requireOk(gmailRawGet(path, query));
}

function gmailRawGet(path: string, query: Map<string, string>): Response {
    const encoded = encodeQuery(query);
    return get(API + path + (encoded.length > 0 ? "?" + encoded : ""), authHeaders());
}

function authHeaders(): Map<string, string> {
    const token = secrets.get("GOOGLE_ACCESS_TOKEN");
    if (token === null) throw new GmailError("missing_token", "GOOGLE_ACCESS_TOKEN is not bound", 0);
    const headers = new Map<string, string>();
    headers.set("Authorization", "Bearer " + token);
    return headers;
}

function requireOk(response: Response): Response {
    if (response.ok) return response;
    let code = response.status === 401 ? "unauthorized" : "http_error";
    let message = "Gmail request failed: HTTP " + response.status.toString() + " " + response.statusText;
    if (response.body.startsWith("{")) {
        const envelope = response.json() as GoogleErrorEnvelope;
        const body = envelope.error;
        if (body !== null) {
            const errors = body.errors;
            const bodyStatus = body.status;
            const bodyMessage = body.message;
            if (errors !== null && errors.length > 0) {
                const reason = errors[0].reason;
                if (reason !== null) code = reason;
            } else if (bodyStatus !== null) {
                code = bodyStatus;
            }
            if (bodyMessage !== null) message = bodyMessage;
        }
    }
    throw new GmailError(code, message, response.status);
}

function applyPage(query: Map<string, string>, page: PageOptions | null, fallback: number, max: number): void {
    query.set("maxResults", bounded(page === null ? null : page.limit, fallback, 1, max).toString());
    if (page !== null) putQuery(query, "pageToken", page.pageToken);
}

function putQuery(query: Map<string, string>, name: string, value: string | null): void {
    if (value !== null) query.set(name, value);
}

function putBool(query: Map<string, string>, name: string, value: boolean | null): void {
    if (value !== null) query.set(name, value ? "true" : "false");
}

function bounded(value: number | null, fallback: number, min: number, max: number): number {
    const actual = value === null ? fallback : value;
    if (actual < min) return min;
    if (actual > max) return max;
    return actual;
}

function str(value: string | null): string {
    return value === null ? "" : value;
}

function basename(path: string): string {
    const parts = path.split("/");
    return parts.length === 0 ? "attachment" : parts[parts.length - 1];
}

function validateHeader(value: string): void {
    if (value.includes("\r") || value.includes("\n")) throw new GmailError("invalid_header", "mail headers must not contain CR or LF", 0);
}

function encodeRawMessage(value: string): string {
    return new TextEncoder().encode(value).toBase64({ alphabet: "base64url", omitPadding: true });
}
