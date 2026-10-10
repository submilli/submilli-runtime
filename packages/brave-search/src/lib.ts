import { get } from "submilli:http";
import secrets from "submilli:secrets";
import { check } from "submilli:security";
import { encodeComponent } from "submilli:url";

/** Adult-content filtering mode. */
export type SafeSearch = "off" | "moderate" | "strict";

/** Web search options. Offset counts pages, not individual results. */
export interface SearchOptions {
    /** Provider country code; omitted uses its default. */
    country?: string;
    /** Provider language code; omitted uses its default. */
    searchLanguage?: string;
    /** pd, pw, pm, py, or YYYY-MM-DDtoYYYY-MM-DD. */
    freshness?: string;
    /** Adult-content filtering; default moderate. */
    safeSearch?: SafeSearch;
    /** 1–20; default 10. */
    count?: number;
    /** 0–9; default 0. */
    offset?: number;
    /** Default true. */
    extraSnippets?: boolean;
    /** Default true. */
    spellcheck?: boolean;
}

/** Context budgets are approximate provider-side limits. */
export interface ContextOptions {
    /** Provider country code; omitted uses its default. */
    country?: string;
    /** Provider language code; omitted uses its default. */
    searchLanguage?: string;
    /** pd, pw, pm, py, or YYYY-MM-DDtoYYYY-MM-DD. */
    freshness?: string;
    /** Adult-content filtering; default moderate. */
    safeSearch?: SafeSearch;
    /** Search candidates, 1–50; default 20. */
    count?: number;
    /** 1–50; default 10. */
    maxUrls?: number;
    /** 1024–32768; default 4096. */
    maxTokens?: number;
    /** 512–8192; default 2048. */
    maxTokensPerUrl?: number;
}

/** One web result with provider snippets. */
export interface SearchItem {
    /** Source page title. */
    title: string;
    /** Source page URL. */
    url: string;
    /** Provider description; empty when absent. */
    description: string;
    /** Alternative excerpts; empty when absent. */
    extraSnippets: string[];
    /** Provider age label, or null when absent. */
    age: string | null;
}

/** One page of results and query/pagination metadata. */
export interface SearchPage {
    /** Results in provider order. */
    items: SearchItem[];
    /** Original query, falling back to the supplied query. */
    originalQuery: string;
    /** Spellchecked query, or null when unchanged. */
    alteredQuery: string | null;
    /** Pass back as offset with the same query and count; null ends pagination. */
    nextOffset: number | null;
}

/** Extracted passages from one source. */
export interface ContextItem {
    /** Source page title. */
    title: string;
    /** Source page URL. */
    url: string;
    /** Extracted passages; empty when absent. */
    snippets: string[];
}

/** Ordered sources returned by the context endpoint. */
export interface ContextResult {
    /** Results in provider order. */
    items: ContextItem[];
}

/** Local failures have status 0. retryAfter preserves the HTTP header value. */
export class BraveSearchError extends Error {
    code: string;
    status: number;
    retryAfter: string | null;

    constructor(code: string, message: string, status: number = 0, retryAfter: string | null = null) {
        super(message);
        this.name = "BraveSearchError";
        this.code = code;
        this.status = status;
        this.retryAfter = retryAfter;
    }
}

/** Search one page of web results. Credentials are supplied internally.
 * @param query Search text, 1–600 characters and at most 75 words.
 * @param options Optional search settings; omitted fields use the provider defaults.
 * @returns One page of web results with query metadata and the next offset, if any.
 * @capability brave.com/search {}
 */
export function search(query: string, options: SearchOptions = {}): SearchPage {
    check("brave.com/search", {});
    const params = buildSearchQuery(query, options);
    return normalizeSearchJson(request("web/search", params), query, options.offset ?? 0);
}

/** Retrieve extracted web passages with their source URLs.
 * @param query Search text, 1–600 characters and at most 75 words.
 * @param options Optional context settings; omitted fields use the provider defaults.
 * @returns Extracted passages grouped by source page, in provider order.
 * @capability brave.com/context {}
 */
export function context(query: string, options: ContextOptions = {}): ContextResult {
    check("brave.com/context", {});
    return normalizeContextJson(request("llm/context", buildContextQuery(query, options)));
}

/**
 * Build encoded web parameters without credentials or network access.
 * @param query Search text, 1–600 characters and at most 75 words.
 * @param options Optional search settings; omitted fields use the defaults.
 * @returns Query string beginning with `?`, with every value percent-encoded.
 */
export function buildSearchQuery(query: string, options: SearchOptions = {}): string {
    const parts = commonQuery(query, options.country, options.searchLanguage, options.freshness, options.safeSearch);
    add(parts, "count", bounded(options.count, 10, 1, 20, "count").toString());
    add(parts, "offset", bounded(options.offset, 0, 0, 9, "offset").toString());
    add(parts, "extra_snippets", (options.extraSnippets ?? true).toString());
    add(parts, "spellcheck", (options.spellcheck ?? true).toString());
    add(parts, "result_filter", "web");
    add(parts, "text_decorations", "false");
    return "?" + parts.join("&");
}

/**
 * Build encoded context parameters without credentials or network access.
 * @param query Search text, 1–600 characters and at most 75 words.
 * @param options Optional context settings; omitted fields use the defaults.
 * @returns Query string beginning with `?`, with every value percent-encoded.
 */
export function buildContextQuery(query: string, options: ContextOptions = {}): string {
    const parts = commonQuery(query, options.country, options.searchLanguage, options.freshness, options.safeSearch);
    add(parts, "count", bounded(options.count, 20, 1, 50, "count").toString());
    add(parts, "maximum_number_of_urls", bounded(options.maxUrls, 10, 1, 50, "maxUrls").toString());
    add(parts, "maximum_number_of_tokens", bounded(options.maxTokens, 4096, 1024, 32768, "maxTokens").toString());
    add(parts, "maximum_number_of_tokens_per_url", bounded(options.maxTokensPerUrl, 2048, 512, 8192, "maxTokensPerUrl").toString());
    add(parts, "enable_local", "false");
    return "?" + parts.join("&");
}

interface ApiSearchItem {
    title: string;
    url: string;
    description?: string | null;
    extra_snippets?: string[] | null;
    age?: string | null;
}

interface ApiQuery {
    original?: string | null;
    altered?: string | null;
    more_results_available?: boolean | null;
}

interface ApiWeb {
    results: ApiSearchItem[];
}

interface ApiSearch {
    type: string;
    query?: ApiQuery | null;
    web?: ApiWeb | null;
}

interface ApiContextItem {
    title: string;
    url: string;
    snippets?: string[] | null;
}

interface ApiGrounding {
    generic?: ApiContextItem[] | null;
}

interface ApiContext {
    grounding: ApiGrounding;
}

/**
 * Normalize a Brave response; unknown provider fields are ignored.
 * @param body Raw JSON response body from the web search endpoint.
 * @param query Query used as the fallback for `originalQuery`.
 * @param offset Page offset of this request; `nextOffset` is derived from it.
 * @returns Parsed page; throws `BraveSearchError` with code `invalid_response` if the body is malformed.
 */
export function normalizeSearchJson(body: string, query: string, offset: number): SearchPage {
    try {
        const data = JSON.parse(body) as ApiSearch;
        if (data.type !== "search") throw invalidResponse();
        const items: SearchItem[] = [];
        const results = data.web?.results;
        if (results !== undefined) {
            for (const item of results) {
                requireSource(item.title, item.url);
                items.push({ title: item.title, url: item.url, description: item.description ?? "",
                    extraSnippets: strings(item.extra_snippets), age: item.age ?? null });
            }
        }
        const meta = data.query;
        return {
            items: items,
            originalQuery: meta?.original ?? query,
            alteredQuery: meta?.altered ?? null,
            nextOffset: meta?.more_results_available === true && offset < 9 ? offset + 1 : null,
        };
    } catch (cause) {
        throw invalidResponse();
    }
}

/**
 * Normalize extracted passages while keeping each passage tied to its source.
 * @param body Raw JSON response body from the context endpoint.
 * @returns Sources with their extracted passages; throws `BraveSearchError` with code `invalid_response` if the body is malformed.
 */
export function normalizeContextJson(body: string): ContextResult {
    try {
        const data = JSON.parse(body) as ApiContext;
        const items: ContextItem[] = [];
        const generic = data.grounding.generic;
        if (generic !== null && generic !== undefined) {
            for (const item of generic) {
                requireSource(item.title, item.url);
                items.push({ title: item.title, url: item.url, snippets: strings(item.snippets) });
            }
        }
        return { items: items };
    } catch (cause) {
        throw invalidResponse();
    }
}

interface ApiError {
    type?: string | null;
    error?: { detail?: string | null } | null;
}

/**
 * Map HTTP failures, preserving the provider's error type and detail when available.
 * @param status HTTP status code of the failed response.
 * @param retryAfter Value of the `Retry-After` header, or `null` when absent.
 * @param body Raw response body, used to append the provider's error type and detail; empty or non-JSON adds nothing.
 * @returns Error with a code such as `unauthorized`, `forbidden`, `rate_limited` or `http_error`.
 */
export function braveHttpError(status: number, retryAfter: string | null = null, body: string = ""): BraveSearchError {
    let code = "http_error";
    let message = "Brave Search request failed: HTTP " + status.toString();
    if (status === 401) {
        code = "unauthorized";
        message = "Brave Search rejected BRAVE_SEARCH_API_KEY; check the bound credential";
    } else if (status === 403) {
        code = "forbidden";
        message = "Brave Search denied access; check the API key's plan and permissions";
    } else if (status === 429) {
        code = "rate_limited";
        message = "Brave Search rate limit exceeded; retry later";
    }
    const detail = braveErrorDetail(body);
    if (detail.length > 0) message += ": " + detail;
    return new BraveSearchError(code, message, status, retryAfter);
}

function braveErrorDetail(body: string): string {
    try {
        const data = JSON.parse(body) as ApiError;
        const parts: string[] = [];
        const type = data.type ?? "";
        const detail = data.error?.detail ?? "";
        if (type.length > 0) parts.push(type);
        if (detail.length > 0) parts.push(detail);
        return parts.join(": ");
    } catch (cause) {
        return "";
    }
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

function commonQuery(query: string, country: string | undefined, language: string | undefined,
    freshness: string | undefined, safeSearch: SafeSearch | undefined): string[] {
    const trimmed = query.trim();
    if (trimmed.length === 0 || query.length > 600 || trimmed.split(/\s+/).length > 75) {
        throw new BraveSearchError("invalid_argument", "query must contain 1–600 characters and at most 75 words");
    }
    const safe = safeSearch ?? "moderate";
    if (safe !== "off" && safe !== "moderate" && safe !== "strict") {
        throw new BraveSearchError("invalid_argument", "safeSearch must be off, moderate, or strict");
    }
    const parts: string[] = [];
    add(parts, "q", query);
    if (country !== undefined) add(parts, "country", country);
    if (language !== undefined) add(parts, "search_lang", language);
    if (freshness !== undefined) add(parts, "freshness", freshness);
    add(parts, "safesearch", safe);
    return parts;
}

function bounded(value: number | undefined, fallback: number, min: number, max: number, name: string): number {
    const actual = value ?? fallback;
    if (!Number.isFinite(actual) || actual !== Math.floor(actual) || actual < min || actual > max) {
        throw new BraveSearchError("invalid_argument", name + " must be an integer between " + min.toString() + " and " + max.toString());
    }
    return actual;
}

function add(parts: string[], name: string, value: string): void {
    parts.push(name + "=" + encodeComponent(value));
}

function requireSource(title: string, url: string): void {
    if (title.trim().length === 0 || url.trim().length === 0) throw invalidResponse();
}

function invalidResponse(): BraveSearchError {
    return new BraveSearchError("invalid_response", "Brave Search returned an invalid response");
}

function strings(value: string[] | null | undefined): string[] {
    return value === null || value === undefined ? [] : value;
}
