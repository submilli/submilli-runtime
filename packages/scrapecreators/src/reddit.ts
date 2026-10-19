import { get, post } from "submilli:http";
import { ProviderIssue, Result, RedditGetSubredditOptions, RedditGetSubredditData, RedditListSubredditPostsOptions, RedditListSubredditPostsData, RedditSearchSubredditOptions, RedditSearchSubredditData, RedditGetPostOptions, RedditGetPostData, RedditListCommentsOptions, RedditListCommentsData, RedditGetTranscriptOptions, RedditGetTranscriptData, RedditSearchOptions, RedditSearchData } from "./types";
export { ProviderIssue, Result, RedditGetSubredditOptions, RedditGetSubredditData, RedditListSubredditPostsOptions, RedditListSubredditPostsData, RedditSearchSubredditOptions, RedditSearchSubredditData, RedditGetPostOptions, RedditGetPostData, RedditListCommentsOptions, RedditListCommentsData, RedditGetTranscriptOptions, RedditGetTranscriptData, RedditSearchOptions, RedditSearchData } from "./types";
import { responseIssues, stringParam, booleanParam, integerParam, queryString, requireIdentity, authHeaders, readResponse, invalidArgument, invalidResponse, validateDates, validateSearchCursor, redditCommentsBody } from "./transport";
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
            const data = raw as RedditListSubredditPostsData;
            if (!Array.isArray(data.posts))
                throw invalidResponse();
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
            const data = raw as RedditSearchSubredditData;
            if (!Array.isArray(data.posts))
                throw invalidResponse();
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
            const data = raw as RedditGetPostData;
            if (typeof data.id !== "string" || data.id.trim().length === 0)
                throw invalidResponse();
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
            const data = raw as RedditListCommentsData;
            if (!Array.isArray(data.comments))
                throw invalidResponse();
            if (data.post === null || typeof data.post !== "object" || Array.isArray(data.post))
                throw invalidResponse();
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
            const data = raw as RedditSearchData;
            if (options.filter === "comments") {
                if (!Array.isArray(data.comments))
                    throw invalidResponse();
            }
            else if (!Array.isArray(data.posts))
                throw invalidResponse();
            return { data: data, raw: raw, issues: responseIssues(raw) };
        }
        catch (cause) {
            throw invalidResponse();
        }
    }
}
