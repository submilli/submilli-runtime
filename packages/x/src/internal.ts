import { encodeComponent } from "submilli:url";
import { SearchOptions, Tweet, TweetPage, TweetResult, TweetMetrics, TweetReference, User, ApiIssue, XError } from "./types";

/** Validate a tweet identity without numeric conversion or arbitrary host requests.
 * @param value Numeric ID or supported HTTPS X/Twitter post URL.
 * @returns Numeric tweet ID as a string.
 */
export function tweetId(value: string): string {
    if (/\s/.test(value)) throw invalidArgument("Expected a tweet ID or HTTPS X/Twitter post URL");
    if (/^[1-9][0-9]{0,18}$/.test(value)) return value;
    const match = /^https:\/\/(?:(?:www|mobile)\.)?(?:x\.com|twitter\.com)\/(?:[A-Za-z0-9_]{1,15}\/status|i\/status|i\/web\/status)\/([1-9][0-9]{0,18})(?:\/(?:photo|video)\/[1-9][0-9]*)?\/?(?:[?#][^\s]*)?$/i.exec(value);
    if (match === null) throw invalidArgument("Expected a tweet ID or HTTPS X/Twitter post URL");
    return match[1];
}

/** The fields and author expansion requested by every read operation.
 * @returns Encoded field selection; no caller-supplied field or endpoint overrides.
 */
export function tweetFieldsQuery(): string {
    return "tweet.fields=" + encodeComponent("author_id,created_at,conversation_id,referenced_tweets,public_metrics,lang,note_tweet") + "&expansions=author_id&user.fields=" + encodeComponent("id,name,username");
}

/** Encode and validate one search page before permissions or credentials.
 * @param query Advanced search expression, 1–4096 characters.
 * @param options Pagination and search controls.
 * @param maximumResults Endpoint result limit, 100 for recent or 500 for archive.
 * @returns Encoded query string, including fixed field selection.
 */
export function buildSearchQuery(query: string, options: SearchOptions, maximumResults: number): string {
    if (query.trim().length === 0 || query.length > 4096) throw invalidArgument("query must contain 1–4096 characters");
    const { maxResults, nextToken, startTime, endTime, sortOrder } = options;
    const count = maxResults ?? 10;
    if (!Number.isInteger(count) || count < 10 || count > maximumResults) throw invalidArgument("maxResults is outside the endpoint's supported integer range");
    const sort = sortOrder ?? "recency";
    if (sort !== "recency" && sort !== "relevancy") throw invalidArgument("sortOrder must be recency or relevancy");
    const params: string[] = ["query=" + encodeComponent(query), "max_results=" + count.toString(), "sort_order=" + sort, tweetFieldsQuery()];
    if (nextToken !== undefined) {
        if (nextToken.trim().length === 0) throw invalidArgument("nextToken must not be blank");
        params.push("next_token=" + encodeComponent(nextToken));
    }
    const start = timestamp(startTime, "startTime");
    const end = timestamp(endTime, "endTime");
    if (start !== null) params.push("start_time=" + encodeComponent(start));
    if (end !== null) params.push("end_time=" + encodeComponent(end));
    if (start !== null && end !== null && Temporal.Instant.compare(Temporal.Instant.from(start), Temporal.Instant.from(end)) >= 0) throw invalidArgument("startTime must be before endTime");
    return params.join("&");
}

interface WireIssue {
    title?: string | null;
    type?: string | null;
    detail?: string | null;
    resource_id?: string | null;
}

interface WireEnvelope {
    data?: unknown;
    includes?: { users?: User[] | null } | null;
    errors?: WireIssue[] | null;
    meta?: { result_count: number; next_token?: string | null } | null;
}

interface WireMetrics {
    like_count?: number | null;
    reply_count?: number | null;
    retweet_count?: number | null;
    repost_count?: number | null;
    quote_count?: number | null;
    bookmark_count?: number | null;
    impression_count?: number | null;
}

interface WireTweet {
    id: string;
    text: string;
    author_id?: string | null;
    created_at?: string | null;
    conversation_id?: string | null;
    lang?: string | null;
    public_metrics?: WireMetrics | null;
    referenced_tweets?: TweetReference[] | null;
    referenced_posts?: TweetReference[] | null;
    note_tweet?: { text: string } | null;
    note_post?: { text: string } | null;
}

/** Decode a successful lookup, preserving partial provider issues.
 * @param body Response JSON.
 * @param expectedId ID requested on the wire.
 * @returns The source tweet with partial issues, or a typed error when no tweet was returned.
 */
export function decodeTweet(body: string, expectedId: string): TweetResult {
    try {
        const envelope = JSON.parse(body) as WireEnvelope;
        const issues = readIssues(envelope.errors);
        requireData(envelope.data, issues);
        const tweet = readTweet(envelope.data, readUsers(envelope.includes?.users));
        if (tweet.id !== expectedId) throw invalidResponse();
        return { tweet: tweet, issues: issues };
    } catch (cause) {
        if (cause instanceof XError) throw cause;
        throw invalidResponse();
    }
}

/** Decode one page; absent data is valid only for a zero-result page.
 * @param body Response JSON.
 * @returns Typed tweets, next cursor, page count, and partial issues.
 */
export function decodePage(body: string): TweetPage {
    try {
        const envelope = JSON.parse(body) as WireEnvelope;
        const issues = readIssues(envelope.errors);
        if (envelope.data === undefined || envelope.data === null) {
            if (issues.length > 0) throw new XError("provider_error", "X returned errors without tweet data", 0, null, null, issues);
        }
        const meta = envelope.meta;
        if (meta === undefined || meta === null || !Number.isSafeInteger(meta.result_count) || meta.result_count < 0) throw invalidResponse();
        const values: unknown[] = envelope.data === undefined || envelope.data === null ? [] : envelope.data as unknown[];
        const users = readUsers(envelope.includes?.users);
        const tweets: Tweet[] = [];
        for (const value of values) tweets.push(readTweet(value, users));
        if (tweets.length === 0 && issues.length > 0) throw new XError("provider_error", "X returned errors without tweet data", 0, null, null, issues);
        if (meta.result_count !== tweets.length) throw invalidResponse();
        const next = meta.next_token ?? null;
        if (next !== null && next.trim().length === 0) throw invalidResponse();
        return { tweets: tweets, nextToken: next, resultCount: meta.result_count, issues: issues };
    } catch (cause) {
        if (cause instanceof XError) throw cause;
        throw invalidResponse();
    }
}

/** Map HTTP errors while retaining structured provider explanations and rate headers.
 * @param status Failed HTTP status.
 * @param body Provider JSON, or an unparseable response body.
 * @param retryAfter Raw Retry-After header.
 * @param rateLimitReset Raw x-rate-limit-reset header.
 * @returns Typed error; malformed error bodies do not hide the HTTP failure.
 */
export function xHttpError(status: number, body: string, retryAfter: string | null = null, rateLimitReset: string | null = null): XError {
    let code = "http_error";
    if (status === 400 || status === 422) code = "invalid_request";
    else if (status === 401) code = "unauthorized";
    else if (status === 402) code = "quota_exceeded";
    else if (status === 403) code = "forbidden";
    else if (status === 404) code = "not_found";
    else if (status === 429) code = "rate_limited";
    let issues: ApiIssue[] = [];
    try {
        const parsed: unknown = JSON.parse(body);
        issues = readIssues((parsed as WireEnvelope).errors);
        const root = parsed as WireIssue;
        if (root.title !== undefined || root.type !== undefined || root.detail !== undefined) issues.push(readIssue(root));
    } catch (cause) {
        // The HTTP status remains authoritative when error JSON is unavailable.
    }
    return new XError(code, "X API request failed: HTTP " + status.toString(), status, retryAfter, rateLimitReset, issues);
}

function readTweet(value: unknown, users: Map<string, User>): Tweet {
    const data = value as WireTweet;
    requireId(data.id);
    if (data.author_id !== undefined && data.author_id !== null) requireId(data.author_id);
    if (data.conversation_id !== undefined && data.conversation_id !== null) requireId(data.conversation_id);
    const references: TweetReference[] = [];
    for (const reference of data.referenced_tweets ?? data.referenced_posts ?? []) {
        requireId(reference.id);
        if (reference.type.trim().length === 0) throw invalidResponse();
        references.push({ id: reference.id, type: reference.type });
    }
    return {
        id: data.id, url: "https://x.com/i/status/" + data.id,
        text: data.note_tweet?.text ?? data.note_post?.text ?? data.text,
        authorId: data.author_id ?? null,
        author: data.author_id === undefined || data.author_id === null ? null : users.get(data.author_id) ?? null,
        createdAt: data.created_at ?? null, conversationId: data.conversation_id ?? null,
        language: data.lang ?? null, metrics: readMetrics(data.public_metrics), references: references,
    };
}

function readUsers(values: User[] | null | undefined): Map<string, User> {
    const users = new Map<string, User>();
    for (const user of values ?? []) {
        requireId(user.id);
        if (user.username.trim().length === 0) throw invalidResponse();
        users.set(user.id, { id: user.id, name: user.name, username: user.username });
    }
    return users;
}

function readMetrics(value: WireMetrics | null | undefined): TweetMetrics {
    return {
        likes: counter(value?.like_count), replies: counter(value?.reply_count),
        reposts: counter(value?.retweet_count ?? value?.repost_count), quotes: counter(value?.quote_count),
        bookmarks: counter(value?.bookmark_count), impressions: counter(value?.impression_count),
    };
}

function readIssues(values: WireIssue[] | null | undefined): ApiIssue[] {
    const issues: ApiIssue[] = [];
    for (const value of values ?? []) issues.push(readIssue(value));
    return issues;
}

function readIssue(value: WireIssue): ApiIssue {
    return { title: value.title ?? null, type: value.type ?? null, detail: value.detail ?? null, resourceId: value.resource_id ?? null };
}

function requireData(value: unknown, issues: ApiIssue[]): void {
    if (value !== undefined && value !== null) return;
    if (issues.length > 0) throw new XError("provider_error", "X returned errors without tweet data", 0, null, null, issues);
    throw invalidResponse();
}

function timestamp(value: string | undefined, name: string): string | null {
    if (value === undefined) return null;
    try {
        if (!/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:\d{2})$/.test(value)) throw invalidArgument(name + " must be an RFC 3339 timestamp");
        return Temporal.Instant.from(value).toString();
    } catch (cause) {
        throw invalidArgument(name + " must be a valid RFC 3339 timestamp with a timezone");
    }
}

function counter(value: number | null | undefined): number | null {
    if (value === undefined || value === null) return null;
    if (!Number.isSafeInteger(value) || value < 0) throw invalidResponse();
    return value;
}

function requireId(value: string): void {
    if (!/^[1-9][0-9]{0,18}$/.test(value)) throw invalidResponse();
}

function invalidArgument(message: string): XError { return new XError("invalid_argument", message); }
function invalidResponse(): XError { return new XError("invalid_response", "X returned an invalid tweet response"); }
