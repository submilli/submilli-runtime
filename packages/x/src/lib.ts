import { get, Response } from "submilli:http";
import secrets from "submilli:secrets";
import { check } from "submilli:security";
import { SearchOptions, ReplyOptions, TweetResult, TweetPage, XError } from "./types";
import { tweetId, tweetFieldsQuery, buildSearchQuery, decodeTweet, decodePage, xHttpError } from "./internal";

export { SearchOptions, ReplyOptions, User, Tweet, TweetMetrics, TweetReference, ApiIssue, TweetResult, TweetPage, XError } from "./types";

/** Retrieve a public tweet by numeric ID or X/Twitter post URL.
 * @param idOrUrl Tweet ID string or HTTPS X/Twitter post URL.
 * @returns Typed tweet and any partial author-expansion issues.
 * @capability x.com/getTweet { id: string }
 */
export function getTweet(idOrUrl: string): TweetResult {
    const id = tweetId(idOrUrl);
    check("x.com/getTweet", { id: id });
    return decodeTweet(requireOk(get("https://api.x.com/2/tweets/" + id + "?" + tweetFieldsQuery(), authHeaders())), id);
}

/** Search one page of public tweets from the last seven days.
 * @param query X advanced search expression, 1–4096 characters.
 * @param options Page, date, and sort controls; defaults to 10 recent results.
 * @returns One page with an explicit continuation cursor and partial issues.
 * @capability x.com/searchRecent {}
 */
export function searchRecent(query: string, options: SearchOptions = {}): TweetPage {
    const params = buildSearchQuery(query, options, 100);
    check("x.com/searchRecent", {});
    return decodePage(requireOk(get("https://api.x.com/2/tweets/search/recent?" + params, authHeaders())));
}

/** Search one page of the full public tweet archive; requires archive API access.
 * @param query X advanced search expression, 1–4096 characters.
 * @param options Page, date, and sort controls; defaults to 10 recent results.
 * @returns One archive page with an explicit continuation cursor and partial issues.
 * @capability x.com/searchAll {}
 */
export function searchAll(query: string, options: SearchOptions = {}): TweetPage {
    const params = buildSearchQuery(query, options, 500);
    check("x.com/searchAll", {});
    return decodePage(requireOk(get("https://api.x.com/2/tweets/search/all?" + params, authHeaders())));
}

/** List one page of replies, including nested replies, to a conversation root.
 * @param conversationId Root tweet ID or URL; read conversationId from getTweet for a nested reply.
 * @param options Search controls, plus archive to include replies older than seven days. Requires x.com/searchRecent or x.com/searchAll respectively.
 * @returns One page of conversation replies; the original post is excluded.
 */
export function listReplies(conversationId: string, options: ReplyOptions = {}): TweetPage {
    const id = tweetId(conversationId);
    const archive = options.archive ?? false;
    const query = "conversation_id:" + id + " is:reply";
    if (archive) return searchAll(query, options);
    return searchRecent(query, options);
}

function authHeaders(): Map<string, string> {
    const token = secrets.get("X_BEARER_TOKEN");
    if (token === undefined || token.trim().length === 0) throw new XError("missing_credentials", "Bind X_BEARER_TOKEN in the blueprint before using X");
    const headers = new Map<string, string>();
    headers.set("Authorization", "Bearer " + token);
    headers.set("Accept", "application/json");
    return headers;
}

function requireOk(response: Response): string {
    if (!response.ok) throw xHttpError(response.status, response.body, response.headers.get("retry-after") ?? null, response.headers.get("x-rate-limit-reset") ?? null);
    return response.body;
}
