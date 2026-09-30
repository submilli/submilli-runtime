import { get, post, download, DownloadOptions, DownloadResult, Response } from "submilli:http";
import { encodeQuery } from "submilli:url";
import secrets from "submilli:secrets";
import { check } from "submilli:security";

const API = "https://slack.com/api/";
const FILES_API = "https://files.slack.com/";
// One Slack user ID: a `U` or `W` followed by capital letters and digits.
const USER_ID = /^[UW][A-Z0-9]+$/;

/** A Slack transport or application error with a stable Slack error code. */
export class SlackError extends Error {
    code: string;
    status: number;

    constructor(code: string, message: string, status: number) {
        super(message);
        this.name = "SlackError";
        this.code = code;
        this.status = status;
    }
}

/** Identity represented by `SLACK_USER_TOKEN`. */
export interface SlackIdentity {
    /** Authenticated user ID. */
    userId: string;
    /** Authenticated user's display handle. */
    user: string;
    /** Workspace ID. */
    teamId: string;
    /** Workspace name. */
    team: string;
    /** Workspace URL. */
    url: string;
}

/** Stable coordinates for a Slack message. */
export interface MessageRef {
    /** Conversation containing the message. */
    channelId: string;
    /** Message timestamp, used by Slack as its message ID. */
    ts: string;
    /** Root timestamp when `ts` identifies a thread reply. */
    threadTs?: string;
}

/** Aggregated reaction data returned on a message. */
export interface SlackReaction {
    /** Emoji name without surrounding colons. */
    name: string;
    /** Number of users who reacted. */
    count: number;
    /** Reacting user IDs visible to the caller. */
    users: string[];
}

/** Curated metadata for a Slack-hosted file. */
export interface SlackFile {
    /** File ID. */
    id: string;
    /** Original filename. */
    name: string;
    /** Human title. */
    title: string;
    /** MIME type. */
    mimetype: string;
    /** Slack file-type identifier. */
    filetype: string;
    /** Size in bytes. */
    size: number;
    /** Uploader user ID. */
    user: string;
    /** Browser permalink. */
    permalink: string;
    /** Authenticated private URL. */
    urlPrivate: string;
    /** Authenticated private download URL. */
    urlPrivateDownload: string;
}

interface ApiFile {
    id: string;
    name?: string;
    title?: string;
    mimetype?: string;
    filetype?: string;
    size?: number;
    user?: string;
    permalink?: string;
    url_private?: string;
    url_private_download?: string;
}

/** Curated Slack message data. */
export interface SlackMessage {
    /** Message timestamp/ID. */
    ts: string;
    /** Plain message text. */
    text: string;
    /** Author user ID, empty for some bot messages. */
    user: string;
    /** Author bot ID, empty for user messages. */
    botId: string;
    /** Thread-root timestamp, empty for non-replies. */
    threadTs: string;
    /** Number of thread replies. */
    replyCount: number;
    /** Files attached to the message. */
    files: SlackFile[];
    /** Reactions on the message. */
    reactions: SlackReaction[];
}

interface ApiMessage {
    ts: string;
    text?: string;
    user?: string;
    bot_id?: string;
    thread_ts?: string;
    reply_count?: number;
    files?: ApiFile[];
    reactions?: SlackReaction[];
}

/** Curated conversation metadata. */
export interface SlackChannel {
    /** Conversation ID. */
    id: string;
    /** Conversation name. */
    name: string;
    /** Whether membership is restricted. */
    isPrivate: boolean;
    /** Whether the authenticated user is a member. */
    isMember: boolean;
    /** Whether the conversation is archived. */
    isArchived: boolean;
    /** Topic text. */
    topic: string;
    /** Purpose text. */
    purpose: string;
}

interface ApiChannel {
    id: string;
    name?: string;
    is_private?: boolean;
    is_member?: boolean;
    is_archived?: boolean;
    topic?: { value: string };
    purpose?: { value: string };
}

/** Curated workspace user data. */
export interface SlackUser {
    /** User ID. */
    id: string;
    /** Slack handle. */
    name: string;
    /** Real name. */
    realName: string;
    /** Profile display name. */
    displayName: string;
    /** Email when the token has permission to read it. */
    email: string;
    /** Whether the account is deleted. */
    deleted: boolean;
    /** Whether this account is a bot. */
    isBot: boolean;
}

interface ApiUser {
    id: string;
    name?: string;
    real_name?: string;
    deleted?: boolean;
    is_bot?: boolean;
    profile?: { display_name?: string; real_name?: string; email?: string };
}

/** Cursor pagination accepted by Slack list methods. */
export interface PageOptions {
    /** Maximum items requested. */
    limit?: number;
    /** Cursor returned by the previous page. */
    cursor?: string;
}

/** Channel-history pagination and timestamp bounds. */
export interface MessageListOptions {
    /** Maximum messages requested. */
    limit?: number;
    /** Cursor returned by the previous page. */
    cursor?: string;
    /** Oldest message timestamp to include. */
    oldest?: string;
    /** Latest message timestamp to include. */
    latest?: string;
    /** Include messages exactly on timestamp bounds. */
    inclusive?: boolean;
}

/** One cursor page of messages. */
export interface MessagePage {
    /** Messages on this page. */
    messages: SlackMessage[];
    /** Empty on the last page. */
    nextCursor: string;
}

/** One cursor page of channels. */
export interface ChannelPage {
    /** Channels on this page. */
    channels: SlackChannel[];
    /** Empty on the last page. */
    nextCursor: string;
}

/** One cursor page of users. */
export interface UserPage {
    /** Users on this page. */
    users: SlackUser[];
    /** Empty on the last page. */
    nextCursor: string;
}

/** Slack Real-time Search options. */
export interface SearchOptions {
    /** Results per page, up to Slack's maximum of 20. */
    limit?: number;
    /** Cursor returned by the previous search page. */
    cursor?: string;
    /** Any of messages, files, channels, and users. */
    contentTypes?: string[];
    /** Conversation types to search. */
    channelTypes?: string[];
    /** Channel providing context for the query. */
    contextChannelId?: string;
    /** Include bot-authored messages. */
    includeBots?: boolean;
    /** Include relevant surrounding messages. */
    includeContextMessages?: boolean;
    /** Only results before this Unix timestamp. */
    before?: number;
    /** Only results after this Unix timestamp. */
    after?: number;
    /** Sort field: score or timestamp. */
    sort?: string;
    /** Sort direction: asc or desc. */
    sortDir?: string;
}

/** Message result from Slack Real-time Search. */
export interface SearchMessage {
    /** Conversation ID. */
    channelId: string;
    /** Conversation name. */
    channelName: string;
    /** Result message timestamp/ID. */
    messageTs: string;
    /** Author user ID. */
    authorUserId: string;
    /** Author display name. */
    authorName: string;
    /** Searchable message content. */
    content: string;
    /** Browser permalink. */
    permalink: string;
    /** Whether the author is a bot. */
    isAuthorBot: boolean;
}

/** File result from Slack Real-time Search. */
export interface SearchFile {
    /** File ID. */
    fileId: string;
    /** File title. */
    title: string;
    /** MIME type or Slack file type. */
    fileType: string;
    /** Searchable extracted content. */
    content: string;
    /** Browser permalink. */
    permalink: string;
}

/** Channel result from Slack Real-time Search. */
export interface SearchChannel {
    /** Channel name. */
    name: string;
    /** Topic text. */
    topic: string;
    /** Purpose text. */
    purpose: string;
    /** Browser permalink. */
    permalink: string;
}

/** User result from Slack Real-time Search. */
export interface SearchUser {
    /** User ID. */
    userId: string;
    /** Slack handle. */
    name: string;
    /** Profile display name. */
    displayName: string;
}

/** Mixed page returned by Slack Real-time Search. */
export interface SearchPage {
    /** Matching messages. */
    messages: SearchMessage[];
    /** Matching files. */
    files: SearchFile[];
    /** Matching channels. */
    channels: SearchChannel[];
    /** Matching users. */
    users: SearchUser[];
    /** Empty on the last page. */
    nextCursor: string;
}

/** Input for a user-authored message. */
export interface SendMessageInput {
    /** Target conversation ID. */
    channelId: string;
    /** Message text. */
    text: string;
    /** Thread-root timestamp when replying. */
    threadTs?: string;
    /** Whether Slack should unfurl links. */
    unfurlLinks?: boolean;
    /** Whether Slack should unfurl media. */
    unfurlMedia?: boolean;
}

interface SlackEnvelope {
    ok: boolean;
    error?: string;
}

interface Metadata {
    next_cursor?: string;
}

interface IdentityResponse {
    ok: boolean;
    error?: string;
    user_id: string;
    user: string;
    team_id: string;
    team: string;
    url: string;
}

interface MessagesResponse {
    ok: boolean;
    error?: string;
    messages: ApiMessage[];
    response_metadata?: Metadata;
}

interface MessageResponse {
    ok: boolean;
    error?: string;
    message: ApiMessage;
    channel?: string;
    ts?: string;
}

interface ChannelResponse { ok: boolean; error?: string; channel: ApiChannel; }
interface ChannelsResponse { ok: boolean; error?: string; channels: ApiChannel[]; response_metadata?: Metadata; }
interface UserResponse { ok: boolean; error?: string; user: ApiUser; }
interface UsersResponse { ok: boolean; error?: string; members: ApiUser[]; response_metadata?: Metadata; }
interface FileResponse { ok: boolean; error?: string; file: ApiFile; }

interface SearchResults {
    messages?: ApiSearchMessage[];
    files?: ApiSearchFile[];
    channels?: ApiSearchChannel[];
    users?: ApiSearchUser[];
    response_metadata?: Metadata;
}
interface SearchResponse { ok: boolean; error?: string; results: SearchResults; }

interface ApiSearchMessage {
    channel_id: string;
    channel_name?: string;
    message_ts: string;
    author_user_id?: string;
    author_name?: string;
    content?: string;
    permalink?: string;
    is_author_bot?: boolean;
}

interface ApiSearchFile {
    file_id: string;
    title?: string;
    file_type?: string;
    content?: string;
    permalink?: string;
}

interface ApiSearchChannel { name: string; topic?: string; purpose?: string; permalink?: string; }
interface ApiSearchUser { user_id: string; name?: string; display_name?: string; }

/**
 * Return the Slack user and workspace represented by `SLACK_USER_TOKEN`.
 * Use this to verify a per-user OAuth binding before performing other operations.
 * @capability slack.com/user/getIdentity {}
 */
export function getIdentity(): SlackIdentity {
    check("slack.com/user/getIdentity", {});
    const data = slackPost("auth.test", {}).json() as IdentityResponse;
    requireOk(data, 200);
    return { userId: data.user_id, user: data.user, teamId: data.team_id, team: data.team, url: data.url };
}

/**
 * Search Slack as the authenticated user and return mixed message, file, channel,
 * and user results. Availability and visible results follow the workspace's Slack
 * search features, the token's search scopes, and the user's own access.
 * @param query Natural-language or keyword search query.
 * @param options Optional result types, context channel, time bounds, sort, and cursor.
 * @capability slack.com/user/search {}
 */
export function search(query: string, options: SearchOptions | null = null): SearchPage {
    const limit = options === null ? null : options.limit;
    const cursor = options === null ? null : options.cursor;
    const requestedContentTypes = options === null ? null : options.contentTypes;
    const requestedChannelTypes = options === null ? null : options.channelTypes;
    const contextChannelId = options === null ? null : options.contextChannelId;
    const includeBots = options === null ? null : options.includeBots;
    const includeContext = options === null ? null : options.includeContextMessages;
    const before = options === null ? null : options.before;
    const after = options === null ? null : options.after;
    const sort = options === null ? null : options.sort;
    const sortDir = options === null ? null : options.sortDir;
    let contentTypes: string[] | null = null;
    if (requestedContentTypes !== null) {
        const copied: string[] = [];
        for (const contentType of requestedContentTypes) copied.push(contentType);
        contentTypes = copied;
    }
    let channelTypes: string[] | null = null;
    if (requestedChannelTypes !== null) {
        const copied: string[] = [];
        for (const channelType of requestedChannelTypes) copied.push(channelType);
        channelTypes = copied;
    }
    check("slack.com/user/search", {});
    const body: SearchRequest = { query: query };
    if (contentTypes !== null) body.content_types = contentTypes;
    if (channelTypes !== null) body.channel_types = channelTypes;
    if (limit !== null) body.limit = limit;
    if (cursor !== null) body.cursor = cursor;
    if (contextChannelId !== null) body.context_channel_id = contextChannelId;
    if (includeBots !== null) body.include_bots = includeBots;
    if (includeContext !== null) body.include_context_messages = includeContext;
    if (before !== null) body.before = before;
    if (after !== null) body.after = after;
    if (sort !== null) body.sort = sort;
    if (sortDir !== null) body.sort_dir = sortDir;
    const data = slackPost("assistant.search.context", body).json() as SearchResponse;
    requireOk(data, 200);
    const results = data.results;
    const foundMessages: SearchMessage[] = [];
    const foundFiles: SearchFile[] = [];
    const foundChannels: SearchChannel[] = [];
    const foundUsers: SearchUser[] = [];
    if (results.messages !== null) {
        for (const item of results.messages) {
            foundMessages.push({
                channelId: item.channel_id, channelName: str(item.channel_name),
                messageTs: item.message_ts, authorUserId: str(item.author_user_id),
                authorName: str(item.author_name), content: str(item.content),
                permalink: str(item.permalink), isAuthorBot: bool(item.is_author_bot),
            });
        }
    }
    if (results.files !== null) {
        for (const item of results.files) {
            foundFiles.push({
                fileId: item.file_id, title: str(item.title), fileType: str(item.file_type),
                content: str(item.content), permalink: str(item.permalink),
            });
        }
    }
    if (results.channels !== null) {
        for (const item of results.channels) {
            foundChannels.push({
                name: item.name, topic: str(item.topic), purpose: str(item.purpose),
                permalink: str(item.permalink),
            });
        }
    }
    if (results.users !== null) {
        for (const item of results.users) {
            foundUsers.push({
                userId: item.user_id, name: str(item.name), displayName: str(item.display_name),
            });
        }
    }
    return {
        messages: foundMessages,
        files: foundFiles,
        channels: foundChannels,
        users: foundUsers,
        nextCursor: cursorOf(results.response_metadata),
    };
}

interface SearchRequest {
    query: string;
    limit?: number;
    cursor?: string;
    content_types?: string[];
    channel_types?: string[];
    context_channel_id?: string;
    include_bots?: boolean;
    include_context_messages?: boolean;
    before?: number;
    after?: number;
    sort?: string;
    sort_dir?: string;
}

/**
 * Fetch one message by channel and Slack timestamp, returning null when absent.
 * When reading a thread reply, set `ref.threadTs` to the root message timestamp.
 * @param ref Stable Slack coordinates for the root message or reply.
 * @capability slack.com/user/getMessage { channelId: string }
 */
export function getMessage(ref: MessageRef): SlackMessage | null {
    const { channelId, ts, threadTs } = ref;
    check("slack.com/user/getMessage", { channelId: channelId });
    let rootTs = ts;
    if (threadTs !== null) rootTs = threadTs;
    const data = messages("conversations.replies", { channel: channelId, ts: rootTs, limit: 100 });
    for (const item of data.messages) {
        if (item.ts === ts) return messageFrom(item);
    }
    return null;
}

/**
 * Return one cursor page of messages visible to the authenticated user.
 * Public-channel and private-conversation access follows the user's Slack access.
 * @param channelId Conversation whose history should be read.
 * @param options Optional page size, cursor, and Slack timestamp bounds.
 * @capability slack.com/user/listMessages { channelId: string }
 */
export function listMessages(channelId: string, options: MessageListOptions | null = null): MessagePage {
    const limit = options === null ? null : options.limit;
    const cursor = options === null ? null : options.cursor;
    const oldest = options === null ? null : options.oldest;
    const latest = options === null ? null : options.latest;
    const inclusive = options === null ? null : options.inclusive;
    check("slack.com/user/listMessages", { channelId: channelId });
    const body: HistoryRequest = { channel: channelId };
    applyMessageOptions(body, {
        limit: limit,
        cursor: cursor,
        oldest: oldest,
        latest: latest,
        inclusive: inclusive,
    });
    return messagePage(messages("conversations.history", body));
}

/**
 * Return one cursor page containing a thread root and its replies.
 * @param channelId Conversation containing the thread.
 * @param threadTs Timestamp of the thread's root message.
 * @param page Optional page size and continuation cursor.
 * @capability slack.com/user/getThread { channelId: string }
 */
export function getThread(channelId: string, threadTs: string, page: PageOptions | null = null): MessagePage {
    const limit = page === null ? null : page.limit;
    const cursor = page === null ? null : page.cursor;
    check("slack.com/user/getThread", { channelId: channelId });
    const body: HistoryRequest = { channel: channelId, ts: threadTs };
    applyPage(body, limit, cursor);
    return messagePage(messages("conversations.replies", body));
}

/**
 * Post a message as the authenticated user and return Slack's stored message.
 * Set `threadTs` to reply in a thread. The action is attributed to the user whose
 * OAuth token is bound to the session.
 * @param input Message text, destination, and optional thread/unfurl settings.
 * @capability slack.com/user/sendMessage { channelId: string }
 */
export function sendMessage(input: SendMessageInput): SlackMessage {
    const { channelId, text, threadTs, unfurlLinks, unfurlMedia } = input;
    check("slack.com/user/sendMessage", { channelId: channelId });
    const body: SendRequest = { channel: channelId, text: text };
    if (threadTs !== null) body.thread_ts = threadTs;
    if (unfurlLinks !== null) body.unfurl_links = unfurlLinks;
    if (unfurlMedia !== null) body.unfurl_media = unfurlMedia;
    const data = slackPost("chat.postMessage", body).json() as MessageResponse;
    requireOk(data, 200);
    return messageFrom(data.message);
}

/**
 * Open or resume a 1:1 DM with another workspace user, send text as the
 * authenticated user, and return Slack's stored message. Do not pass the
 * authenticated user's own ID.
 * Requires Slack's `im:write` and `chat:write` user-token scopes.
 * @param userId Workspace user ID that should receive the message.
 * @param text Message text.
 * @capability slack.com/user/sendDirectMessage { userId: string }
 */
export function sendDirectMessage(userId: string, text: string): SlackMessage {
    const recipientId = singleUserId(userId);
    check("slack.com/user/sendDirectMessage", { userId: recipientId });
    const opened = slackPost("conversations.open", { users: recipientId }).json() as ChannelResponse;
    requireOk(opened, 200);
    const data = slackPost("chat.postMessage", { channel: opened.channel.id, text: text }).json() as MessageResponse;
    requireOk(data, 200);
    return messageFrom(data.message);
}

/**
 * Open or resume a multi-person direct message with 2–8 other workspace users,
 * send text as the authenticated user, and return Slack's stored message. Do
 * not include the authenticated user's own ID.
 * Requires Slack's `mpim:write` and `chat:write` user-token scopes.
 * @param userIds Workspace user IDs that should receive the message.
 * @param text Message text.
 * @capability slack.com/user/sendGroupDirectMessage { userIds }
 */
export function sendGroupDirectMessage(userIds: string[], text: string): SlackMessage {
    const participantIds: string[] = [];
    for (const userId of userIds) participantIds.push(singleUserId(userId));
    check("slack.com/user/sendGroupDirectMessage", { userIds: participantIds });
    const opened = slackPost("conversations.open", { users: participantIds.join(",") }).json() as ChannelResponse;
    requireOk(opened, 200);
    const data = slackPost("chat.postMessage", { channel: opened.channel.id, text: text }).json() as MessageResponse;
    requireOk(data, 200);
    return messageFrom(data.message);
}

interface SendRequest { channel: string; text: string; thread_ts?: string; unfurl_links?: boolean; unfurl_media?: boolean; }

/**
 * Add an emoji reaction as the authenticated user.
 * @param ref Channel and timestamp of the target message.
 * @param emoji Slack emoji name without surrounding colons.
 * @capability slack.com/user/addReaction { channelId: string }
 */
export function addReaction(ref: MessageRef, emoji: string): void {
    const { channelId, ts } = ref;
    check("slack.com/user/addReaction", { channelId: channelId });
    const data = slackPost("reactions.add", { channel: channelId, timestamp: ts, name: emoji }).json() as SlackEnvelope;
    requireOk(data, 200);
}

/**
 * Fetch metadata for one channel, private group, or direct-message conversation.
 * @param channelId Slack conversation ID.
 * @capability slack.com/user/getChannel { channelId: string }
 */
export function getChannel(channelId: string): SlackChannel {
    check("slack.com/user/getChannel", { channelId: channelId });
    const query = new Map<string, string>();
    query.set("channel", channelId);
    const data = slackGet("conversations.info", query).json() as ChannelResponse;
    requireOk(data, 200);
    return channelFrom(data.channel);
}

/**
 * List channels, private groups, DMs, and group DMs visible to the user.
 * @param page Optional page size and continuation cursor.
 * @capability slack.com/user/listChannels {}
 */
export function listChannels(page: PageOptions | null = null): ChannelPage {
    const limit = page === null ? null : page.limit;
    const cursor = page === null ? null : page.cursor;
    check("slack.com/user/listChannels", {});
    const body: PageRequest = { types: "public_channel,private_channel,mpim,im" };
    applyPage(body, limit, cursor);
    const data = slackGet("conversations.list", pageQuery(body)).json() as ChannelsResponse;
    requireOk(data, 200);
    const channels: SlackChannel[] = [];
    for (const item of data.channels) channels.push(channelFrom(item));
    return { channels: channels, nextCursor: cursorOf(data.response_metadata) };
}

/**
 * Fetch one workspace user by Slack user ID.
 * @param userId Slack user ID, such as `U012ABCDEF`.
 * @capability slack.com/user/getUser { userId: string }
 */
export function getUser(userId: string): SlackUser {
    check("slack.com/user/getUser", { userId: userId });
    const query = new Map<string, string>();
    query.set("user", userId);
    const data = slackGet("users.info", query).json() as UserResponse;
    requireOk(data, 200);
    return userFrom(data.user);
}

/**
 * List workspace users visible to the authenticated user, including bots and
 * deactivated accounts.
 * @param page Optional page size and continuation cursor.
 * @capability slack.com/user/listUsers {}
 */
export function listUsers(page: PageOptions | null = null): UserPage {
    const limit = page === null ? null : page.limit;
    const cursor = page === null ? null : page.cursor;
    check("slack.com/user/listUsers", {});
    const body: PageRequest = {};
    applyPage(body, limit, cursor);
    const data = slackGet("users.list", pageQuery(body)).json() as UsersResponse;
    requireOk(data, 200);
    const users: SlackUser[] = [];
    for (const item of data.members) users.push(userFrom(item));
    return { users: users, nextCursor: cursorOf(data.response_metadata) };
}

/**
 * Find a workspace user by their registered email address.
 * Requires Slack's `users:read.email` user-token scope and throws when no user matches.
 * @param email Exact workspace email address.
 * @capability slack.com/user/findUserByEmail { email: string }
 */
export function findUserByEmail(email: string): SlackUser {
    check("slack.com/user/findUserByEmail", { email: email });
    const query = new Map<string, string>();
    query.set("email", email);
    const data = slackGet("users.lookupByEmail", query).json() as UserResponse;
    requireOk(data, 200);
    return userFrom(data.user);
}

/**
 * Fetch metadata and authenticated private URLs for a Slack-hosted file.
 * @param fileId Slack file ID, such as `F012ABCDEF`.
 * @capability slack.com/user/getFile {}
 */
export function getFile(fileId: string): SlackFile {
    check("slack.com/user/getFile", {});
    return fetchFile(fileId);
}

/**
 * Stream a Slack-hosted private file into the Submilli VFS.
 * The package rejects download URLs outside `files.slack.com`; the destination is
 * not overwritten unless the underlying download policy permits it.
 * @param fileId Slack file ID to download.
 * @param path Destination path in the session VFS.
 * @capability slack.com/user/downloadFile { path: string }
 */
export function downloadFile(fileId: string, path: string): DownloadResult {
    check("slack.com/user/downloadFile", { path: path });
    const file = fetchFile(fileId);
    if (file.urlPrivateDownload === "" || !file.urlPrivateDownload.startsWith(FILES_API)) {
        throw new SlackError("unsafe_file_url", "Slack returned a file URL outside files.slack.com", 200);
    }
    const headers = authHeaders();
    const options: DownloadOptions = { headers: headers };
    return download(FILES_API + file.urlPrivateDownload.slice(FILES_API.length), path, options);
}

function fetchFile(fileId: string): SlackFile {
    const query = new Map<string, string>();
    query.set("file", fileId);
    const data = slackGet("files.info", query).json() as FileResponse;
    requireOk(data, 200);
    return fileFrom(data.file);
}

interface PageRequest { limit?: number; cursor?: string; types?: string; }
interface HistoryRequest { channel: string; ts?: string; limit?: number; cursor?: string; oldest?: string; latest?: string; inclusive?: boolean; }

function applyPage(body: PageRequest, limit: number | null, cursor: string | null): void {
    if (limit !== null) body.limit = limit;
    if (cursor !== null) body.cursor = cursor;
}

interface MessageListFields {
    limit: number | null;
    cursor: string | null;
    oldest: string | null;
    latest: string | null;
    inclusive: boolean | null;
}

function applyMessageOptions(body: HistoryRequest, fields: MessageListFields): void {
    const { limit, cursor, oldest, latest, inclusive } = fields;
    applyPage(body, limit, cursor);
    if (oldest !== null) body.oldest = oldest;
    if (latest !== null) body.latest = latest;
    if (inclusive !== null) body.inclusive = inclusive;
}

function messages(method: string, body: HistoryRequest): MessagesResponse {
    const query = pageQuery(body);
    query.set("channel", body.channel);
    const ts = body.ts;
    const oldest = body.oldest;
    const latest = body.latest;
    const inclusive = body.inclusive;
    if (ts !== null) query.set("ts", ts);
    if (oldest !== null) query.set("oldest", oldest);
    if (latest !== null) query.set("latest", latest);
    if (inclusive !== null) query.set("inclusive", inclusive.toString());
    const data = slackGet(method, query).json() as MessagesResponse;
    requireOk(data, 200);
    return data;
}

function messagePage(data: MessagesResponse): MessagePage {
    const out: SlackMessage[] = [];
    for (const item of data.messages) out.push(messageFrom(item));
    return { messages: out, nextCursor: cursorOf(data.response_metadata) };
}

function slackPost(method: string, body: {}): Response {
    const response = post(API + method, body, authHeaders());
    return requireHttpOk(response);
}

function slackGet(method: string, query: Map<string, string>): Response {
    const encoded = encodeQuery(query);
    const suffix = encoded.length > 0 ? "?" + encoded : "";
    const response = get(API + method + suffix, authHeaders());
    return requireHttpOk(response);
}

function requireHttpOk(response: Response): Response {
    if (!response.ok) {
        throw new SlackError("http_error", `Slack request failed: HTTP ${response.status} ${response.statusText}`, response.status);
    }
    requireOk(response.json() as SlackEnvelope, response.status);
    return response;
}

function pageQuery(page: PageRequest): Map<string, string> {
    const query = new Map<string, string>();
    const limit = page.limit;
    const cursor = page.cursor;
    const types = page.types;
    if (limit !== null) query.set("limit", limit.toString());
    if (cursor !== null) query.set("cursor", cursor);
    if (types !== null) query.set("types", types);
    return query;
}

function authHeaders(): Map<string, string> {
    const token = secrets.get("SLACK_USER_TOKEN");
    if (token === null) throw new SlackError("missing_token", "SLACK_USER_TOKEN is not bound", 200);
    const headers = new Map<string, string>();
    headers.set("Authorization", `Bearer ${token}`);
    return headers;
}

// `conversations.open` reads `users` as a comma-separated list, so a value that is not exactly one
// ID would address users the check never saw.
function singleUserId(value: string): string {
    if (!USER_ID.test(value)) {
        throw new SlackError("invalid_user_id", "user ID must be one Slack user ID, such as U012ABCDEF", 0);
    }
    return value;
}

function requireOk(envelope: SlackEnvelope, status: number): void {
    if (!envelope.ok) {
        const code = envelope.error !== null ? envelope.error : "unknown_error";
        throw new SlackError(code, `Slack API error: ${code}`, status);
    }
}

function messageFrom(item: ApiMessage): SlackMessage {
    const files: SlackFile[] = [];
    if (item.files !== null) for (const file of item.files) files.push(fileFrom(file));
    return {
        ts: item.ts,
        text: str(item.text),
        user: str(item.user),
        botId: str(item.bot_id),
        threadTs: str(item.thread_ts),
        replyCount: num(item.reply_count),
        files: files,
        reactions: item.reactions !== null ? item.reactions : [],
    };
}

function fileFrom(file: ApiFile): SlackFile {
    return {
        id: file.id, name: str(file.name), title: str(file.title), mimetype: str(file.mimetype),
        filetype: str(file.filetype), size: num(file.size), user: str(file.user),
        permalink: str(file.permalink), urlPrivate: str(file.url_private),
        urlPrivateDownload: str(file.url_private_download),
    };
}

function channelFrom(channel: ApiChannel): SlackChannel {
    return {
        id: channel.id, name: str(channel.name), isPrivate: bool(channel.is_private),
        isMember: bool(channel.is_member), isArchived: bool(channel.is_archived),
        topic: channel.topic !== null ? channel.topic.value : "",
        purpose: channel.purpose !== null ? channel.purpose.value : "",
    };
}

function userFrom(user: ApiUser): SlackUser {
    const profile = user.profile;
    return {
        id: user.id, name: str(user.name), realName: str(user.real_name),
        displayName: profile !== null ? str(profile.display_name) : "",
        email: profile !== null ? str(profile.email) : "", deleted: bool(user.deleted), isBot: bool(user.is_bot),
    };
}

function cursorOf(metadata: Metadata | null): string {
    return metadata !== null ? str(metadata.next_cursor) : "";
}

function str(value: string | null): string { return value !== null ? value : ""; }
function num(value: number | null): number { return value !== null ? value : 0; }
function bool(value: boolean | null): boolean { return value === true; }
