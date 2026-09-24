import { post, Response } from "submilli:http";
import secrets from "submilli:secrets";
import { check } from "submilli:security";
import { parse } from "submilli:url";

/** Select one extraction format; highlights is the default. */
export type ContentMode = "highlights" | "text";

/** Content controls are sent only when explicitly supplied. */
export interface ContentOptions {
    /** Use text only when the task needs broad page context. Default highlights. */
    mode?: ContentMode;
    /** Per-page character budget; omit unless the task requires a cap. */
    maxCharacters?: number;
    /** Cached-content age in hours: -1 cache only, 0 live crawl, up to 720. Not publication recency. */
    maxAgeHours?: number;
    /** Live crawl timeout in milliseconds, 1–90000; the HTTP transport also has its own timeout. */
    livecrawlTimeout?: number;
}

/** Search stays on Exa's default auto mode. Add filters only for explicit task constraints. */
export interface SearchOptions {
    /** Explicit result count, 1–100. Omitted uses the server default (10). */
    numResults?: number;
    /** Hard domain/path allowlist; supply only when explicitly requested. */
    includeDomains?: string[];
    /** Hard domain/path blocklist; supply only when explicitly requested. */
    excludeDomains?: string[];
    /** Hard ISO 8601 publication boundary; may exclude undated pages. */
    startPublishedDate?: string;
    /** Hard ISO 8601 publication boundary; may exclude undated pages. */
    endPublishedDate?: string;
    /** Defaults to bare highlights without a size or freshness override. */
    contents?: ContentOptions;
}

/** One source returned by Exa. Metadata may be absent on some pages. */
export interface Result {
    /** Provider document ID, or null when absent. */
    id: string | null;
    /** Source URL; keep with the content for attribution. */
    url: string;
    /** Page title, empty when absent. */
    title: string;
    /** Publisher-provided author, or null. */
    author: string | null;
    /** Publisher-provided date, or null. */
    publishedDate: string | null;
    /** Extracted passages, empty when absent. */
    highlights: string[];
    /** Extracted page text, empty when absent or highlights were requested. */
    text: string;
}

/** Search results and request metadata; there is no pagination cursor. */
export interface SearchResponse {
    /** Sources in provider order. */
    results: Result[];
    /** Provider request ID, or null. */
    requestId: string | null;
    /** Provider estimate of total dollar cost, or null; not an invoice. */
    costDollars: number | null;
}

/** Per-URL extraction outcome, including failures inside HTTP 200 responses. */
export interface ContentStatus {
    /** Requested URL/document ID. */
    id: string;
    /** Provider status, normally success or error. Unknown future values are preserved. */
    status: string;
    /** Provider cache/crawl provenance, or null. */
    source: string | null;
    /** Provider crawl failure tag, or null. */
    errorTag: string | null;
    /** HTTP status from the source page, or null. */
    httpStatusCode: number | null;
}

/** Batch extraction result. Always inspect statuses, even when results is nonempty. */
export interface ContentsResponse {
    /** Successfully extracted sources; match by ID/URL, not by array position. */
    results: Result[];
    /** Per-URL outcomes; partial and total crawl failures do not throw. */
    statuses: ContentStatus[];
    /** Provider request ID, or null. */
    requestId: string | null;
    /** Provider estimate of total dollar cost, or null. */
    costDollars: number | null;
}

/** API or validation failure. Local failures use status 0; no automatic retries. */
export class ExaError extends Error {
    code: string;
    status: number;
    retryAfter: string | null;

    constructor(code: string, message: string, status: number = 0, retryAfter: string | null = null) {
        super(message);
        this.name = "ExaError";
        this.code = code;
        this.status = status;
        this.retryAfter = retryAfter;
    }
}

/** Retrieve web sources with highlights for the calling agent to use.
 * @capability exa.ai/search {}
 */
export function search(query: string, options: SearchOptions | null = null): SearchResponse {
    check("exa.ai/search", {});
    const body = buildSearchBody(query, options);
    const response = post("https://api.exa.ai/search", body, authHeaders());
    return normalizeSearchJson(requireOk(response));
}

/** Extract known URLs. Every host must be allowed before any request is sent.
 * @capability exa.ai/contents { host: string }
 */
export function getContents(urls: string[], options: ContentOptions | null = null): ContentsResponse {
    const hosts = contentsHosts(urls);
    for (const host of hosts) check("exa.ai/contents", { host: host });
    const body = buildContentsBody(urls, options);
    const response = post("https://api.exa.ai/contents", body, authHeaders());
    return normalizeContentsJson(requireOk(response));
}

/** Build the exact search payload without credentials or network access. */
export function buildSearchBody(query: string, options: SearchOptions | null = null): string {
    requireText(query, "query");
    const opts: SearchOptions = options === null ? {} : options;
    const fields: string[] = [field("query", JSON.stringify(query)), field("contents", contentBody(opts.contents))];
    if (opts.numResults !== null) {
        integerRange(opts.numResults, 1, 100, "numResults");
        fields.push(field("numResults", JSON.stringify(opts.numResults)));
    }
    addDomains(fields, "includeDomains", opts.includeDomains);
    addDomains(fields, "excludeDomains", opts.excludeDomains);
    addDate(fields, "startPublishedDate", opts.startPublishedDate);
    addDate(fields, "endPublishedDate", opts.endPublishedDate);
    if (opts.startPublishedDate !== null && opts.endPublishedDate !== null &&
        Temporal.Instant.from(opts.startPublishedDate).epochMilliseconds > Temporal.Instant.from(opts.endPublishedDate).epochMilliseconds) {
        throw invalidArgument("startPublishedDate must not be after endPublishedDate");
    }
    return "{" + fields.join(",") + "}";
}

/** Build a known-URL request with extraction fields at the top level. */
export function buildContentsBody(urls: string[], options: ContentOptions | null = null): string {
    contentsHosts(urls);
    const extraction = contentBody(options);
    return "{" + field("urls", JSON.stringify(urls)) + "," + extraction.slice(1);
}

/** Validate a batch and return hosts for caller permission checks. */
export function contentsHosts(urls: string[]): string[] {
    integerRange(urls.length, 1, 100, "URL count");
    const hosts: string[] = [];
    for (const url of urls) {
        if (url.length > 2048 || url.trim() !== url) throw invalidArgument("URLs must be at most 2048 characters with no surrounding whitespace");
        try {
            const parsed = parse(url);
            if ((parsed.protocol !== "https" && parsed.protocol !== "http") || parsed.host.length === 0) {
                throw invalidArgument("URLs must be absolute HTTP or HTTPS URLs");
            }
            hosts.push(parsed.host);
        } catch (cause) {
            throw invalidArgument("URLs must be absolute HTTP or HTTPS URLs");
        }
    }
    return hosts;
}

interface ApiResult {
    id?: string;
    url: string;
    title?: string;
    author?: string;
    publishedDate?: string;
    highlights?: string[];
    text?: string;
}

interface ApiCost { total?: number; }
interface ApiSearch { results: ApiResult[]; requestId?: string; costDollars?: ApiCost; }
interface ApiCrawlError { tag?: string; httpStatusCode?: number; }
interface ApiStatus { id: string; status: string; source?: string; error?: ApiCrawlError; }
interface ApiContents { results: ApiResult[]; statuses: ApiStatus[]; requestId?: string; costDollars?: ApiCost; }

/** Normalize search metadata and reject malformed response shapes. */
export function normalizeSearchJson(body: string): SearchResponse {
    try {
        const data = JSON.parse(body) as ApiSearch;
        return { results: resultsFrom(data.results), requestId: data.requestId, costDollars: totalCost(data.costDollars) };
    } catch (cause) {
        throw invalidResponse();
    }
}

/** Preserve successful results and per-URL failures independently. */
export function normalizeContentsJson(body: string): ContentsResponse {
    try {
        const data = JSON.parse(body) as ApiContents;
        const statuses: ContentStatus[] = [];
        for (const item of data.statuses) {
            if (item.id.length === 0 || item.status.length === 0) throw invalidResponse();
            const error = item.error;
            statuses.push({ id: item.id, status: item.status, source: item.source,
                errorTag: error === null ? null : error.tag,
                httpStatusCode: error === null ? null : error.httpStatusCode });
        }
        return { results: resultsFrom(data.results), statuses: statuses,
            requestId: data.requestId, costDollars: totalCost(data.costDollars) };
    } catch (cause) {
        throw invalidResponse();
    }
}

/** Map HTTP status without exposing request credentials or raw response bodies. */
export function exaHttpError(status: number, retryAfter: string | null = null): ExaError {
    let code = "http_error";
    let message = "Exa request failed: HTTP " + status.toString();
    if (status === 400 || status === 422) {
        code = "invalid_request";
        message = "Exa rejected the request; check query, filters, and content options";
    } else if (status === 401) {
        code = "unauthorized";
        message = "Exa rejected EXA_API_KEY; check the bound credential";
    } else if (status === 402) {
        code = "quota_exceeded";
        message = "Exa credits are exhausted; check the account balance";
    } else if (status === 403) {
        code = "forbidden";
        message = "Exa denied access; check the API key's permissions";
    } else if (status === 429) {
        code = "rate_limited";
        message = "Exa rate limit exceeded; retry later";
    }
    return new ExaError(code, message, status, retryAfter);
}

function contentBody(options: ContentOptions | null): string {
    const opts: ContentOptions = options === null ? {} : options;
    const mode = opts.mode ?? "highlights";
    if (mode !== "highlights" && mode !== "text") throw invalidArgument("mode must be highlights or text");
    let extraction = "true";
    if (opts.maxCharacters !== null) {
        integerRange(opts.maxCharacters, 1, 9007199254740991, "maxCharacters");
        extraction = "{" + field("maxCharacters", JSON.stringify(opts.maxCharacters)) + "}";
    }
    const fields: string[] = [field(mode, extraction)];
    if (opts.maxAgeHours !== null) {
        integerRange(opts.maxAgeHours, -1, 720, "maxAgeHours");
        fields.push(field("maxAgeHours", JSON.stringify(opts.maxAgeHours)));
    }
    if (opts.livecrawlTimeout !== null) {
        integerRange(opts.livecrawlTimeout, 1, 90000, "livecrawlTimeout");
        fields.push(field("livecrawlTimeout", JSON.stringify(opts.livecrawlTimeout)));
    }
    return "{" + fields.join(",") + "}";
}

function authHeaders(): Map<string, string> {
    const key = secrets.get("EXA_API_KEY");
    if (key === null || key.trim().length === 0) {
        throw new ExaError("missing_credentials", "Bind EXA_API_KEY in the blueprint before using Exa");
    }
    const headers = new Map<string, string>();
    headers.set("x-api-key", key);
    headers.set("Content-Type", "application/json");
    headers.set("Accept", "application/json");
    return headers;
}

function requireOk(response: Response): string {
    if (!response.ok) throw exaHttpError(response.status, response.headers.get("retry-after"));
    return response.body;
}

function resultsFrom(values: ApiResult[]): Result[] {
    const results: Result[] = [];
    for (const item of values) {
        if (item.url.trim().length === 0) throw invalidResponse();
        const highlights: string[] = item.highlights === null ? [] : item.highlights;
        results.push({ id: item.id, url: item.url, title: item.title ?? "", author: item.author,
            publishedDate: item.publishedDate, highlights: highlights, text: item.text ?? "" });
    }
    return results;
}

function totalCost(cost: ApiCost | null): number | null {
    return cost === null ? null : cost.total;
}

function addDomains(fields: string[], name: string, domains: string[] | null): void {
    if (domains === null) return;
    integerRange(domains.length, 1, 1200, name + " count");
    for (const domain of domains) requireText(domain, name + " entry");
    fields.push(field(name, JSON.stringify(domains)));
}

function addDate(fields: string[], name: string, value: string | null): void {
    if (value === null) return;
    try {
        Temporal.Instant.from(value);
    } catch (cause) {
        throw invalidArgument(name + " must be an ISO 8601 date-time");
    }
    fields.push(field(name, JSON.stringify(value)));
}

function integerRange(value: number, min: number, max: number, name: string): void {
    if (!Number.isFinite(value) || value !== Math.floor(value) || value < min || value > max) {
        throw invalidArgument(name + " must be an integer between " + min.toString() + " and " + max.toString());
    }
}

function requireText(value: string, name: string): void {
    if (value.trim().length === 0) throw invalidArgument(name + " cannot be empty");
}

function field(name: string, json: string): string {
    return JSON.stringify(name) + ":" + json;
}

function invalidArgument(message: string): ExaError { return new ExaError("invalid_argument", message); }
function invalidResponse(): ExaError { return new ExaError("invalid_response", "Exa returned an invalid response"); }
