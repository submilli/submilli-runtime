import { get, post } from "submilli:http";
import { ProviderIssue, Result, GitHubGetUserOptions, GitHubGetUserData, GitHubListUserRepositoriesOptions, GitHubListUserRepositoriesData, GitHubListUserPullRequestsOptions, GitHubListUserPullRequestsData, GitHubListUserActivityOptions, GitHubListUserActivityData, GitHubListFollowersOptions, GitHubListFollowersData, GitHubListFollowingOptions, GitHubListFollowingData, GitHubGetContributionsOptions, GitHubGetContributionsData, GitHubGetRepositoryOptions, GitHubGetRepositoryData, GitHubListTrendingRepositoriesOptions, GitHubListTrendingRepositoriesData, GitHubListTrendingDevelopersOptions, GitHubListTrendingDevelopersData } from "./types";
export { ProviderIssue, Result, GitHubGetUserOptions, GitHubGetUserData, GitHubListUserRepositoriesOptions, GitHubListUserRepositoriesData, GitHubListUserPullRequestsOptions, GitHubListUserPullRequestsData, GitHubListUserActivityOptions, GitHubListUserActivityData, GitHubListFollowersOptions, GitHubListFollowersData, GitHubListFollowingOptions, GitHubListFollowingData, GitHubGetContributionsOptions, GitHubGetContributionsData, GitHubGetRepositoryOptions, GitHubGetRepositoryData, GitHubListTrendingRepositoriesOptions, GitHubListTrendingRepositoriesData, GitHubListTrendingDevelopersOptions, GitHubListTrendingDevelopersData } from "./types";
import { responseIssues, stringParam, booleanParam, integerParam, queryString, requireIdentity, authHeaders, readResponse, invalidArgument, invalidResponse, invalidResponseFrom, validateDates, validateSearchCursor, redditCommentsBody } from "./transport";
/** GitHub public-data operations through ScrapeCreators. */
export class GitHubTransport {
    /** User.
     * @param options Provider controls; omit optional values to retain provider defaults.
     * @returns One typed response and its complete raw JSON.
     */
    getUser(options: GitHubGetUserOptions = {}): Result<GitHubGetUserData> {
        const params = new Map<string, string>();
        stringParam(params, "handle", options.handle, false);
        stringParam(params, "url", options.url, false);
        requireIdentity([options.handle, options.url]);
        const response = get("https://api.scrapecreators.com/v1/github/user" + queryString(params), authHeaders());
        const raw = readResponse(response);
        try {
            const data = raw as GitHubGetUserData;
            if (typeof data.login !== "string" || data.login.trim().length === 0)
                throw invalidResponse("login is missing or blank");
            return { data: data, raw: raw, issues: responseIssues(raw) };
        }
        catch (cause) {
            throw invalidResponseFrom(cause);
        }
    }
    /** Repositories.
     * @param options Provider controls; omit optional values to retain provider defaults.
     * @returns One typed response and its complete raw JSON.
     */
    listUserRepositories(options: GitHubListUserRepositoriesOptions = {}): Result<GitHubListUserRepositoriesData> {
        const params = new Map<string, string>();
        stringParam(params, "handle", options.handle, false);
        stringParam(params, "url", options.url, false);
        stringParam(params, "type", options.type, false, ["owner", "all", "member"]);
        stringParam(params, "sort", options.sort, false, ["created", "updated", "pushed", "full_name"]);
        stringParam(params, "direction", options.direction, false, ["asc", "desc"]);
        integerParam(params, "cursor", options.cursor, false);
        requireIdentity([options.handle, options.url]);
        const response = get("https://api.scrapecreators.com/v1/github/user/repositories" + queryString(params), authHeaders());
        const raw = readResponse(response);
        try {
            const data = raw as GitHubListUserRepositoriesData;
            if (!Array.isArray(data.repositories))
                throw invalidResponse("repositories is not an array");
            return { data: data, raw: raw, issues: responseIssues(raw) };
        }
        catch (cause) {
            throw invalidResponseFrom(cause);
        }
    }
    /** Pull Requests.
     * @param options Provider controls; omit optional values to retain provider defaults.
     * @returns One typed response and its complete raw JSON.
     */
    listUserPullRequests(options: GitHubListUserPullRequestsOptions): Result<GitHubListUserPullRequestsData> {
        const params = new Map<string, string>();
        stringParam(params, "handle", options.handle, true);
        stringParam(params, "since", options.since, false);
        stringParam(params, "until", options.until, false);
        integerParam(params, "cursor", options.cursor, false);
        validateDates(options.since, options.until);
        const response = get("https://api.scrapecreators.com/v1/github/user/pull-requests" + queryString(params), authHeaders());
        const raw = readResponse(response);
        try {
            const data = raw as GitHubListUserPullRequestsData;
            if (!Array.isArray(data.pull_requests))
                throw invalidResponse("pull_requests is not an array");
            return { data: data, raw: raw, issues: responseIssues(raw) };
        }
        catch (cause) {
            throw invalidResponseFrom(cause);
        }
    }
    /** Activity.
     * @param options Provider controls; omit optional values to retain provider defaults.
     * @returns One typed response and its complete raw JSON.
     */
    listUserActivity(options: GitHubListUserActivityOptions = {}): Result<GitHubListUserActivityData> {
        const params = new Map<string, string>();
        stringParam(params, "handle", options.handle, false);
        stringParam(params, "url", options.url, false);
        integerParam(params, "year", options.year, false);
        integerParam(params, "cursor", options.cursor, false);
        requireIdentity([options.handle, options.url]);
        const response = get("https://api.scrapecreators.com/v1/github/user/activity" + queryString(params), authHeaders());
        const raw = readResponse(response);
        try {
            const data = raw as GitHubListUserActivityData;
            if (!Array.isArray(data.activity))
                throw invalidResponse("activity is not an array");
            return { data: data, raw: raw, issues: responseIssues(raw) };
        }
        catch (cause) {
            throw invalidResponseFrom(cause);
        }
    }
    /** Followers.
     * @param options Provider controls; omit optional values to retain provider defaults.
     * @returns One typed response and its complete raw JSON.
     */
    listFollowers(options: GitHubListFollowersOptions = {}): Result<GitHubListFollowersData> {
        const params = new Map<string, string>();
        stringParam(params, "handle", options.handle, false);
        stringParam(params, "url", options.url, false);
        integerParam(params, "cursor", options.cursor, false);
        requireIdentity([options.handle, options.url]);
        const response = get("https://api.scrapecreators.com/v1/github/user/followers" + queryString(params), authHeaders());
        const raw = readResponse(response);
        try {
            const data = raw as GitHubListFollowersData;
            if (!Array.isArray(data.followers))
                throw invalidResponse("followers is not an array");
            return { data: data, raw: raw, issues: responseIssues(raw) };
        }
        catch (cause) {
            throw invalidResponseFrom(cause);
        }
    }
    /** Following.
     * @param options Provider controls; omit optional values to retain provider defaults.
     * @returns One typed response and its complete raw JSON.
     */
    listFollowing(options: GitHubListFollowingOptions = {}): Result<GitHubListFollowingData> {
        const params = new Map<string, string>();
        stringParam(params, "handle", options.handle, false);
        stringParam(params, "url", options.url, false);
        integerParam(params, "cursor", options.cursor, false);
        requireIdentity([options.handle, options.url]);
        const response = get("https://api.scrapecreators.com/v1/github/user/following" + queryString(params), authHeaders());
        const raw = readResponse(response);
        try {
            const data = raw as GitHubListFollowingData;
            if (!Array.isArray(data.following))
                throw invalidResponse("following is not an array");
            return { data: data, raw: raw, issues: responseIssues(raw) };
        }
        catch (cause) {
            throw invalidResponseFrom(cause);
        }
    }
    /** Contributions.
     * @param options Provider controls; omit optional values to retain provider defaults.
     * @returns One typed response and its complete raw JSON.
     */
    getContributions(options: GitHubGetContributionsOptions = {}): Result<GitHubGetContributionsData> {
        const params = new Map<string, string>();
        stringParam(params, "handle", options.handle, false);
        stringParam(params, "url", options.url, false);
        integerParam(params, "year", options.year, false);
        requireIdentity([options.handle, options.url]);
        const response = get("https://api.scrapecreators.com/v1/github/user/contributions" + queryString(params), authHeaders());
        const raw = readResponse(response);
        try {
            const data = raw as GitHubGetContributionsData;
            if (data.contributions === null || typeof data.contributions !== "object" || Array.isArray(data.contributions))
                throw invalidResponse("contributions is not an object");
            return { data: data, raw: raw, issues: responseIssues(raw) };
        }
        catch (cause) {
            throw invalidResponseFrom(cause);
        }
    }
    /** Repository.
     * @param options Provider controls; omit optional values to retain provider defaults.
     * @returns One typed response and its complete raw JSON.
     */
    getRepository(options: GitHubGetRepositoryOptions): Result<GitHubGetRepositoryData> {
        const params = new Map<string, string>();
        stringParam(params, "url", options.url, true);
        const response = get("https://api.scrapecreators.com/v1/github/repository" + queryString(params), authHeaders());
        const raw = readResponse(response);
        try {
            const data = raw as GitHubGetRepositoryData;
            if (data.repository === null || typeof data.repository !== "object" || Array.isArray(data.repository))
                throw invalidResponse("repository is not an object");
            return { data: data, raw: raw, issues: responseIssues(raw) };
        }
        catch (cause) {
            throw invalidResponseFrom(cause);
        }
    }
    /** Trending Repositories.
     * @param options Provider controls; omit optional values to retain provider defaults.
     * @returns One typed response and its complete raw JSON.
     */
    listTrendingRepositories(options: GitHubListTrendingRepositoriesOptions = {}): Result<GitHubListTrendingRepositoriesData> {
        const params = new Map<string, string>();
        stringParam(params, "language", options.language, false);
        stringParam(params, "since", options.since, false, ["daily", "weekly", "monthly"]);
        stringParam(params, "spoken_language_code", options.spokenLanguageCode, false);
        const response = get("https://api.scrapecreators.com/v1/github/trending/repositories" + queryString(params), authHeaders());
        const raw = readResponse(response);
        try {
            const data = raw as GitHubListTrendingRepositoriesData;
            if (!Array.isArray(data.repositories))
                throw invalidResponse("repositories is not an array");
            return { data: data, raw: raw, issues: responseIssues(raw) };
        }
        catch (cause) {
            throw invalidResponseFrom(cause);
        }
    }
    /** Trending Developers.
     * @param options Provider controls; omit optional values to retain provider defaults.
     * @returns One typed response and its complete raw JSON.
     */
    listTrendingDevelopers(options: GitHubListTrendingDevelopersOptions = {}): Result<GitHubListTrendingDevelopersData> {
        const params = new Map<string, string>();
        stringParam(params, "language", options.language, false);
        stringParam(params, "since", options.since, false, ["daily", "weekly", "monthly"]);
        const response = get("https://api.scrapecreators.com/v1/github/trending/developers" + queryString(params), authHeaders());
        const raw = readResponse(response);
        try {
            const data = raw as GitHubListTrendingDevelopersData;
            if (!Array.isArray(data.developers))
                throw invalidResponse("developers is not an array");
            return { data: data, raw: raw, issues: responseIssues(raw) };
        }
        catch (cause) {
            throw invalidResponseFrom(cause);
        }
    }
}
