/** Search controls; the package fetches one page and never retries automatically. */
export interface SearchOptions {
    /** Default 10. Range 10–100 for recent search, 10–500 for archive search. */
    maxResults?: number;
    /** Opaque cursor from the preceding page; keep query and other options unchanged. */
    nextToken?: string;
    /** Inclusive RFC 3339 timestamp, with a timezone. Provider validates the search window. */
    startTime?: string;
    /** Exclusive RFC 3339 timestamp, with a timezone. */
    endTime?: string;
    /** Defaults to recency. */
    sortOrder?: "recency" | "relevancy";
}

/** Conversation search options; a conversation includes nested replies. */
export interface ReplyOptions extends SearchOptions {
    /** Search the full archive instead of the last seven days; default false. */
    archive?: boolean;
}

/** Expanded author data supplied by X; may be unavailable even when the tweet is readable. */
export interface User {
    /** Numeric ID as a string to avoid precision loss. */
    id: string;
    /** Display name. */
    name: string;
    /** Username without @. */
    username: string;
}

/** Public engagement counts; missing values remain null, not zero. */
export interface TweetMetrics {
    /** Number of likes. */
    likes: number | null;
    /** Number of replies. */
    replies: number | null;
    /** Number of reposts/retweets. */
    reposts: number | null;
    /** Number of quotes. */
    quotes: number | null;
    /** Number of bookmarks. */
    bookmarks: number | null;
    /** Number of impressions when available. */
    impressions: number | null;
}

/** A parent, quoted tweet, or repost target. */
export interface TweetReference {
    /** Referenced tweet ID. */
    id: string;
    /** Provider relationship, normally replied_to, quoted, or retweeted. */
    type: string;
}

/** Structured source record from the X API, with no generated summary or raw envelope. */
export interface Tweet {
    /** Numeric tweet ID, preserved as a string. */
    id: string;
    /** Canonical source URL. */
    url: string;
    /** Full note text when available; otherwise the provider's tweet text. */
    text: string;
    /** Author ID when supplied. */
    authorId: string | null;
    /** Expanded author matched by ID, not response array position. */
    author: User | null;
    /** Provider publication timestamp. */
    createdAt: string | null;
    /** Root tweet ID for the entire conversation. */
    conversationId: string | null;
    /** Provider language code. */
    language: string | null;
    /** Engagement counters as reported by X. */
    metrics: TweetMetrics;
    /** Relationships including replied_to for reconstructing reply trees. */
    references: TweetReference[];
}

/** Provider error or warning, including partial author expansion failures. */
export interface ApiIssue {
    /** Provider title. */
    title: string | null;
    /** Provider problem type URL. */
    type: string | null;
    /** Provider explanation. */
    detail: string | null;
    /** Affected resource ID, when supplied. */
    resourceId: string | null;
}

/** A successful lookup may still contain partial expansion issues. */
export interface TweetResult {
    /** The requested source record. */
    tweet: Tweet;
    /** Partial provider errors; inspect even when the tweet is present. */
    issues: ApiIssue[];
}

/** One search page; continue explicitly while nextToken is non-null. */
export interface TweetPage {
    /** Tweets in provider order. */
    tweets: Tweet[];
    /** Opaque continuation cursor, null when absent. */
    nextToken: string | null;
    /** Provider count for this page, not total matches. */
    resultCount: number;
    /** Partial provider errors, such as unavailable author expansions. */
    issues: ApiIssue[];
}

/** Local failures use status 0. HTTP failures preserve retry/reset metadata. */
export class XError extends Error {
    /** invalid_argument, missing_credentials, invalid_response, provider_error, or mapped HTTP error. */
    code: string;
    /** HTTP status, or 0 for local decoding and validation errors. */
    status: number;
    /** Raw Retry-After header; null when absent. */
    retryAfter: string | null;
    /** Raw x-rate-limit-reset header (Unix seconds); null when absent. */
    rateLimitReset: string | null;
    /** Structured provider errors when they could be decoded. */
    issues: ApiIssue[];
    constructor(code: string, message: string, status: number = 0, retryAfter: string | null = null, rateLimitReset: string | null = null, issues: ApiIssue[] = []) {
        super(message);
        this.name = "XError";
        this.code = code;
        this.status = status;
        this.retryAfter = retryAfter;
        this.rateLimitReset = rateLimitReset;
        this.issues = issues;
    }
}
