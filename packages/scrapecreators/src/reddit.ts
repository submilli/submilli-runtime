import { get, post } from "submilli:http";
import { ProviderIssue, Result, RedditGetSubredditOptions, RedditGetSubredditData, RedditListSubredditPostsOptions, RedditListSubredditPostsData, RedditSearchSubredditOptions, RedditSearchSubredditData, RedditGetPostOptions, RedditGetPostData, RedditListCommentsOptions, RedditListCommentsData, RedditGetTranscriptOptions, RedditGetTranscriptData, RedditSearchOptions, RedditSearchData } from "./types";
export { ProviderIssue, Result, RedditGetSubredditOptions, RedditGetSubredditData, RedditListSubredditPostsOptions, RedditListSubredditPostsData, RedditSearchSubredditOptions, RedditSearchSubredditData, RedditGetPostOptions, RedditGetPostData, RedditListCommentsOptions, RedditListCommentsData, RedditGetTranscriptOptions, RedditGetTranscriptData, RedditSearchOptions, RedditSearchData } from "./types";
import { responseIssues, stringParam, booleanParam, integerParam, queryString, requireIdentity, authHeaders, readResponse, invalidArgument, invalidResponse, validateDates, validateSearchCursor, redditCommentsBody } from "./transport";
const redditOrigin = "https://www.reddit.com";
/** The fields of a returned Reddit post that give its canonical URL. */
interface PostUrlFields {
    id?: unknown;
    permalink?: unknown;
    canonical_url?: string | null;
}
/** The fields of a returned Reddit comment that give its canonical URL, and what it nests. */
interface CommentUrlFields {
    url?: unknown;
    permalink?: unknown;
    canonical_url?: string | null;
    post?: unknown;
    replies?: unknown;
}
interface ReplyListing {
    items?: unknown;
}
function permalinkUrl(permalink: unknown): string | null {
    return typeof permalink === "string" && permalink.startsWith("/") ? redditOrigin + permalink : null;
}
// A post's canonical URL is its permalink on reddit.com, else /comments/<id> without a t3_ prefix.
function addPostCanonicalUrl(value: unknown): void {
    if (value === null || typeof value !== "object" || Array.isArray(value))
        return;
    const post = value as PostUrlFields;
    let url = permalinkUrl(post.permalink);
    if (url === null && typeof post.id === "string") {
        const id = post.id.startsWith("t3_") ? post.id.slice(3) : post.id;
        if (/^[A-Za-z0-9]+$/.test(id))
            url = redditOrigin + "/comments/" + id;
    }
    post.canonical_url = url;
}
function addPostCanonicalUrls(values: unknown): void {
    if (Array.isArray(values))
        for (const value of values as unknown[])
            addPostCanonicalUrl(value);
}
// Covers the comment's embedded post and every nested reply too.
function addCommentCanonicalUrl(value: unknown): void {
    if (value === null || typeof value !== "object" || Array.isArray(value))
        return;
    const comment = value as CommentUrlFields;
    let url = permalinkUrl(comment.permalink);
    if (url === null && typeof comment.url === "string" && comment.url.startsWith(redditOrigin + "/"))
        url = comment.url;
    comment.canonical_url = url;
    addPostCanonicalUrl(comment.post);
    if (comment.replies !== null && typeof comment.replies === "object" && !Array.isArray(comment.replies))
        addCommentCanonicalUrls((comment.replies as ReplyListing).items);
}
function addCommentCanonicalUrls(values: unknown): void {
    if (Array.isArray(values))
        for (const value of values as unknown[])
            addCommentCanonicalUrl(value);
}
/** Reddit public-data operations through ScrapeCreators. */
export class RedditTransport {
    /** Subreddit Details.
     * @param options Provider controls; omit optional values to retain provider defaults.
     * @returns One typed response and its complete raw JSON.
     */
    getSubreddit(options: RedditGetSubredditOptions = {}): Result<RedditGetSubredditData> {
        const params = new Map<string, string>();
        stringParam(params, "subreddit", options.subreddit, false);
        stringParam(params, "url", options.url, false);
        stringParam(params, "cache_max_age", options.cacheMaxAge, false, ["1d", "3d", "7d", "14d", "30d"]);
        requireIdentity([options.subreddit, options.url]);
        const response = get("https://api.scrapecreators.com/v1/reddit/subreddit/details" + queryString(params), authHeaders());
        const raw = readResponse(response);
        try {
            const data = raw as RedditGetSubredditData;
            if (typeof data.display_name !== "string" || data.display_name.trim().length === 0)
                throw invalidResponse();
            return { data: data, raw: raw, issues: responseIssues(raw) };
        }
        catch (cause) {
            throw invalidResponse();
        }
    }
    /** Subreddit Posts.
     * @param options Provider controls; omit optional values to retain provider defaults.
     * @returns One typed response and its complete raw JSON.
     */
    listSubredditPosts(options: RedditListSubredditPostsOptions): Result<RedditListSubredditPostsData> {
        const params = new Map<string, string>();
        stringParam(params, "subreddit", options.subreddit, true);
        stringParam(params, "timeframe", options.timeframe, false, ["all", "day", "week", "month", "year"]);
        stringParam(params, "sort", options.sort, false, ["best", "hot", "new", "top", "rising"]);
        stringParam(params, "after", options.after, false);
        booleanParam(params, "trim", options.trim, false);
        stringParam(params, "cache_max_age", options.cacheMaxAge, false, ["1d", "3d", "7d", "14d", "30d"]);
        const response = get("https://api.scrapecreators.com/v1/reddit/subreddit" + queryString(params), authHeaders());
        const raw = readResponse(response);
        try {
            // A separate decode keeps raw native while data gains canonical_url.
            const data = readResponse(response) as RedditListSubredditPostsData;
            if (!Array.isArray(data.posts))
                throw invalidResponse();
            addPostCanonicalUrls(data.posts);
            return { data: data, raw: raw, issues: responseIssues(raw) };
        }
        catch (cause) {
            throw invalidResponse();
        }
    }
    /** Subreddit Search.
     * @param options Provider controls; omit optional values to retain provider defaults.
     * @returns One typed response and its complete raw JSON.
     */
    searchSubreddit(options: RedditSearchSubredditOptions): Result<RedditSearchSubredditData> {
        const params = new Map<string, string>();
        stringParam(params, "subreddit", options.subreddit, true);
        stringParam(params, "query", options.query, false);
        stringParam(params, "sort", options.sort, false, ["relevance", "hot", "top", "new", "comments"]);
        stringParam(params, "timeframe", options.timeframe, false, ["all", "year", "month", "week", "day", "hour"]);
        stringParam(params, "cursor", options.cursor, false);
        const response = get("https://api.scrapecreators.com/v1/reddit/subreddit/search" + queryString(params), authHeaders());
        const raw = readResponse(response);
        try {
            // A separate decode keeps raw native while data gains canonical_url.
            const data = readResponse(response) as RedditSearchSubredditData;
            if (!Array.isArray(data.posts))
                throw invalidResponse();
            addPostCanonicalUrls(data.posts);
            addCommentCanonicalUrls(data.comments);
            addPostCanonicalUrls(data.media);
            return { data: data, raw: raw, issues: responseIssues(raw) };
        }
        catch (cause) {
            throw invalidResponse();
        }
    }
    /** Post.
     * @param options Provider controls; omit optional values to retain provider defaults.
     * @returns One typed response and its complete raw JSON.
     */
    getPost(options: RedditGetPostOptions): Result<RedditGetPostData> {
        const params = new Map<string, string>();
        stringParam(params, "url", options.url, true);
        const response = get("https://api.scrapecreators.com/v1/reddit/post" + queryString(params), authHeaders());
        const raw = readResponse(response);
        try {
            // A separate decode keeps raw native while data gains canonical_url.
            const data = readResponse(response) as RedditGetPostData;
            if (typeof data.id !== "string" || data.id.trim().length === 0)
                throw invalidResponse();
            addPostCanonicalUrl(data);
            return { data: data, raw: raw, issues: responseIssues(raw) };
        }
        catch (cause) {
            throw invalidResponse();
        }
    }
    /** Post Comments.
     * @param options Provider controls; omit optional values to retain provider defaults.
     * @returns One typed response and its complete raw JSON.
     */
    listComments(options: RedditListCommentsOptions): Result<RedditListCommentsData> {
        const params = new Map<string, string>();
        stringParam(params, "url", options.url, true);
        stringParam(params, "cursor", options.cursor, false);
        booleanParam(params, "trim", options.trim, false);
        const response = options.usePost === true
            ? post("https://api.scrapecreators.com/v1/reddit/post/comments", redditCommentsBody(params), authHeaders())
            : get("https://api.scrapecreators.com/v1/reddit/post/comments" + queryString(params), authHeaders());
        const raw = readResponse(response);
        try {
            // A separate decode keeps raw native while data gains canonical_url.
            const data = readResponse(response) as RedditListCommentsData;
            if (!Array.isArray(data.comments))
                throw invalidResponse();
            if (data.post === null || typeof data.post !== "object" || Array.isArray(data.post))
                throw invalidResponse();
            addPostCanonicalUrl(data.post);
            addCommentCanonicalUrls(data.comments);
            return { data: data, raw: raw, issues: responseIssues(raw) };
        }
        catch (cause) {
            throw invalidResponse();
        }
    }
    /** Post Transcript.
     * @param options Provider controls; omit optional values to retain provider defaults.
     * @returns One typed response and its complete raw JSON.
     */
    getTranscript(options: RedditGetTranscriptOptions): Result<RedditGetTranscriptData> {
        const params = new Map<string, string>();
        stringParam(params, "url", options.url, true);
        stringParam(params, "language", options.language, false);
        stringParam(params, "cache_max_age", options.cacheMaxAge, false, ["1d", "3d", "7d", "14d", "30d"]);
        const response = get("https://api.scrapecreators.com/v1/reddit/post/transcript" + queryString(params), authHeaders());
        const raw = readResponse(response);
        try {
            const data = raw as RedditGetTranscriptData;
            if (data.transcript === undefined)
                throw invalidResponse();
            return { data: data, raw: raw, issues: responseIssues(raw) };
        }
        catch (cause) {
            throw invalidResponse();
        }
    }
    /** Search.
     * @param options Provider controls; omit optional values to retain provider defaults.
     * @returns One typed response and its complete raw JSON.
     */
    search(options: RedditSearchOptions): Result<RedditSearchData> {
        const params = new Map<string, string>();
        stringParam(params, "query", options.query, true);
        stringParam(params, "filter", options.filter, false, ["posts", "comments"]);
        stringParam(params, "sort", options.sort, false, ["relevance", "new", "top", "comment_count"]);
        stringParam(params, "timeframe", options.timeframe, false, ["all", "day", "week", "month", "year"]);
        stringParam(params, "after", options.after, false);
        booleanParam(params, "trim", options.trim, false);
        if (options.filter === "comments" && (options.timeframe !== undefined || options.sort === "comment_count"))
            throw invalidArgument("Comment search does not support timeframe or comment_count sorting");
        const response = get("https://api.scrapecreators.com/v1/reddit/search" + queryString(params), authHeaders());
        const raw = readResponse(response);
        try {
            // A separate decode keeps raw native while data gains canonical_url.
            const data = readResponse(response) as RedditSearchData;
            if (options.filter === "comments") {
                if (!Array.isArray(data.comments))
                    throw invalidResponse();
            }
            else if (!Array.isArray(data.posts))
                throw invalidResponse();
            addPostCanonicalUrls(data.posts);
            addCommentCanonicalUrls(data.comments);
            return { data: data, raw: raw, issues: responseIssues(raw) };
        }
        catch (cause) {
            throw invalidResponse();
        }
    }
}
