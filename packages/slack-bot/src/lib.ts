import { get, post, Response } from "submilli:http";
import { encodeQuery } from "submilli:url";
import secrets from "submilli:secrets";
import { check } from "submilli:security";

const API = "https://slack.com/api/";
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
    text?: string | null;
    user?: string | null;
    bot_id?: string | null;
    thread_ts?: string | null;
    reply_count?: number | null;
    reactions?: SlackReaction[] | null;
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
    name?: string | null;
    is_private?: boolean | null;
    is_member?: boolean | null;
    is_archived?: boolean | null;
    is_im?: boolean | null;
    is_mpim?: boolean | null;
    topic?: { value: string } | null;
    purpose?: { value: string } | null;
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
    name?: string | null;
    real_name?: string | null;
    deleted?: boolean | null;
    is_bot?: boolean | null;
    profile?: { display_name?: string | null; email?: string | null } | null;
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
  * @returns The bot's user and workspace identity as reported by `auth.test`.
 * @capability slack.com/bot/getIdentity {}
 */
export function getIdentity(): SlackIdentity {
    check("slack.com/bot/getIdentity", {});
    const data = slackPost("auth.test", {}).json() as IdentityResponse;
    requireOk(data, 200);
    return {
        userId: data.user_id, user: data.user, botId: data.bot_id ?? "",
        teamId: data.team_id, team: data.team, url: data.url,
    };
}

/**
 * Post a message as the bot and return Slack's stored message.
 * Set `threadTs` to reply in a thread. The bot must be allowed to post in the
 * target conversation, which normally means it must already be a member.
 * @param input Message text, destination, and optional thread/unfurl settings.
  * @returns The message as stored by Slack, including its `ts` for later updates or threading.
 * @capability slack.com/bot/sendMessage { channelId: string }
 */
export function sendMessage(input: SendMessageInput): SlackMessage {
    const { channelId, text, threadTs, unfurlLinks, unfurlMedia } = input;
    check("slack.com/bot/sendMessage", { channelId: channelId });
    const body: MessageWriteRequest = { channel: channelId, text: text };
    if (threadTs !== undefined) body.thread_ts = threadTs;
    if (unfurlLinks !== undefined) body.unfurl_links = unfurlLinks;
    if (unfurlMedia !== undefined) body.unfurl_media = unfurlMedia;
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
  * @returns The message as stored by Slack, posted in the DM conversation.
 * @capability slack.com/bot/sendDirectMessage { userId: string }
 */
export function sendDirectMessage(userId: string, text: string): SlackMessage {
    const recipientId = singleUserId(userId);
    check("slack.com/bot/sendDirectMessage", { userId: recipientId });
    const opened = slackPost("conversations.open", { users: recipientId }).json() as ConversationResponse;
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
  * @returns The message as stored by Slack, posted in the group DM conversation.
 * @capability slack.com/bot/sendGroupDirectMessage { userIds }
 */
export function sendGroupDirectMessage(userIds: string[], text: string): SlackMessage {
    const participantIds: string[] = [];
    for (const userId of userIds) participantIds.push(singleUserId(userId));
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
  * @returns The edited message, re-read from Slack after the update.
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
  * @returns The message, or `null` when no message with that timestamp is found.
 * @capability slack.com/bot/getMessage { channelId: string }
 */
export function getMessage(ref: MessageRef): SlackMessage | null {
    const { channelId, ts, threadTs } = ref;
    check("slack.com/bot/getMessage", { channelId: channelId });
    let rootTs = ts;
    if (threadTs !== undefined) rootTs = threadTs;
    const data = messages("conversations.replies", { channel: channelId, ts: rootTs, limit: 100 });
    for (const item of data.messages) if (item.ts === ts) return messageFrom(item);
    return null;
}

/**
 * Return one cursor page of messages from a conversation the bot can read.
 * Timestamp bounds are Slack timestamps represented as strings.
 * @param channelId Conversation whose history should be read.
 * @param options Optional page size, cursor, and timestamp bounds; omit it for Slack's defaults.
  * @returns One page of messages and a `nextCursor` that is empty on the last page.
 * @capability slack.com/bot/listMessages { channelId: string }
 */
export function listMessages(channelId: string, options: MessageListOptions = {}): MessagePage {
    const { limit, cursor, oldest, latest, inclusive } = options;
    check("slack.com/bot/listMessages", { channelId: channelId });
    const body: HistoryRequest = {
        channel: channelId,
        limit: limit,
        cursor: cursor,
        oldest: oldest,
        latest: latest,
        inclusive: inclusive,
    };
    return messagePage(messages("conversations.history", body));
}

/**
 * Return one cursor page containing a thread root and its replies.
 * @param channelId Conversation containing the thread.
 * @param threadTs Timestamp of the thread's root message.
 * @param page Optional page size and continuation cursor; omit it for Slack's defaults.
  * @returns The thread root and replies on this page, with a `nextCursor` that is empty on the last page.
 * @capability slack.com/bot/getThread { channelId: string }
 */
export function getThread(channelId: string, threadTs: string, page: PageOptions = {}): MessagePage {
    const { limit, cursor } = page;
    check("slack.com/bot/getThread", { channelId: channelId });
    const body: HistoryRequest = { channel: channelId, ts: threadTs, limit: limit, cursor: cursor };
    return messagePage(messages("conversations.replies", body));
}

/**
 * Fetch metadata for one channel, private group, or direct-message conversation.
 * Visibility follows the bot token's scopes and conversation membership.
 * @param channelId Slack conversation ID.
  * @returns The conversation's metadata.
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
 * @param page Optional page size and continuation cursor; omit it for Slack's defaults.
  * @returns One page of conversations and a `nextCursor` that is empty on the last page.
 * @capability slack.com/bot/listConversations {}
 */
export function listConversations(page: PageOptions = {}): ConversationPage {
    const { limit, cursor } = page;
    check("slack.com/bot/listConversations", {});
    const body: PageRequest = { types: "public_channel,private_channel,mpim,im", limit: limit, cursor: cursor };
    const data = slackGet("conversations.list", pageQuery(body)).json() as ConversationsResponse;
    requireOk(data, 200);
    const conversations: SlackConversation[] = [];
    for (const item of data.channels) conversations.push(conversationFrom(item));
    return { conversations: conversations, nextCursor: cursorOf(data.response_metadata) };
}

/**
 * List the Slack user IDs belonging to a conversation.
 * @param channelId Conversation whose membership should be read.
 * @param page Optional page size and continuation cursor; omit it for Slack's defaults.
  * @returns One page of member user IDs and a `nextCursor` that is empty on the last page.
 * @capability slack.com/bot/listMembers { channelId: string }
 */
export function listMembers(channelId: string, page: PageOptions = {}): MemberPage {
    const { limit, cursor } = page;
    check("slack.com/bot/listMembers", { channelId: channelId });
    const query = pageQuery({ limit: limit, cursor: cursor });
    query.set("channel", channelId);
    const data = slackGet("conversations.members", query).json() as MembersResponse;
    requireOk(data, 200);
    return { memberIds: data.members, nextCursor: cursorOf(data.response_metadata) };
}

/**
 * Open or resume a 1:1 direct-message conversation without sending a message.
 * Requires Slack's `im:write` bot-token scope.
 * @param userId Workspace user ID to include, excluding the bot itself.
  * @returns The 1:1 DM conversation, including its conversation ID for later sends.
 * @capability slack.com/bot/openDirectMessage { userId }
 */
export function openDirectMessage(userId: string): SlackConversation {
    const recipientId = singleUserId(userId);
    check("slack.com/bot/openDirectMessage", { userId: recipientId });
    const data = slackPost("conversations.open", { users: recipientId }).json() as ConversationResponse;
    requireOk(data, 200);
    return conversationFrom(data.channel);
}

/**
 * Open or resume a multi-person direct message without sending a message.
 * Pass 2–8 workspace user IDs and do not include the bot itself.
 * Requires Slack's `mpim:write` bot-token scope.
 * @param userIds Workspace user IDs to include, excluding the bot itself.
  * @returns The group DM conversation, including its conversation ID for later sends.
 * @capability slack.com/bot/openGroupDirectMessage { userIds }
 */
export function openGroupDirectMessage(userIds: string[]): SlackConversation {
    const participantIds: string[] = [];
    for (const userId of userIds) participantIds.push(singleUserId(userId));
    check("slack.com/bot/openGroupDirectMessage", { userIds: participantIds });
    const data = slackPost("conversations.open", { users: participantIds.join(",") }).json() as ConversationResponse;
    requireOk(data, 200);
    return conversationFrom(data.channel);
}

/**
 * Fetch one workspace user by Slack user ID.
 * @param userId Slack user ID, such as `U012ABCDEF`.
  * @returns The user's profile data.
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
 * @param page Optional page size and continuation cursor; omit it for Slack's defaults.
  * @returns One page of users and a `nextCursor` that is empty on the last page.
 * @capability slack.com/bot/listUsers {}
 */
export function listUsers(page: PageOptions = {}): UserPage {
    const { limit, cursor } = page;
    check("slack.com/bot/listUsers", {});
    const data = slackGet("users.list", pageQuery({ limit: limit, cursor: cursor })).json() as UsersResponse;
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

function messages(method: string, body: HistoryRequest): MessagesResponse {
    const query = pageQuery(body);
    query.set("channel", body.channel);
    const ts = body.ts;
    const oldest = body.oldest;
    const latest = body.latest;
    const inclusive = body.inclusive;
    if (ts !== undefined) query.set("ts", ts);
    if (oldest !== undefined) query.set("oldest", oldest);
    if (latest !== undefined) query.set("latest", latest);
    if (inclusive !== undefined) query.set("inclusive", inclusive.toString());
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
    if (limit !== undefined) query.set("limit", limit.toString());
    if (cursor !== undefined) query.set("cursor", cursor);
    if (types !== undefined) query.set("types", types);
    return query;
}

function authHeaders(): Map<string, string> {
    const token = secrets.get("SLACK_BOT_TOKEN");
    if (token === undefined) throw new SlackError("missing_token", "SLACK_BOT_TOKEN is not bound", 200);
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
        const code = envelope.error ?? "unknown_error";
        throw new SlackError(code, `Slack API error: ${code}`, status);
    }
}

function messageFrom(item: ApiMessage): SlackMessage {
    let reactions: SlackReaction[] = [];
    if (item.reactions) reactions = item.reactions;
    return {
        ts: item.ts, text: item.text ?? "", user: item.user ?? "", botId: item.bot_id ?? "",
        threadTs: item.thread_ts ?? "", replyCount: item.reply_count ?? 0,
        reactions: reactions,
    };
}

function conversationFrom(channel: ApiConversation): SlackConversation {
    return {
        id: channel.id, name: channel.name ?? "", isPrivate: channel.is_private === true,
        isMember: channel.is_member === true, isArchived: channel.is_archived === true,
        isIm: channel.is_im === true, isMpim: channel.is_mpim === true,
        topic: channel.topic?.value ?? "",
        purpose: channel.purpose?.value ?? "",
    };
}

function userFrom(user: ApiUser): SlackUser {
    const profile = user.profile;
    return {
        id: user.id, name: user.name ?? "", realName: user.real_name ?? "",
        displayName: profile?.display_name ?? "",
        email: profile?.email ?? "",
        deleted: user.deleted === true, isBot: user.is_bot === true,
    };
}

function cursorOf(metadata: Metadata | undefined): string { return metadata?.next_cursor ?? ""; }
