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
 * @param query Natural-language search text; must not be blank.
 * @param options Optional result count, domain and date filters and content settings; omit it to use Exa's defaults with highlights.
 * @returns Sources in provider order with request metadata; `results` is empty when nothing matched.
 * @capability exa.ai/search {}
 */
export function search(query: string, options: SearchOptions = {}): SearchResponse {
    check("exa.ai/search", {});
    const body = buildSearchBody(query, options);
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
    const hosts = contentsHosts(ownedUrls);
    for (const host of hosts) check("exa.ai/contents", { host: host });
    const body = buildContentsBody(ownedUrls, options);
    const response = post("https://api.exa.ai/contents", body, authHeaders());
    return normalizeContentsJson(requireOk(response));
}

/**
 * Build the exact search payload without credentials or network access.
 * @param query Search text; must not be blank.
 * @param options Optional search settings; omit it to use the defaults. Throws `ExaError` with code `invalid_argument` on out-of-range values.
 * @returns JSON request body for the search endpoint.
 */
export function buildSearchBody(query: string, options: SearchOptions = {}): string {
    requireText(query, "query");
    const { numResults, includeDomains, excludeDomains, startPublishedDate, endPublishedDate, contents } = options;
    const fields: string[] = [field("query", JSON.stringify(query)), field("contents", contentBody(contents))];
    if (numResults !== undefined) {
        integerRange(numResults, 1, 100, "numResults");
        fields.push(field("numResults", JSON.stringify(numResults)));
    }
    addDomains(fields, "includeDomains", includeDomains);
    addDomains(fields, "excludeDomains", excludeDomains);
    addDate(fields, "startPublishedDate", startPublishedDate);
    addDate(fields, "endPublishedDate", endPublishedDate);
    if (startPublishedDate !== undefined && endPublishedDate !== undefined &&
        Temporal.Instant.from(startPublishedDate).epochMilliseconds > Temporal.Instant.from(endPublishedDate).epochMilliseconds) {
        throw invalidArgument("startPublishedDate must not be after endPublishedDate");
    }
    return "{" + fields.join(",") + "}";
}

/**
 * Build a known-URL request with extraction fields at the top level.
 * @param urls URLs to extract; validated like `contentsHosts`.
 * @param options Optional extraction settings; omit it to use the defaults.
 * @returns JSON request body for the contents endpoint, with extraction fields at the top level.
 */
export function buildContentsBody(urls: string[], options: ContentOptions = {}): string {
    contentsHosts(urls);
    const extraction = contentBody(options);
    return "{" + field("urls", JSON.stringify(urls)) + "," + extraction.slice(1);
}

/**
 * Validate a batch and return hosts for caller permission checks.
 * @param urls URLs to validate, 1–100 entries; each must be an absolute HTTP or HTTPS URL of at most 2048 characters with no surrounding whitespace.
 * @returns Host of each URL, in input order. Throws `ExaError` with code `invalid_argument` for an invalid batch.
 */
export function contentsHosts(urls: string[]): string[] {
    integerRange(urls.length, 1, 100, "URL count");
    const hosts: string[] = [];
    for (const url of urls) {
        if (url.length > 2048 || url.trim() !== url) throw invalidArgument("URLs must be at most 2048 characters with no surrounding whitespace");
        try {
            const parsed = parse(url);
            if ((parsed.protocol !== "https" && parsed.protocol !== "http") || /^\.*$/.test(parsed.host)) {
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
    id?: string | null;
    url: string;
    title?: string | null;
    author?: string | null;
    publishedDate?: string | null;
    highlights?: string[] | null;
    text?: string | null;
}

interface ApiCost { total?: number | null; }
interface ApiSearch { results: ApiResult[]; requestId?: string | null; costDollars?: ApiCost | null; }
interface ApiCrawlError { tag?: string | null; httpStatusCode?: number | null; }
interface ApiStatus { id: string; status: string; source?: string | null; error?: ApiCrawlError | null; }
interface ApiContents { results: ApiResult[]; statuses: ApiStatus[]; requestId?: string | null; costDollars?: ApiCost | null; }

/**
 * Normalize search metadata and reject malformed response shapes.
 * @param body Raw JSON response body from the search endpoint.
 * @returns Parsed results and metadata; throws `ExaError` with code `invalid_response` if the body is malformed.
 */
export function normalizeSearchJson(body: string): SearchResponse {
    try {
        const data = JSON.parse(body) as ApiSearch;
        return { results: resultsFrom(data.results), requestId: data.requestId ?? null, costDollars: data.costDollars?.total ?? null };
    } catch (cause) {
        throw invalidResponse();
    }
}

/**
 * Preserve successful results and per-URL failures independently.
 * @param body Raw JSON response body from the contents endpoint.
 * @returns Parsed results and per-URL statuses; throws `ExaError` with code `invalid_response` if the body is malformed.
 */
export function normalizeContentsJson(body: string): ContentsResponse {
    try {
        const data = JSON.parse(body) as ApiContents;
        const statuses: ContentStatus[] = [];
        for (const item of data.statuses) {
            if (item.id.length === 0 || item.status.length === 0) throw invalidResponse();
            statuses.push({ id: item.id, status: item.status, source: item.source ?? null,
                errorTag: item.error?.tag ?? null, httpStatusCode: item.error?.httpStatusCode ?? null });
        }
        return { results: resultsFrom(data.results), statuses: statuses,
            requestId: data.requestId ?? null, costDollars: data.costDollars?.total ?? null };
    } catch (cause) {
        throw invalidResponse();
    }
}

/**
 * Map HTTP status without exposing request credentials or raw response bodies.
 * @param status HTTP status code of the failed response.
 * @param retryAfter Value of the `Retry-After` header, or `null` when absent.
 * @returns Error with a code such as `invalid_request`, `unauthorized`, `quota_exceeded`, `forbidden`, `rate_limited` or `http_error`.
 */
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

function contentBody(options: ContentOptions = {}): string {
    const { maxCharacters, maxAgeHours, livecrawlTimeout } = options;
    const mode = options.mode ?? "highlights";
    if (mode !== "highlights" && mode !== "text") throw invalidArgument("mode must be highlights or text");
    let extraction = "true";
    if (maxCharacters !== undefined) {
        integerRange(maxCharacters, 1, 9007199254740991, "maxCharacters");
        extraction = "{" + field("maxCharacters", JSON.stringify(maxCharacters)) + "}";
    }
    const fields: string[] = [field(mode, extraction)];
    if (maxAgeHours !== undefined) {
        integerRange(maxAgeHours, -1, 720, "maxAgeHours");
        fields.push(field("maxAgeHours", JSON.stringify(maxAgeHours)));
    }
    if (livecrawlTimeout !== undefined) {
        integerRange(livecrawlTimeout, 1, 90000, "livecrawlTimeout");
        fields.push(field("livecrawlTimeout", JSON.stringify(livecrawlTimeout)));
    }
    return "{" + fields.join(",") + "}";
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

function resultsFrom(values: ApiResult[]): Result[] {
    const results: Result[] = [];
    for (const item of values) {
        if (item.url.trim().length === 0) throw invalidResponse();
        const highlights: string[] = (item.highlights === null || item.highlights === undefined) ? [] : item.highlights;
        results.push({ id: item.id ?? null, url: item.url, title: item.title ?? "", author: item.author ?? null,
            publishedDate: item.publishedDate ?? null, highlights: highlights, text: item.text ?? "" });
    }
    return results;
}

function addDomains(fields: string[], name: string, domains: string[] | undefined): void {
    if (domains === undefined) return;
    integerRange(domains.length, 1, 1200, name + " count");
    for (const domain of domains) requireText(domain, name + " entry");
    fields.push(field(name, JSON.stringify(domains)));
}

function addDate(fields: string[], name: string, value: string | undefined): void {
    if (value === undefined) return;
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
