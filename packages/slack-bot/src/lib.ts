import { get, post, Response } from "submilli:http";
import { encodeQuery } from "submilli:url";
import secrets from "submilli:secrets";
import { check } from "submilli:security";

const API = "https://slack.com/api/";

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

/** Identity represented by `SLACK_BOT_TOKEN`. */
export interface SlackIdentity {
    /** Bot user ID. */
    userId: string;
    /** Bot user handle. */
    user: string;
    /** Bot installation ID when returned by Slack. */
    botId: string;
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
    /** Message timestamp/ID. */
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
    /** Reacting user IDs visible to the bot. */
    users: string[];
}

/** Curated Slack message data. */
export interface SlackMessage {
    /** Message timestamp/ID. */
    ts: string;
    /** Plain message text. */
    text: string;
    /** Author user ID. */
    user: string;
    /** Author bot ID, empty for user messages. */
    botId: string;
    /** Thread-root timestamp, empty for non-replies. */
    threadTs: string;
    /** Number of thread replies. */
    replyCount: number;
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
    reactions?: SlackReaction[];
}

/** Curated channel, group, or direct-message metadata. */
export interface SlackConversation {
    /** Conversation ID. */
    id: string;
    /** Conversation name, empty for some direct messages. */
    name: string;
    /** Whether membership is restricted. */
    isPrivate: boolean;
    /** Whether the bot is a member. */
    isMember: boolean;
    /** Whether the conversation is archived. */
    isArchived: boolean;
    /** Whether this is a direct message. */
    isIm: boolean;
    /** Whether this is a multi-person direct message. */
    isMpim: boolean;
    /** Topic text. */
    topic: string;
    /** Purpose text. */
    purpose: string;
}

interface ApiConversation {
    id: string;
    name?: string;
    is_private?: boolean;
    is_member?: boolean;
    is_archived?: boolean;
    is_im?: boolean;
    is_mpim?: boolean;
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
    /** Email when the bot has permission to read it. */
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
    profile?: { display_name?: string; email?: string };
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

/** One cursor page of conversations. */
export interface ConversationPage {
    /** Conversations on this page. */
    conversations: SlackConversation[];
    /** Empty on the last page. */
    nextCursor: string;
}

/** One cursor page of conversation member IDs. */
export interface MemberPage {
    /** Member user IDs. */
    memberIds: string[];
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

/** Input for a bot-authored message. */
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

interface SlackEnvelope { ok: boolean; error?: string; }
interface Metadata { next_cursor?: string; }
interface IdentityResponse { ok: boolean; error?: string; user_id: string; user: string; bot_id?: string; team_id: string; team: string; url: string; }
interface MessagesResponse { ok: boolean; error?: string; messages: ApiMessage[]; response_metadata?: Metadata; }
interface MessageResponse { ok: boolean; error?: string; message: ApiMessage; }
interface UpdateResponse { ok: boolean; error?: string; channel: string; ts: string; text: string; }
interface ConversationResponse { ok: boolean; error?: string; channel: ApiConversation; }
interface ConversationsResponse { ok: boolean; error?: string; channels: ApiConversation[]; response_metadata?: Metadata; }
interface MembersResponse { ok: boolean; error?: string; members: string[]; response_metadata?: Metadata; }
interface UserResponse { ok: boolean; error?: string; user: ApiUser; }
interface UsersResponse { ok: boolean; error?: string; members: ApiUser[]; response_metadata?: Metadata; }

/**
 * Return the bot user and workspace represented by `SLACK_BOT_TOKEN`.
 * Use this to verify an installation and obtain the bot's Slack user ID.
 * @capability slack.com/bot/getIdentity {}
 */
export function getIdentity(): SlackIdentity {
    check("slack.com/bot/getIdentity", {});
    const data = slackPost("auth.test", {}).json() as IdentityResponse;
    requireOk(data, 200);
    return {
        userId: data.user_id, user: data.user, botId: str(data.bot_id),
        teamId: data.team_id, team: data.team, url: data.url,
    };
}

/**
 * Post a message as the bot and return Slack's stored message.
 * Set `threadTs` to reply in a thread. The bot must be allowed to post in the
 * target conversation, which normally means it must already be a member.
 * @param input Message text, destination, and optional thread/unfurl settings.
 * @capability slack.com/bot/sendMessage { channelId: string }
 */
export function sendMessage(input: SendMessageInput): SlackMessage {
    const { channelId, text, threadTs, unfurlLinks, unfurlMedia } = input;
    check("slack.com/bot/sendMessage", { channelId: channelId });
    const body: MessageWriteRequest = { channel: channelId, text: text };
    if (threadTs !== null) body.thread_ts = threadTs;
    if (unfurlLinks !== null) body.unfurl_links = unfurlLinks;
    if (unfurlMedia !== null) body.unfurl_media = unfurlMedia;
    const data = slackPost("chat.postMessage", body).json() as MessageResponse;
    requireOk(data, 200);
    return messageFrom(data.message);
}

/**
 * Open or resume a 1:1 DM with a workspace user, send text as the bot, and
 * return Slack's stored message. The recipient must not be the bot itself.
 * Requires Slack's `im:write` and `chat:write` bot-token scopes.
 * @param userId Workspace user ID that should receive the message.
 * @param text Message text.
 * @capability slack.com/bot/sendDirectMessage { userId: string }
 */
export function sendDirectMessage(userId: string, text: string): SlackMessage {
    check("slack.com/bot/sendDirectMessage", { userId: userId });
    const opened = slackPost("conversations.open", { users: userId }).json() as ConversationResponse;
    requireOk(opened, 200);
    const data = slackPost("chat.postMessage", { channel: opened.channel.id, text: text }).json() as MessageResponse;
    requireOk(data, 200);
    return messageFrom(data.message);
}

/**
 * Open or resume a multi-person direct message with 2–8 workspace users, send
 * text as the bot, and return Slack's stored message. Do not include the bot's
 * own user ID.
 * Requires Slack's `mpim:write` and `chat:write` bot-token scopes.
 * @param userIds Workspace user IDs that should receive the message.
 * @param text Message text.
 * @capability slack.com/bot/sendGroupDirectMessage { userIds }
 */
export function sendGroupDirectMessage(userIds: string[], text: string): SlackMessage {
    const participantIds: string[] = [];
    for (const userId of userIds) participantIds.push(userId);
    check("slack.com/bot/sendGroupDirectMessage", { userIds: participantIds });
    const opened = slackPost("conversations.open", { users: participantIds.join(",") }).json() as ConversationResponse;
    requireOk(opened, 200);
    const data = slackPost("chat.postMessage", { channel: opened.channel.id, text: text }).json() as MessageResponse;
    requireOk(data, 200);
    return messageFrom(data.message);
}

/**
 * Replace the text of a message authored by this bot and return the refreshed message.
 * Slack does not allow a bot token to edit messages authored by another user or app.
 * @param ref Channel and timestamp of the bot-authored message.
 * @param text Replacement message text.
 * @capability slack.com/bot/updateMessage { channelId: string }
 */
export function updateMessage(ref: MessageRef, text: string): SlackMessage {
    const { channelId, ts } = ref;
    check("slack.com/bot/updateMessage", { channelId: channelId });
    const data = slackPost("chat.update", { channel: channelId, ts: ts, text: text }).json() as UpdateResponse;
    requireOk(data, 200);
    const refreshed = messages("conversations.replies", { channel: data.channel, ts: data.ts, limit: 1 });
    for (const message of refreshed.messages) {
        if (message.ts === data.ts) return messageFrom(message);
    }
    throw new SlackError("message_not_found", "Slack updated the message but did not return it on refresh", 200);
}

/**
 * Permanently delete a message authored by this bot.
 * @param ref Channel and timestamp of the bot-authored message.
 * @capability slack.com/bot/deleteMessage { channelId: string }
 */
export function deleteMessage(ref: MessageRef): void {
    const { channelId, ts } = ref;
    check("slack.com/bot/deleteMessage", { channelId: channelId });
    const data = slackPost("chat.delete", { channel: channelId, ts: ts }).json() as SlackEnvelope;
    requireOk(data, 200);
}

interface MessageWriteRequest {
    channel: string;
    text: string;
    thread_ts?: string;
    unfurl_links?: boolean;
    unfurl_media?: boolean;
}

/**
 * Fetch one message by channel and Slack timestamp, returning null when absent.
 * When reading a thread reply, set `ref.threadTs` to the root message timestamp.
 * @param ref Stable Slack coordinates for the root message or reply.
 * @capability slack.com/bot/getMessage { channelId: string }
 */
export function getMessage(ref: MessageRef): SlackMessage | null {
    const { channelId, ts, threadTs } = ref;
    check("slack.com/bot/getMessage", { channelId: channelId });
    let rootTs = ts;
    if (threadTs !== null) rootTs = threadTs;
    const data = messages("conversations.replies", { channel: channelId, ts: rootTs, limit: 100 });
    for (const item of data.messages) if (item.ts === ts) return messageFrom(item);
    return null;
}

/**
 * Return one cursor page of messages from a conversation the bot can read.
 * Timestamp bounds are Slack timestamps represented as strings.
 * @param channelId Conversation whose history should be read.
 * @param options Optional page size, cursor, and timestamp bounds.
 * @capability slack.com/bot/listMessages { channelId: string }
 */
export function listMessages(channelId: string, options: MessageListOptions | null = null): MessagePage {
    const limit = options === null ? null : options.limit;
    const cursor = options === null ? null : options.cursor;
    const oldest = options === null ? null : options.oldest;
    const latest = options === null ? null : options.latest;
    const inclusive = options === null ? null : options.inclusive;
    check("slack.com/bot/listMessages", { channelId: channelId });
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
 * @capability slack.com/bot/getThread { channelId: string }
 */
export function getThread(channelId: string, threadTs: string, page: PageOptions | null = null): MessagePage {
    const limit = page === null ? null : page.limit;
    const cursor = page === null ? null : page.cursor;
    check("slack.com/bot/getThread", { channelId: channelId });
    const body: HistoryRequest = { channel: channelId, ts: threadTs };
    applyPage(body, limit, cursor);
    return messagePage(messages("conversations.replies", body));
}

/**
 * Fetch metadata for one channel, private group, or direct-message conversation.
 * Visibility follows the bot token's scopes and conversation membership.
 * @param channelId Slack conversation ID.
 * @capability slack.com/bot/getConversation { channelId: string }
 */
export function getConversation(channelId: string): SlackConversation {
    check("slack.com/bot/getConversation", { channelId: channelId });
    const query = new Map<string, string>();
    query.set("channel", channelId);
    const data = slackGet("conversations.info", query).json() as ConversationResponse;
    requireOk(data, 200);
    return conversationFrom(data.channel);
}

/**
 * List channels, private groups, DMs, and group DMs visible to the bot token.
 * A returned public channel is not necessarily one the bot has joined.
 * @param page Optional page size and continuation cursor.
 * @capability slack.com/bot/listConversations {}
 */
export function listConversations(page: PageOptions | null = null): ConversationPage {
    const limit = page === null ? null : page.limit;
    const cursor = page === null ? null : page.cursor;
    check("slack.com/bot/listConversations", {});
    const body: PageRequest = { types: "public_channel,private_channel,mpim,im" };
    applyPage(body, limit, cursor);
    const data = slackGet("conversations.list", pageQuery(body)).json() as ConversationsResponse;
    requireOk(data, 200);
    const conversations: SlackConversation[] = [];
    for (const item of data.channels) conversations.push(conversationFrom(item));
    return { conversations: conversations, nextCursor: cursorOf(data.response_metadata) };
}

/**
 * List the Slack user IDs belonging to a conversation.
 * @param channelId Conversation whose membership should be read.
 * @param page Optional page size and continuation cursor.
 * @capability slack.com/bot/listMembers { channelId: string }
 */
export function listMembers(channelId: string, page: PageOptions | null = null): MemberPage {
    const limit = page === null ? null : page.limit;
    const cursor = page === null ? null : page.cursor;
    check("slack.com/bot/listMembers", { channelId: channelId });
    const body: MembersRequest = { channel: channelId };
    applyPage(body, limit, cursor);
    const query = pageQuery(body);
    query.set("channel", body.channel);
    const data = slackGet("conversations.members", query).json() as MembersResponse;
    requireOk(data, 200);
    return { memberIds: data.members, nextCursor: cursorOf(data.response_metadata) };
}

interface MembersRequest { channel: string; limit?: number; cursor?: string; }

/**
 * Open or resume a 1:1 direct-message conversation without sending a message.
 * Requires Slack's `im:write` bot-token scope.
 * @param userId Workspace user ID to include, excluding the bot itself.
 * @capability slack.com/bot/openDirectMessage { userId }
 */
export function openDirectMessage(userId: string): SlackConversation {
    check("slack.com/bot/openDirectMessage", { userId: userId });
    const data = slackPost("conversations.open", { users: userId }).json() as ConversationResponse;
    requireOk(data, 200);
    return conversationFrom(data.channel);
}

/**
 * Open or resume a multi-person direct message without sending a message.
 * Pass 2–8 workspace user IDs and do not include the bot itself.
 * Requires Slack's `mpim:write` bot-token scope.
 * @param userIds Workspace user IDs to include, excluding the bot itself.
 * @capability slack.com/bot/openGroupDirectMessage { userIds }
 */
export function openGroupDirectMessage(userIds: string[]): SlackConversation {
    const participantIds: string[] = [];
    for (const userId of userIds) participantIds.push(userId);
    check("slack.com/bot/openGroupDirectMessage", { userIds: participantIds });
    const data = slackPost("conversations.open", { users: participantIds.join(",") }).json() as ConversationResponse;
    requireOk(data, 200);
    return conversationFrom(data.channel);
}

/**
 * Fetch one workspace user by Slack user ID.
 * @param userId Slack user ID, such as `U012ABCDEF`.
 * @capability slack.com/bot/getUser { userId: string }
 */
export function getUser(userId: string): SlackUser {
    check("slack.com/bot/getUser", { userId: userId });
    const query = new Map<string, string>();
    query.set("user", userId);
    const data = slackGet("users.info", query).json() as UserResponse;
    requireOk(data, 200);
    return userFrom(data.user);
}

/**
 * List workspace users visible to the bot token, including deactivated users and bots.
 * @param page Optional page size and continuation cursor.
 * @capability slack.com/bot/listUsers {}
 */
export function listUsers(page: PageOptions | null = null): UserPage {
    const limit = page === null ? null : page.limit;
    const cursor = page === null ? null : page.cursor;
    check("slack.com/bot/listUsers", {});
    const body: PageRequest = {};
    applyPage(body, limit, cursor);
    const data = slackGet("users.list", pageQuery(body)).json() as UsersResponse;
    requireOk(data, 200);
    const users: SlackUser[] = [];
    for (const item of data.members) users.push(userFrom(item));
    return { users: users, nextCursor: cursorOf(data.response_metadata) };
}

/**
 * Add an emoji reaction as the bot.
 * @param ref Channel and timestamp of the target message.
 * @param emoji Slack emoji name without surrounding colons.
 * @capability slack.com/bot/addReaction { channelId: string }
 */
export function addReaction(ref: MessageRef, emoji: string): void {
    const { channelId, ts } = ref;
    check("slack.com/bot/addReaction", { channelId: channelId });
    reaction("reactions.add", channelId, ts, emoji);
}

/**
 * Remove this bot's emoji reaction from a message.
 * @param ref Channel and timestamp of the target message.
 * @param emoji Slack emoji name without surrounding colons.
 * @capability slack.com/bot/removeReaction { channelId: string }
 */
export function removeReaction(ref: MessageRef, emoji: string): void {
    const { channelId, ts } = ref;
    check("slack.com/bot/removeReaction", { channelId: channelId });
    reaction("reactions.remove", channelId, ts, emoji);
}

function reaction(method: string, channelId: string, ts: string, emoji: string): void {
    const data = slackPost(method, { channel: channelId, timestamp: ts, name: emoji }).json() as SlackEnvelope;
    requireOk(data, 200);
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
    const token = secrets.get("SLACK_BOT_TOKEN");
    if (token === null) throw new SlackError("missing_token", "SLACK_BOT_TOKEN is not bound", 200);
    const headers = new Map<string, string>();
    headers.set("Authorization", `Bearer ${token}`);
    return headers;
}

function requireOk(envelope: SlackEnvelope, status: number): void {
    if (!envelope.ok) {
        const code = envelope.error !== null ? envelope.error : "unknown_error";
        throw new SlackError(code, `Slack API error: ${code}`, status);
    }
}

function messageFrom(item: ApiMessage): SlackMessage {
    return {
        ts: item.ts, text: str(item.text), user: str(item.user), botId: str(item.bot_id),
        threadTs: str(item.thread_ts), replyCount: num(item.reply_count),
        reactions: item.reactions !== null ? item.reactions : [],
    };
}

function conversationFrom(channel: ApiConversation): SlackConversation {
    return {
        id: channel.id, name: str(channel.name), isPrivate: bool(channel.is_private),
        isMember: bool(channel.is_member), isArchived: bool(channel.is_archived),
        isIm: bool(channel.is_im), isMpim: bool(channel.is_mpim),
        topic: channel.topic !== null ? channel.topic.value : "",
        purpose: channel.purpose !== null ? channel.purpose.value : "",
    };
}

function userFrom(user: ApiUser): SlackUser {
    const profile = user.profile;
    return {
        id: user.id, name: str(user.name), realName: str(user.real_name),
        displayName: profile !== null ? str(profile.display_name) : "",
        email: profile !== null ? str(profile.email) : "",
        deleted: bool(user.deleted), isBot: bool(user.is_bot),
    };
}

function cursorOf(metadata: Metadata | null): string { return metadata !== null ? str(metadata.next_cursor) : ""; }
function str(value: string | null): string { return value !== null ? value : ""; }
function num(value: number | null): number { return value !== null ? value : 0; }
function bool(value: boolean | null): boolean { return value === true; }
