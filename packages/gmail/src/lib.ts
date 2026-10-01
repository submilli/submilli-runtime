import { get, post, delete, Response } from "submilli:http";
import { decodeComponent, encodeComponent, encodeQuery } from "submilli:url";
import { read, stat, write } from "submilli:fs";
import secrets from "submilli:secrets";
import { check } from "submilli:security";

const API = "https://gmail.googleapis.com/gmail/v1/users/me";
const MAX_BODY_BYTES = 1048576;
const MAX_ATTACHMENT_BYTES = 10485760;
// Exactly one `@` between two runs of printable ASCII. A character outside that range can look
// like, or be dropped to leave, an address policy refuses, and a lone surrogate is encoded as
// U+FFFD, so the header would not carry the string the check read. The characters RFC 5322
// reserves for address syntax, `()<>[]:;\,"`, are left out: they would let the mail system read a
// list, a group, a comment or a quoted name where policy saw one address.
const BARE_ADDRESS = /^[!#-'*+\-.\/0-9=?A-Z^_`a-z{|}~]+@[!#-'*+\-.\/0-9=?A-Z^_`a-z{|}~]+$/;

// The dots that end a fully qualified domain. The address is the same mailbox without them.
const TRAILING_DOTS = /\.+$/;

// A line break, CRLF or LF, and the space or tab that makes it folding.
const FOLD = /\r?\n([ \t])/g;

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
    /**
     * Recipient addresses, one bare address such as "dana@example.com" per entry; at least one is
     * required. Every address is checked and sent in lowercase, without a dot after the domain.
     */
    to: string[];
    /** Message subject. */
    subject: string;
    /** Plain-text body. Combined text and HTML bodies must stay under 1 MiB. */
    text: string;
    /** Cc recipient addresses, one bare address per entry. */
    cc?: string[];
    /** Bcc recipient addresses, one bare address per entry. */
    bcc?: string[];
    /** HTML body, sent as a multipart/alternative alongside the text body. */
    html?: string;
    /** One bare address to send from, such as a send-as alias; defaults to the authenticated account. */
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

/** The To and Cc addresses of a reply, as bare addresses. */
export interface ReplyRecipients {
    /** To addresses: the original Reply-To addresses, or From when there are none, then on reply-all the original To. */
    to: string[];
    /** Cc addresses: on reply-all, the original Cc addresses that are not already in `to`; otherwise empty. */
    cc: string[];
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
    const limit = options === null ? null : options.limit;
    const pageToken = options === null ? null : options.pageToken;
    const includeSpamTrash = options === null ? null : options.includeSpamTrash;
    check("submilli/gmail.searchThreads", {});
    const params = new Map<string, string>();
    params.set("q", query);
    params.set("maxResults", bounded(limit, 20, 1, 100).toString());
    putQuery(params, "pageToken", pageToken);
    putBool(params, "includeSpamTrash", includeSpamTrash);
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
    const limit = options === null ? null : options.limit;
    const pageToken = options === null ? null : options.pageToken;
    check("submilli/gmail.triage", {});
    const search: SearchOptions = { limit: bounded(limit, 20, 1, 50) };
    if (pageToken !== null) search.pageToken = pageToken;
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
    const limit = page === null ? null : page.limit;
    const pageToken = page === null ? null : page.pageToken;
    check("submilli/gmail.listDrafts", {});
    const query = new Map<string, string>();
    applyPage(query, limit, pageToken, 20, 100);
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
    const draft = fetchDraft(draftId, formatFull());
    if (draft === null) return null;
    return { id: draft.id, message: messageFrom(draft.message) };
}

/**
 * Create a draft from structured headers, bodies, and VFS attachments.
 * @capability submilli/gmail.createDraft { recipients: string[], from: string }
 */
export function createDraft(input: EmailInput): Draft {
    const { to: requestedTo, subject, text, cc: requestedCc, bcc: requestedBcc, html, from } = input;
    const to: string[] = [];
    for (const entry of requestedTo) to.push(entry);
    let cc: string[] | null = null;
    if (requestedCc !== null) {
        const copied: string[] = [];
        for (const entry of requestedCc) copied.push(entry);
        cc = copied;
    }
    let bcc: string[] | null = null;
    if (requestedBcc !== null) {
        const copied: string[] = [];
        for (const entry of requestedBcc) copied.push(entry);
        bcc = copied;
    }
    const attachments = input.attachments;
    const mail = composeInput({
        to: to,
        subject: subject,
        text: text,
        cc: cc,
        bcc: bcc,
        html: html,
        from: from,
    });
    check("submilli/gmail.createDraft", { recipients: messageRecipients(mail), from: mail.from });
    const raw = composeEmail(mail, attachments);
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
    const { messageId, text, html, replyAll } = input;
    const attachments = input.attachments;
    const resolved = replyEmail({
        messageId: messageId,
        text: text,
        html: html,
        replyAll: replyAll,
    });
    check("submilli/gmail.createReplyDraft", { recipients: messageRecipients(resolved.mail) });
    const raw = composeEmail(resolved.mail, attachments);
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
 * @capability submilli/gmail.sendEmail { recipients: string[], from: string }
 */
export function sendEmail(input: EmailInput): Message {
    const { to: requestedTo, subject, text, cc: requestedCc, bcc: requestedBcc, html, from } = input;
    const to: string[] = [];
    for (const entry of requestedTo) to.push(entry);
    let cc: string[] | null = null;
    if (requestedCc !== null) {
        const copied: string[] = [];
        for (const entry of requestedCc) copied.push(entry);
        cc = copied;
    }
    let bcc: string[] | null = null;
    if (requestedBcc !== null) {
        const copied: string[] = [];
        for (const entry of requestedBcc) copied.push(entry);
        bcc = copied;
    }
    const attachments = input.attachments;
    const mail = composeInput({
        to: to,
        subject: subject,
        text: text,
        cc: cc,
        bcc: bcc,
        html: html,
        from: from,
    });
    check("submilli/gmail.sendEmail", { recipients: messageRecipients(mail), from: mail.from });
    const response = post(API + "/messages/send", { raw: composeEmail(mail, attachments) }, authHeaders());
    requireOk(response);
    return messageFrom(response.json() as ApiMessage);
}

/**
 * Reply to a message after resolving reply or reply-all recipients.
 * @capability submilli/gmail.reply { recipients: string[] }
 */
export function reply(input: ReplyInput): Message {
    const { messageId, text, html, replyAll } = input;
    const attachments = input.attachments;
    const resolved = replyEmail({
        messageId: messageId,
        text: text,
        html: html,
        replyAll: replyAll,
    });
    check("submilli/gmail.reply", { recipients: messageRecipients(resolved.mail) });
    const request: RawMessageRequest = { raw: composeEmail(resolved.mail, attachments), threadId: resolved.threadId };
    const response = post(API + "/messages/send", request, authHeaders());
    requireOk(response);
    return messageFrom(response.json() as ApiMessage);
}

/**
 * Send an existing draft. The draft is read first, so the check covers every address in its To, Cc and Bcc headers.
 * @capability submilli/gmail.sendDraft { recipients: string[] }
 */
export function sendDraft(draftId: string): Message {
    const draft = fetchDraft(draftId, formatMetadata());
    if (draft === null) throw new GmailError("not_found", "Gmail draft was not found", 404);
    const recipients = draftRecipients(sentHeaders(draft.message));
    check("submilli/gmail.sendDraft", { recipients: recipients });
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
    const { addLabelIds: requestedAddLabelIds, removeLabelIds: requestedRemoveLabelIds } = changes;
    const addLabelIds: string[] = [];
    if (requestedAddLabelIds !== null) {
        for (const id of requestedAddLabelIds) addLabelIds.push(id);
    }
    const removeLabelIds: string[] = [];
    if (requestedRemoveLabelIds !== null) {
        for (const id of requestedRemoveLabelIds) removeLabelIds.push(id);
    }
    const request: LabelModifyRequest = { addLabelIds: addLabelIds, removeLabelIds: removeLabelIds };
    check("submilli/gmail.modifyThreadLabels", {});
    const response = post(
        API + "/threads/" + encodeComponent(threadId) + "/modify",
        request,
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
    const { addLabelIds: requestedAddLabelIds, removeLabelIds: requestedRemoveLabelIds } = changes;
    const addLabelIds: string[] = [];
    if (requestedAddLabelIds !== null) {
        for (const id of requestedAddLabelIds) addLabelIds.push(id);
    }
    const removeLabelIds: string[] = [];
    if (requestedRemoveLabelIds !== null) {
        for (const id of requestedRemoveLabelIds) removeLabelIds.push(id);
    }
    const request: LabelModifyRequest = { addLabelIds: addLabelIds, removeLabelIds: removeLabelIds };
    check("submilli/gmail.modifyMessageLabels", {});
    const response = post(
        API + "/messages/" + encodeComponent(messageId) + "/modify",
        request,
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

/**
 * Resolve the To and Cc addresses of a reply from the original message's headers, as bare addresses.
 * Pass header values as Gmail returns them, before RFC 2047 decoding. The `headers` of a `Message`
 * this package returns are already decoded and are not suitable input: decoding can turn a display
 * name into address syntax, which would then be read as a recipient. Throws `GmailError`
 * `invalid_recipient` when an address header does not resolve to bare addresses.
 */
export function replyRecipients(headers: Header[], replyAll: boolean): ReplyRecipients {
    const replyTo = headerAddresses(headers, "Reply-To");
    const seen = new Set<string>();
    const to: string[] = [];
    appendUnseen(to, replyTo.length > 0 ? replyTo : headerAddresses(headers, "From"), seen);
    const cc: string[] = [];
    if (replyAll) {
        appendUnseen(to, headerAddresses(headers, "To"), seen);
        appendUnseen(cc, headerAddresses(headers, "Cc"), seen);
    }
    return { to: to, cc: cc };
}

/**
 * Return every address in a draft's To, Cc and Bcc headers as bare addresses, deduplicated, in that order.
 * Pass header values as Gmail returns them, before RFC 2047 decoding. The `headers` of a `Message`
 * this package returns are already decoded and are not suitable input: decoding can turn a display
 * name into address syntax, which would then be read as a recipient. Throws `GmailError`
 * `invalid_recipient` when one of those headers does not resolve to bare addresses.
 */
export function draftRecipients(headers: Header[]): string[] {
    const seen = new Set<string>();
    const recipients: string[] = [];
    for (const name of ["To", "Cc", "Bcc"]) appendUnseen(recipients, headerAddresses(headers, name), seen);
    return recipients;
}

interface ComposeInput {
    to: string[];
    subject: string;
    text: string;
    cc?: string[];
    bcc?: string[];
    html?: string;
    from?: string;
    inReplyTo?: string;
    references?: string;
}

interface ResolvedEmail {
    mail: ComposeInput;
    threadId: string;
}

interface EmailFields {
    to: string[];
    subject: string;
    text: string;
    cc: string[] | null;
    bcc: string[] | null;
    html: string | null;
    from: string | null;
}

interface ReplyFields {
    messageId: string;
    text: string;
    html: string | null;
    replyAll: boolean | null;
}

function replyEmail(fields: ReplyFields): ResolvedEmail {
    const original = fetchApiMessage(fields.messageId);
    if (original === null) throw new GmailError("not_found", "Gmail message was not found", 404);
    const headers = sentHeaders(original);
    const recipients = replyRecipients(headers, fields.replyAll === true);
    let subject = decodeHeader(unfold(headerValue(headers, "Subject")));
    if (!subject.toLowerCase().startsWith("re:")) subject = "Re: " + subject;
    let references = decodeHeader(unfold(headerValue(headers, "References")));
    const inReplyTo = decodeHeader(unfold(headerValue(headers, "Message-ID")));
    if (inReplyTo.length > 0) references = references.length > 0 ? references + " " + inReplyTo : inReplyTo;
    const mail: ComposeInput = {
        to: recipients.to,
        subject: subject,
        text: fields.text,
        inReplyTo: inReplyTo,
        references: references,
    };
    if (recipients.cc.length > 0) mail.cc = recipients.cc;
    const html = fields.html;
    if (html !== null) mail.html = html;
    return { mail: mail, threadId: original.threadId };
}

function composeInput(fields: EmailFields): ComposeInput {
    const { to, subject, text, cc, bcc, html, from } = fields;
    const result: ComposeInput = { to: bareAddresses(to, "to"), subject: subject, text: text };
    if (cc !== null) result.cc = bareAddresses(cc, "cc");
    if (bcc !== null) result.bcc = bareAddresses(bcc, "bcc");
    if (html !== null) result.html = html;
    if (from !== null) result.from = senderAddress(from);
    return result;
}

// The check reads the same lists `composeEmail` writes, so policy sees every address the message goes to.
function messageRecipients(mail: ComposeInput): string[] {
    const seen = new Set<string>();
    const recipients: string[] = [];
    appendUnseen(recipients, mail.to, seen);
    const cc = mail.cc;
    if (cc !== null) appendUnseen(recipients, cc, seen);
    const bcc = mail.bcc;
    if (bcc !== null) appendUnseen(recipients, bcc, seen);
    return recipients;
}

function composeEmail(input: ComposeInput, requestedAttachments: OutgoingAttachment[] | null): string {
    if (input.to.length === 0) throw new GmailError("missing_recipient", "at least one recipient is required", 0);
    validateHeader(input.subject);
    let headers = addressHeader("To", input.to, "to");
    if (input.cc !== null && input.cc.length > 0) headers += addressHeader("Cc", input.cc, "cc");
    if (input.bcc !== null && input.bcc.length > 0) headers += addressHeader("Bcc", input.bcc, "bcc");
    if (input.from !== null) headers += addressHeader("From", [input.from], "from");
    headers += "Subject: " + encodeHeader(input.subject) + "\r\n";
    if (input.inReplyTo !== null && input.inReplyTo.length > 0) {
        validateHeader(input.inReplyTo);
        headers += "In-Reply-To: " + input.inReplyTo + "\r\n";
    }
    if (input.references !== null && input.references.length > 0) {
        validateHeader(input.references);
        headers += "References: " + input.references + "\r\n";
    }
    headers += "MIME-Version: 1.0\r\n";
    const textBytes = new TextEncoder().encode(input.text);
    const html = input.html;
    let bodyBytes = textBytes.length;
    if (html !== null) bodyBytes += new TextEncoder().encode(html).length;
    if (bodyBytes > MAX_BODY_BYTES) throw new GmailError("body_too_large", "combined text and HTML bodies exceed 1 MiB", 0);
    const attachments: OutgoingAttachment[] = requestedAttachments !== null ? requestedAttachments : [];
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
        // One read of the path, so the size checked is the size of the file sent.
        const path = attachment.path;
        const filename = attachment.filename !== null ? attachment.filename : basename(path);
        const mimeType = attachment.mimeType !== null ? attachment.mimeType : "application/octet-stream";
        validateFilename(filename);
        validateHeader(mimeType);
        const metadata = stat(path);
        if (metadata === null || metadata.kind !== "file") throw new GmailError("attachment_not_found", "attachment is not a VFS file: " + path, 0);
        total += metadata.size;
        if (total > MAX_ATTACHMENT_BYTES) throw new GmailError("attachments_too_large", "combined attachment contents exceed 10 MiB", 0);
        const bytes = read(path);
        if (bytes === null) throw new GmailError("attachment_unreadable", "attachment exceeds the VFS whole-read limit", 0);
        mime += "--" + boundary + "\r\nContent-Type: " + mimeType + "; name=\"" + filename + "\"\r\n";
        mime += "Content-Disposition: attachment; filename=\"" + filename + "\"\r\nContent-Transfer-Encoding: base64\r\n\r\n";
        mime += bytes.toBase64() + "\r\n";
    }
    return encodeRawMessage(mime + "--" + boundary + "--\r\n");
}

// Refuses rather than repairs: the caller's check has already read these addresses.
function addressHeader(name: string, addresses: string[], field: string): string {
    for (const address of addresses) if (!isBareAddress(address)) throw invalidRecipient(field);
    return name + ": " + addresses.join(", ") + "\r\n";
}

function fetchMessage(messageId: string): Message | null {
    const item = fetchApiMessage(messageId);
    if (item === null) return null;
    return messageFrom(item);
}

function fetchApiMessage(messageId: string): ApiMessage | null {
    const response = gmailRawGet("/messages/" + encodeComponent(messageId), formatFull());
    if (response.status === 404) return null;
    requireOk(response);
    return response.json() as ApiMessage;
}

function fetchDraft(draftId: string, query: Map<string, string>): ApiDraft | null {
    const response = gmailRawGet("/drafts/" + encodeComponent(draftId), query);
    if (response.status === 404) return null;
    requireOk(response);
    return response.json() as ApiDraft;
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

// Headers as stored. They are not RFC 2047-decoded, because decoding an encoded display name
// can produce commas, quotes and angle brackets that were never address syntax. They are not
// unfolded either: an address header must show its line breaks to `unfoldAddresses`, and the
// reader of any other header unfolds it.
function sentHeaders(item: ApiMessage): Header[] {
    const headers: Header[] = [];
    const payload = item.payload;
    if (payload === null) return headers;
    const values = payload.headers;
    if (values === null) return headers;
    for (const header of values) headers.push({ name: header.name, value: header.value });
    return headers;
}

function unfold(value: string): string {
    return value.replaceAll("\r\n", "").replaceAll("\r", " ").replaceAll("\n", " ");
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

function headerAddresses(headers: Header[], name: string): string[] {
    const expected = name.toLowerCase();
    const addresses: string[] = [];
    for (const header of headers) {
        if (header.name.toLowerCase() !== expected) continue;
        for (const address of addressList(header.value, name)) addresses.push(address);
    }
    return addresses;
}

// Anything that is not plainly `address` or `Name <address>` is refused rather than guessed at:
// for a stored draft the mail system reads the header itself, and the check must not see fewer
// addresses than it does.
function addressList(value: string, header: string): string[] {
    const addresses: string[] = [];
    for (const mailbox of addressSyntax(unfoldAddresses(value, header), header).split(",")) {
        const written = mailboxAddress(mailbox, header);
        if (written === null) continue;
        const address = normalizedAddress(written);
        if (address === null) throw unresolvedHeader(header);
        addresses.push(address);
    }
    return addresses;
}

// A line break followed by a space or a tab is folding, and is removed. Any other CR or LF ends
// the header for a mail system, which would read what follows as something else, so it is
// refused as the send path refuses it. The ends are trimmed first, so that a line break with
// only spaces after it counts as one that ends the header. The folds are removed in one pass:
// what removing one leaves behind is never read as another.
function unfoldAddresses(value: string, header: string): string {
    const unfolded = trimSpacesAndTabs(value).replace(FOLD, "$1");
    if (unfolded.includes("\r") || unfolded.includes("\n")) throw unresolvedHeader(header);
    return unfolded;
}

// Replaces each comment with a space and each quoted string with a bare `"`, so the commas and
// angle brackets that remain are address syntax and not display-name text. It moves from one
// quote or parenthesis to the next and copies what lies between them whole: reading a header
// one character at a time costs a host call per character.
function addressSyntax(value: string, header: string): string {
    const segments: string[] = [];
    let position = 0;
    let special = firstOf(value, ["\"", "(", ")"], position);
    while (special >= 0) {
        const char = value.charAt(special);
        if (char === ")") throw unresolvedHeader(header);
        if (char === "\"") {
            segments.push(value.slice(position, special + 1));
            position = quotedStringEnd(value, special + 1, header);
        } else {
            segments.push(value.slice(position, special));
            segments.push(" ");
            position = commentEnd(value, special + 1, header);
        }
        special = firstOf(value, ["\"", "(", ")"], position);
    }
    if (position === 0) return value;
    segments.push(value.slice(position));
    return segments.join("");
}

// The index after the `"` that closes a quoted string whose content starts at `start`.
function quotedStringEnd(value: string, start: number, header: string): number {
    let position = start;
    while (position < value.length) {
        const special = firstOf(value, ["\"", "\\"], position);
        if (special < 0) break;
        if (value.charAt(special) === "\"") return special + 1;
        // A backslash escapes the character after it, whichever it is.
        position = special + 2;
    }
    throw unresolvedHeader(header);
}

// The index after the `)` that closes a comment whose content starts at `start`. Comments nest.
function commentEnd(value: string, start: number, header: string): number {
    let position = start;
    let depth = 1;
    while (position < value.length) {
        const special = firstOf(value, ["(", ")", "\\"], position);
        if (special < 0) break;
        const char = value.charAt(special);
        if (char === "\\") {
            position = special + 2;
            continue;
        }
        depth += char === "(" ? 1 : -1;
        position = special + 1;
        if (depth === 0) return position;
    }
    throw unresolvedHeader(header);
}

// The lowest index at or after `from` that holds one of `marks`, or -1.
function firstOf(value: string, marks: string[], from: number): number {
    let first = -1;
    for (const mark of marks) {
        const index = value.indexOf(mark, from);
        if (index >= 0 && (first < 0 || index < first)) first = index;
    }
    return first;
}

// Returns null for an element that names nobody: an empty element or an empty group.
function mailboxAddress(mailbox: string, header: string): string | null {
    const open = mailbox.indexOf("<");
    if (open < 0) {
        const text = trimSpacesAndTabs(mailbox);
        if (text.length === 0 || isEmptyGroup(text)) return null;
        return text;
    }
    const close = mailbox.indexOf(">", open);
    // Mail systems disagree on whether an unquoted name holding `@`, or text after the `>`, is
    // a second address.
    const nameHoldsAddress = mailbox.slice(0, open).includes("@");
    const trailing = close < 0 ? "" : trimSpacesAndTabs(mailbox.slice(close + 1));
    if (close < 0 || nameHoldsAddress || trailing.length > 0) throw unresolvedHeader(header);
    return trimSpacesAndTabs(mailbox.slice(open + 1, close));
}

// `undisclosed-recipients:;` is what a message sent only to Bcc carries in To.
function isEmptyGroup(text: string): boolean {
    const colon = text.indexOf(":");
    return colon > 0 && !text.includes("@") && trimSpacesAndTabs(text.slice(colon + 1)) === ";";
}

// One address per entry, so the list the check reads is the list the mail system reads.
function bareAddresses(entries: string[], field: string): string[] {
    const addresses: string[] = [];
    for (const entry of entries) {
        const address = normalizedAddress(trimSpacesAndTabs(entry));
        if (address === null) throw invalidRecipient(field);
        addresses.push(address);
    }
    return addresses;
}

function senderAddress(from: string): string {
    const address = normalizedAddress(trimSpacesAndTabs(from));
    if (address === null) {
        throw new GmailError("invalid_sender", "from must hold one bare email address, such as dana@example.com, without a display name", 0);
    }
    return address;
}

// The one spelling of a bare address that policy reads and the header carries, or null when the
// value is not a bare address. Mail systems deliver to a mailbox whatever the case of its address
// and whether or not a dot ends its domain, so a rule naming one spelling would let another past.
function normalizedAddress(value: string): string | null {
    if (!isBareAddress(value)) return null;
    const address = value.toLowerCase().replace(TRAILING_DOTS, "");
    if (address.endsWith("@")) return null;
    return address;
}

// Removes the ASCII space and tab at either end, and nothing else. `trim` also removes line
// breaks, the no-break space and the other Unicode spaces, which would then never reach the
// `BARE_ADDRESS` test, while a stored header would still hold them.
function trimSpacesAndTabs(value: string): string {
    let start = 0;
    let end = value.length;
    while (start < end && isSpaceOrTab(value.charCodeAt(start))) start += 1;
    while (end > start && isSpaceOrTab(value.charCodeAt(end - 1))) end -= 1;
    if (start === 0 && end === value.length) return value;
    return value.slice(start, end);
}

function isSpaceOrTab(unit: number): boolean {
    return unit === 0x20 || unit === 0x09;
}

// One match per address rather than a test per character, which costs a host call each.
function isBareAddress(value: string): boolean {
    // An RFC 2047 encoded word could decode to an address other than the one the check reads.
    if (value.includes("=?")) return false;
    return BARE_ADDRESS.test(value);
}

function invalidRecipient(field: string): GmailError {
    const expected = "one bare email address per entry, such as dana@example.com, without a display name, "
        + "angle brackets, commas, or other address syntax; spaces and tabs around an entry are removed, "
        + "and any other whitespace is refused";
    return new GmailError("invalid_recipient", "recipient field " + field + " must hold " + expected, 0);
}

function unresolvedHeader(header: string): GmailError {
    return new GmailError(
        "invalid_recipient",
        "the " + header + " header of the stored message does not resolve to bare email addresses",
        0,
    );
}

// Appends, in order, each value `seen` does not hold yet. One set serves every list of a
// message, so an address appears once across them.
function appendUnseen(target: string[], values: string[], seen: Set<string>): void {
    for (const value of values) {
        if (seen.has(value)) continue;
        seen.add(value);
        target.push(value);
    }
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

function formatFull(): Map<string, string> {
    const query = new Map<string, string>();
    query.set("format", "full");
    return query;
}

// Headers without bodies or attachments: all that a recipient check reads.
function formatMetadata(): Map<string, string> {
    const query = new Map<string, string>();
    query.set("format", "metadata");
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

function applyPage(query: Map<string, string>, limit: number | null, pageToken: string | null, fallback: number, max: number): void {
    query.set("maxResults", bounded(limit, fallback, 1, max).toString());
    putQuery(query, "pageToken", pageToken);
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

// The name sits inside a quoted string, where a `"` would end it and a `\` would escape what follows.
function validateFilename(filename: string): void {
    validateHeader(filename);
    if (filename.includes("\"") || filename.includes("\\")) {
        throw new GmailError("invalid_attachment_filename", "attachment filenames must not contain a double quote or a backslash", 0);
    }
}

function validateHeader(value: string): void {
    if (value.includes("\r") || value.includes("\n")) throw new GmailError("invalid_header", "mail headers must not contain CR or LF", 0);
}

function encodeRawMessage(value: string): string {
    return new TextEncoder().encode(value).toBase64({ alphabet: "base64url", omitPadding: true });
}
