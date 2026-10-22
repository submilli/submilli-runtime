import { get } from "submilli:http";
import secrets from "submilli:secrets";
import { check } from "submilli:security";
import {
    braveHttpError,
    BraveSearchError,
    buildContextQuery,
    buildSearchQuery,
    ContextOptions,
    ContextResult,
    normalizeContextJson,
    normalizeSearchJson,
    SearchOptions,
    SearchPage,
} from "./internal";

export { SafeSearch, SearchOptions, ContextOptions, SearchItem, SearchPage, ContextItem, ContextResult, BraveSearchError } from "./internal";

/** Search one page of web results. Credentials are supplied internally.
 * @param query Search text, 1–600 characters and at most 75 words.
 * @param options Optional search settings; omitted fields use the provider defaults.
 * @returns One page of web results with query metadata and the next offset, if any.
 * @capability brave.com/search {}
 */
export function search(query: string, options: SearchOptions = {}): SearchPage {
    const params = buildSearchQuery(query, options);
    check("brave.com/search", {});
    return normalizeSearchJson(request("web/search", params), query, options.offset ?? 0);
}

/** Retrieve extracted web passages with their source URLs.
 * @param query Search text, 1–600 characters and at most 75 words.
 * @param options Optional context settings; omitted fields use the provider defaults.
 * @returns Extracted passages grouped by source page, in provider order.
 * @capability brave.com/context {}
 */
export function context(query: string, options: ContextOptions = {}): ContextResult {
    const params = buildContextQuery(query, options);
    check("brave.com/context", {});
    return normalizeContextJson(request("llm/context", params));
}

function request(path: string, params: string): string {
    const token = secrets.get("BRAVE_SEARCH_API_KEY");
    if (token === undefined || token.trim().length === 0) {
        throw new BraveSearchError("missing_credentials", "Bind BRAVE_SEARCH_API_KEY in the blueprint before using Brave Search");
    }
    const headers = new Map<string, string>();
    headers.set("X-Subscription-Token", token);
    headers.set("Accept", "application/json");
    // Keep the authority and its trailing slash static for capability derivation.
    const response = get("https://api.search.brave.com/res/v1/" + path + params, headers);
    if (!response.ok) throw braveHttpError(response.status, response.headers.get("retry-after") ?? null, response.body);
    return response.body;
}
