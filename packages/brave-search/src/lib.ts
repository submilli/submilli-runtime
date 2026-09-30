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
 * @capability brave.com/search {}
 */
export function search(query: string, options: SearchOptions | null = null): SearchPage {
    const country = options === null ? null : options.country;
    const searchLanguage = options === null ? null : options.searchLanguage;
    const freshness = options === null ? null : options.freshness;
    const safeSearch = options === null ? null : options.safeSearch;
    const count = options === null ? null : options.count;
    const offset = options === null ? null : options.offset;
    const extraSnippets = options === null ? null : options.extraSnippets;
    const spellcheck = options === null ? null : options.spellcheck;
    const searchOptions: SearchOptions = {};
    if (country !== null) searchOptions.country = country;
    if (searchLanguage !== null) searchOptions.searchLanguage = searchLanguage;
    if (freshness !== null) searchOptions.freshness = freshness;
    if (safeSearch !== null) searchOptions.safeSearch = safeSearch;
    if (count !== null) searchOptions.count = count;
    if (offset !== null) searchOptions.offset = offset;
    if (extraSnippets !== null) searchOptions.extraSnippets = extraSnippets;
    if (spellcheck !== null) searchOptions.spellcheck = spellcheck;
    check("brave.com/search", {});
    const params = buildSearchQuery(query, searchOptions);
    const body = request("web/search", params);
    return normalizeSearchJson(body, query, offset ?? 0);
}

/** Retrieve extracted web passages with their source URLs.
 * @capability brave.com/context {}
 */
export function context(query: string, options: ContextOptions | null = null): ContextResult {
    const country = options === null ? null : options.country;
    const searchLanguage = options === null ? null : options.searchLanguage;
    const freshness = options === null ? null : options.freshness;
    const safeSearch = options === null ? null : options.safeSearch;
    const count = options === null ? null : options.count;
    const maxUrls = options === null ? null : options.maxUrls;
    const maxTokens = options === null ? null : options.maxTokens;
    const maxTokensPerUrl = options === null ? null : options.maxTokensPerUrl;
    const contextOptions: ContextOptions = {};
    if (country !== null) contextOptions.country = country;
    if (searchLanguage !== null) contextOptions.searchLanguage = searchLanguage;
    if (freshness !== null) contextOptions.freshness = freshness;
    if (safeSearch !== null) contextOptions.safeSearch = safeSearch;
    if (count !== null) contextOptions.count = count;
    if (maxUrls !== null) contextOptions.maxUrls = maxUrls;
    if (maxTokens !== null) contextOptions.maxTokens = maxTokens;
    if (maxTokensPerUrl !== null) contextOptions.maxTokensPerUrl = maxTokensPerUrl;
    check("brave.com/context", {});
    return normalizeContextJson(request("llm/context", buildContextQuery(query, contextOptions)));
}

/** Build encoded web parameters without credentials or network access. */
export function buildSearchQuery(query: string, options: SearchOptions | null = null): string {
    const opts: SearchOptions = options === null ? {} : options;
    const parts = commonQuery(query, opts.country, opts.searchLanguage, opts.freshness, opts.safeSearch);
    add(parts, "count", bounded(opts.count, 10, 1, 20, "count").toString());
    add(parts, "offset", bounded(opts.offset, 0, 0, 9, "offset").toString());
    add(parts, "extra_snippets", (opts.extraSnippets ?? true).toString());
    add(parts, "spellcheck", (opts.spellcheck ?? true).toString());
    add(parts, "result_filter", "web");
    add(parts, "text_decorations", "false");
    return "?" + parts.join("&");
}

/** Build encoded context parameters without credentials or network access. */
export function buildContextQuery(query: string, options: ContextOptions | null = null): string {
    const opts: ContextOptions = options === null ? {} : options;
    const parts = commonQuery(query, opts.country, opts.searchLanguage, opts.freshness, opts.safeSearch);
    add(parts, "count", bounded(opts.count, 20, 1, 50, "count").toString());
    add(parts, "maximum_number_of_urls", bounded(opts.maxUrls, 10, 1, 50, "maxUrls").toString());
    add(parts, "maximum_number_of_tokens", bounded(opts.maxTokens, 4096, 1024, 32768, "maxTokens").toString());
    add(parts, "maximum_number_of_tokens_per_url", bounded(opts.maxTokensPerUrl, 2048, 512, 8192, "maxTokensPerUrl").toString());
    add(parts, "enable_local", "false");
    return "?" + parts.join("&");
}

interface ApiSearchItem {
    title: string;
    url: string;
    description?: string;
    extra_snippets?: string[];
    age?: string;
}

interface ApiQuery {
    original?: string;
    altered?: string;
    more_results_available?: boolean;
}

interface ApiWeb {
    results: ApiSearchItem[];
}

interface ApiSearch {
    type: string;
    query?: ApiQuery;
    web?: ApiWeb;
}

interface ApiContextItem {
    title: string;
    url: string;
    snippets?: string[];
}

interface ApiGrounding {
    generic?: ApiContextItem[];
}

interface ApiContext {
    grounding: ApiGrounding;
}

/** Normalize a Brave response; unknown provider fields are ignored. */
export function normalizeSearchJson(body: string, query: string, offset: number): SearchPage {
    try {
        const data = JSON.parse(body) as ApiSearch;
        if (data.type !== "search") throw invalidResponse();
        const items: SearchItem[] = [];
        if (data.web !== null) {
            for (const item of data.web.results) {
                requireSource(item.title, item.url);
                items.push({ title: item.title, url: item.url, description: item.description ?? "",
                    extraSnippets: strings(item.extra_snippets), age: item.age });
            }
        }
        const meta = data.query;
        return {
            items: items,
            originalQuery: meta === null ? query : meta.original ?? query,
            alteredQuery: meta === null ? null : meta.altered,
            nextOffset: meta !== null && meta.more_results_available === true && offset < 9 ? offset + 1 : null,
        };
    } catch (cause) {
        throw invalidResponse();
    }
}

/** Normalize extracted passages while keeping each passage tied to its source. */
export function normalizeContextJson(body: string): ContextResult {
    try {
        const data = JSON.parse(body) as ApiContext;
        const items: ContextItem[] = [];
        const generic = data.grounding.generic;
        if (generic !== null) {
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

/** Map HTTP failures without including provider bodies or credentials. */
export function braveHttpError(status: number, retryAfter: string | null = null): BraveSearchError {
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
    return new BraveSearchError(code, message, status, retryAfter);
}

function request(path: string, params: string): string {
    const token = secrets.get("BRAVE_SEARCH_API_KEY");
    if (token === null || token.trim().length === 0) {
        throw new BraveSearchError("missing_credentials", "Bind BRAVE_SEARCH_API_KEY in the blueprint before using Brave Search");
    }
    const headers = new Map<string, string>();
    headers.set("X-Subscription-Token", token);
    headers.set("Accept", "application/json");
    // Keep the authority and its trailing slash static for capability derivation.
    const response = get("https://api.search.brave.com/res/v1/" + path + params, headers);
    if (!response.ok) throw braveHttpError(response.status, response.headers.get("retry-after"));
    return response.body;
}

function commonQuery(query: string, country: string | null, language: string | null,
    freshness: string | null, safeSearch: SafeSearch | null): string[] {
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
    if (country !== null) add(parts, "country", country);
    if (language !== null) add(parts, "search_lang", language);
    if (freshness !== null) add(parts, "freshness", freshness);
    add(parts, "safesearch", safe);
    return parts;
}

function bounded(value: number | null, fallback: number, min: number, max: number, name: string): number {
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

function strings(value: string[] | null): string[] {
    return value === null ? [] : value;
}
