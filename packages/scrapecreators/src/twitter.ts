import { get, post } from "submilli:http";
import { ProviderIssue, Result, TwitterSearchPostsOptions, TwitterSearchPostsData, TwitterGetProfileOptions, TwitterGetProfileData, TwitterListUserTweetsOptions, TwitterListUserTweetsData, TwitterGetTweetOptions, TwitterGetTweetData, TwitterGetTranscriptOptions, TwitterGetTranscriptData, TwitterGetCommunityOptions, TwitterGetCommunityData, TwitterListCommunityTweetsOptions, TwitterListCommunityTweetsData } from "./types";
export { ProviderIssue, Result, TwitterSearchPostsOptions, TwitterSearchPostsData, TwitterGetProfileOptions, TwitterGetProfileData, TwitterListUserTweetsOptions, TwitterListUserTweetsData, TwitterGetTweetOptions, TwitterGetTweetData, TwitterGetTranscriptOptions, TwitterGetTranscriptData, TwitterGetCommunityOptions, TwitterGetCommunityData, TwitterListCommunityTweetsOptions, TwitterListCommunityTweetsData } from "./types";
import { responseIssues, stringParam, booleanParam, integerParam, queryString, requireIdentity, authHeaders, readResponse, invalidArgument, invalidResponse, validateDates, validateSearchCursor, redditCommentsBody } from "./transport";
/** Twitter public-data operations through ScrapeCreators. */
export class TwitterTransport {
    /** Search Posts.
     * @param options Provider controls; omit optional values to retain provider defaults.
     * @returns One typed response and its complete raw JSON.
     */
    searchPosts(options: TwitterSearchPostsOptions): Result<TwitterSearchPostsData> {
        const params = new Map<string, string>();
        stringParam(params, "query", options.query, true);
        stringParam(params, "cursor", options.cursor, false);
        validateSearchCursor(options.cursor);
        const response = get("https://api.scrapecreators.com/v1/twitter/search/posts" + queryString(params), authHeaders());
        const raw = readResponse(response);
        try {
            const data = raw as TwitterSearchPostsData;
            if (!Array.isArray(data.posts))
                throw invalidResponse();
            return { data: data, raw: raw, issues: responseIssues(raw) };
        }
        catch (cause) {
            throw invalidResponse();
        }
    }
    /** Profile.
     * @param options Provider controls; omit optional values to retain provider defaults.
     * @returns One typed response and its complete raw JSON.
     */
    getProfile(options: TwitterGetProfileOptions): Result<TwitterGetProfileData> {
        const params = new Map<string, string>();
        stringParam(params, "handle", options.handle, true);
        stringParam(params, "cache_max_age", options.cacheMaxAge, false, ["1d", "3d", "7d", "14d", "30d"]);
        const response = get("https://api.scrapecreators.com/v1/twitter/profile" + queryString(params), authHeaders());
        const raw = readResponse(response);
        try {
            const data = raw as TwitterGetProfileData;
            if (typeof data.rest_id !== "string" || data.rest_id.trim().length === 0)
                throw invalidResponse();
            return { data: data, raw: raw, issues: responseIssues(raw) };
        }
        catch (cause) {
            throw invalidResponse();
        }
    }
    /** User Tweets.
     * @param options Provider controls; omit optional values to retain provider defaults.
     * @returns One typed response and its complete raw JSON.
     */
    listUserTweets(options: TwitterListUserTweetsOptions): Result<TwitterListUserTweetsData> {
        const params = new Map<string, string>();
        stringParam(params, "handle", options.handle, true);
        booleanParam(params, "trim", options.trim, false);
        const response = get("https://api.scrapecreators.com/v1/twitter/user-tweets" + queryString(params), authHeaders());
        const raw = readResponse(response);
        try {
            const data = raw as TwitterListUserTweetsData;
            if (!Array.isArray(data.tweets))
                throw invalidResponse();
            return { data: data, raw: raw, issues: responseIssues(raw) };
        }
        catch (cause) {
            throw invalidResponse();
        }
    }
    /** Tweet Details.
     * @param options Provider controls; omit optional values to retain provider defaults.
     * @returns One typed response and its complete raw JSON.
     */
    getTweet(options: TwitterGetTweetOptions): Result<TwitterGetTweetData> {
        const params = new Map<string, string>();
        stringParam(params, "url", options.url, true);
        booleanParam(params, "trim", options.trim, false);
        stringParam(params, "cache_max_age", options.cacheMaxAge, false, ["1d", "3d", "7d", "14d", "30d"]);
        const response = get("https://api.scrapecreators.com/v1/twitter/tweet" + queryString(params), authHeaders());
        const raw = readResponse(response);
        try {
            const data = raw as TwitterGetTweetData;
            if (typeof data.rest_id !== "string" || data.rest_id.trim().length === 0)
                throw invalidResponse();
            return { data: data, raw: raw, issues: responseIssues(raw) };
        }
        catch (cause) {
            throw invalidResponse();
        }
    }
    /** Transcript.
     * @param options Provider controls; omit optional values to retain provider defaults.
     * @returns One typed response and its complete raw JSON.
     */
    getTranscript(options: TwitterGetTranscriptOptions): Result<TwitterGetTranscriptData> {
        const params = new Map<string, string>();
        booleanParam(params, "allow_long_transcription", options.allowLongTranscription, false);
        stringParam(params, "url", options.url, true);
        stringParam(params, "cache_max_age", options.cacheMaxAge, false, ["1d", "3d", "7d", "14d", "30d"]);
        const response = get("https://api.scrapecreators.com/v1/twitter/tweet/transcript" + queryString(params), authHeaders());
        const raw = readResponse(response);
        try {
            const data = raw as TwitterGetTranscriptData;
            if (data.transcript === undefined)
                throw invalidResponse();
            return { data: data, raw: raw, issues: responseIssues(raw) };
        }
        catch (cause) {
            throw invalidResponse();
        }
    }
    /** Community.
     * @param options Provider controls; omit optional values to retain provider defaults.
     * @returns One typed response and its complete raw JSON.
     */
    getCommunity(options: TwitterGetCommunityOptions): Result<TwitterGetCommunityData> {
        const params = new Map<string, string>();
        stringParam(params, "url", options.url, true);
        const response = get("https://api.scrapecreators.com/v1/twitter/community" + queryString(params), authHeaders());
        const raw = readResponse(response);
        try {
            const data = raw as TwitterGetCommunityData;
            if (typeof data.rest_id !== "string" || data.rest_id.trim().length === 0)
                throw invalidResponse();
            return { data: data, raw: raw, issues: responseIssues(raw) };
        }
        catch (cause) {
            throw invalidResponse();
        }
    }
    /** Community Tweets.
     * @param options Provider controls; omit optional values to retain provider defaults.
     * @returns One typed response and its complete raw JSON.
     */
    listCommunityTweets(options: TwitterListCommunityTweetsOptions): Result<TwitterListCommunityTweetsData> {
        const params = new Map<string, string>();
        stringParam(params, "url", options.url, true);
        const response = get("https://api.scrapecreators.com/v1/twitter/community/tweets" + queryString(params), authHeaders());
        const raw = readResponse(response);
        try {
            const data = raw as TwitterListCommunityTweetsData;
            if (!Array.isArray(data.tweets))
                throw invalidResponse();
            return { data: data, raw: raw, issues: responseIssues(raw) };
        }
        catch (cause) {
            throw invalidResponse();
        }
    }
}
