import { post, Response } from "submilli:http";
import secrets from "submilli:secrets";
import { check } from "submilli:security";
import {
    buildContentsBody,
    buildSearchBody,
    ContentOptions,
    ContentsResponse,
    contentsHosts,
    exaHttpError,
    ExaError,
    normalizeContentsJson,
    normalizeSearchJson,
    SearchOptions,
    SearchResponse,
} from "./internal";

export { ContentMode, ContentOptions, SearchOptions, Result, SearchResponse, ContentStatus, ContentsResponse, ExaError } from "./internal";

/** Retrieve web sources with highlights for the calling agent to use.
 * @param query Natural-language search text; must not be blank.
 * @param options Optional result count, domain and date filters and content settings; omit it to use Exa's defaults with highlights.
 * @returns Sources in provider order with request metadata; `results` is empty when nothing matched.
 * @capability exa.ai/search {}
 */
export function search(query: string, options: SearchOptions = {}): SearchResponse {
    const body = buildSearchBody(query, options);
    check("exa.ai/search", {});
    const response = post("https://api.exa.ai/search", body, authHeaders());
    return normalizeSearchJson(requireOk(response));
}

/** Extract known URLs. Every host must be allowed before any request is sent.
 * @param urls Absolute HTTP or HTTPS URLs to extract, 1–100 entries of at most 2048 characters each.
 * @param options Optional extraction settings; omit it to use highlights with Exa's defaults.
 * @returns Extracted sources plus a per-URL status list; crawl failures appear in `statuses` rather than throwing.
 * @capability exa.ai/contents { host: string }
 */
export function getContents(urls: string[], options: ContentOptions = {}): ContentsResponse {
    // The checked hosts and the request body come from one copy of the caller's array.
    const ownedUrls: string[] = [];
    for (const url of urls) ownedUrls.push(url);
    // Built before the checks, so invalid input fails before any check.
    const { mode, maxCharacters, maxAgeHours, livecrawlTimeout } = options;
    const body = buildContentsBody(ownedUrls, { mode, maxCharacters, maxAgeHours, livecrawlTimeout });
    const hosts = contentsHosts(ownedUrls);
    for (const host of hosts) check("exa.ai/contents", { host: host });
    const response = post("https://api.exa.ai/contents", body, authHeaders());
    return normalizeContentsJson(requireOk(response));
}

function authHeaders(): Map<string, string> {
    const key = secrets.get("EXA_API_KEY");
    if (key === undefined || key.trim().length === 0) {
        throw new ExaError("missing_credentials", "Bind EXA_API_KEY in the blueprint before using Exa");
    }
    const headers = new Map<string, string>();
    headers.set("x-api-key", key);
    headers.set("Content-Type", "application/json");
    headers.set("Accept", "application/json");
    return headers;
}

function requireOk(response: Response): string {
    if (!response.ok) throw exaHttpError(response.status, response.headers.get("retry-after") ?? null);
    return response.body;
}
