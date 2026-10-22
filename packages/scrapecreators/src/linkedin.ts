import { get, post } from "submilli:http";
import { ProviderIssue, Result, LinkedInGetProfileOptions, LinkedInGetProfileData, LinkedInGetCompanyOptions, LinkedInGetCompanyData, LinkedInListCompanyPostsOptions, LinkedInListCompanyPostsData, LinkedInSearchPostsOptions, LinkedInSearchPostsData, LinkedInGetPostOptions, LinkedInGetPostData, LinkedInGetTranscriptOptions, LinkedInGetTranscriptData } from "./types";
export { ProviderIssue, Result, LinkedInGetProfileOptions, LinkedInGetProfileData, LinkedInGetCompanyOptions, LinkedInGetCompanyData, LinkedInListCompanyPostsOptions, LinkedInListCompanyPostsData, LinkedInSearchPostsOptions, LinkedInSearchPostsData, LinkedInGetPostOptions, LinkedInGetPostData, LinkedInGetTranscriptOptions, LinkedInGetTranscriptData } from "./types";
import { responseIssues, stringParam, booleanParam, integerParam, queryString, requireIdentity, authHeaders, readResponse, invalidArgument, invalidResponse, invalidResponseFrom, validateDates, validateSearchCursor, redditCommentsBody } from "./transport";
/** LinkedIn public-data operations through ScrapeCreators. */
export class LinkedInTransport {
    /** Person's Profile.
     * @param options Provider controls; omit optional values to retain provider defaults.
     * @returns One typed response and its complete raw JSON.
     */
    getProfile(options: LinkedInGetProfileOptions): Result<LinkedInGetProfileData> {
        const params = new Map<string, string>();
        stringParam(params, "url", options.url, true);
        // The provider charges for, then refuses, company and school pages.
        if (!/^https?:\/\/([a-z0-9-]+\.)*linkedin\.com\/in\/[^/?#]+/i.test(options.url))
            throw invalidArgument("url must be a person's profile, linkedin.com/in/<handle>; use getCompany for company pages");
        const response = get("https://api.scrapecreators.com/v1/linkedin/profile" + queryString(params), authHeaders());
        const raw = readResponse(response);
        try {
            const data = raw as LinkedInGetProfileData;
            if (typeof data.name !== "string" || data.name.trim().length === 0)
                throw invalidResponse("name is missing or blank");
            return { data: data, raw: raw, issues: responseIssues(raw) };
        }
        catch (cause) {
            throw invalidResponseFrom(cause);
        }
    }
    /** Company Page.
     * @param options Provider controls; omit optional values to retain provider defaults.
     * @returns One typed response and its complete raw JSON.
     */
    getCompany(options: LinkedInGetCompanyOptions): Result<LinkedInGetCompanyData> {
        const params = new Map<string, string>();
        stringParam(params, "url", options.url, true);
        const response = get("https://api.scrapecreators.com/v1/linkedin/company" + queryString(params), authHeaders());
        const raw = readResponse(response);
        try {
            const data = raw as LinkedInGetCompanyData;
            if (typeof data.name !== "string" || data.name.trim().length === 0)
                throw invalidResponse("name is missing or blank");
            return { data: data, raw: raw, issues: responseIssues(raw) };
        }
        catch (cause) {
            throw invalidResponseFrom(cause);
        }
    }
    /** Company Posts.
     * @param options Provider controls; omit optional values to retain provider defaults.
     * @returns One typed response and its complete raw JSON.
     */
    listCompanyPosts(options: LinkedInListCompanyPostsOptions): Result<LinkedInListCompanyPostsData> {
        const params = new Map<string, string>();
        stringParam(params, "url", options.url, true);
        integerParam(params, "page", options.page, false);
        const response = get("https://api.scrapecreators.com/v1/linkedin/company/posts" + queryString(params), authHeaders());
        const raw = readResponse(response);
        try {
            const data = raw as LinkedInListCompanyPostsData;
            if (!Array.isArray(data.posts))
                throw invalidResponse("posts is not an array");
            return { data: data, raw: raw, issues: responseIssues(raw) };
        }
        catch (cause) {
            throw invalidResponseFrom(cause);
        }
    }
    /** Search Posts.
     * @param options Provider controls; omit optional values to retain provider defaults.
     * @returns One typed response and its complete raw JSON.
     */
    searchPosts(options: LinkedInSearchPostsOptions): Result<LinkedInSearchPostsData> {
        const params = new Map<string, string>();
        stringParam(params, "query", options.query, true);
        stringParam(params, "date_posted", options.datePosted, false, ["last-hour", "last-day", "last-week", "last-month", "last-year"]);
        stringParam(params, "cursor", options.cursor, false);
        validateSearchCursor(options.cursor);
        const response = get("https://api.scrapecreators.com/v1/linkedin/search/posts" + queryString(params), authHeaders());
        const raw = readResponse(response);
        try {
            const data = raw as LinkedInSearchPostsData;
            if (!Array.isArray(data.posts))
                throw invalidResponse("posts is not an array");
            return { data: data, raw: raw, issues: responseIssues(raw) };
        }
        catch (cause) {
            throw invalidResponseFrom(cause);
        }
    }
    /** Post.
     * @param options Provider controls; omit optional values to retain provider defaults.
     * @returns One typed response and its complete raw JSON.
     */
    getPost(options: LinkedInGetPostOptions): Result<LinkedInGetPostData> {
        const params = new Map<string, string>();
        stringParam(params, "url", options.url, true);
        const response = get("https://api.scrapecreators.com/v1/linkedin/post" + queryString(params), authHeaders());
        const raw = readResponse(response);
        try {
            const data = raw as LinkedInGetPostData;
            if (typeof data.url !== "string" || data.url.trim().length === 0)
                throw invalidResponse("url is missing or blank");
            return { data: data, raw: raw, issues: responseIssues(raw) };
        }
        catch (cause) {
            throw invalidResponseFrom(cause);
        }
    }
    /** Post Transcript.
     * @param options Provider controls; omit optional values to retain provider defaults.
     * @returns One typed response and its complete raw JSON.
     */
    getTranscript(options: LinkedInGetTranscriptOptions): Result<LinkedInGetTranscriptData> {
        const params = new Map<string, string>();
        stringParam(params, "url", options.url, true);
        const response = get("https://api.scrapecreators.com/v1/linkedin/post/transcript" + queryString(params), authHeaders());
        const raw = readResponse(response);
        try {
            const data = raw as LinkedInGetTranscriptData;
            if (data.transcript === undefined)
                throw invalidResponse("transcript is missing");
            return { data: data, raw: raw, issues: responseIssues(raw) };
        }
        catch (cause) {
            throw invalidResponseFrom(cause);
        }
    }
}
