// A small client for Jina AI (https://jina.ai): the Reader (r.jina.ai) and
// Search (s.jina.ai) endpoints. Both work keyless at a lower rate limit; a
// token raises it.
//
// The token VALUE is never passed in — this code runs LLM-generated callers,
// and a token in an argument is a token an attacker can exfiltrate. The bearer
// token is read from the `JINA_API_KEY` secret via the secrets capability at
// call time; the request goes keyless when that secret is absent.

import { post, download, Response, DownloadOptions, DownloadResult } from "submilli:http";
import secrets from "submilli:secrets";
import { check } from "submilli:security";
import { parse } from "submilli:url";

const READER_ENDPOINT = "https://r.jina.ai/";
const SEARCH_ENDPOINT = "https://s.jina.ai/";

/** Options for the Reader endpoints (`read` / `readJson`). All fields optional. */
export interface ReaderOptions {
    /**
     * Fetch engine: "auto" (default), "browser" (headless Chrome, for
     * JS-heavy pages), or "curl" (fast, static).
     */
    engine?: string;
    /** CSS selector — return only content inside the matched element(s). */
    targetSelector?: string;
    /** CSS selector — strip the matched element(s) before extraction. */
    removeSelector?: string;
    /** CSS selector — wait until it renders before returning. */
    waitForSelector?: string;
    /** Append a deduplicated footer of all links (markdown `read` only). */
    withLinksSummary?: boolean;
    /** Append a deduplicated footer of all images (markdown `read` only). */
    withImagesSummary?: boolean;
    /** Caption images lacking alt text via a vision model. */
    withGeneratedAlt?: boolean;
    /** Bypass Jina's cache for this request. */
    noCache?: boolean;
    /** Seconds to wait for page load before returning what's available (max 180). */
    timeout?: number;
    /** Browser locale for the render (affects geo/locale-sensitive pages). */
    locale?: string;
    /** Reject the request if it would exceed this token count. */
    tokenBudget?: number;
}

/** Structured page returned by `readJson`. */
export interface ReaderResult {
    /** Page title (empty when Jina reports none). */
    title: string;
    /** Page description / summary (empty when absent). */
    description: string;
    /** Final URL the Reader resolved. */
    url: string;
    /** LLM-friendly page body, as markdown. */
    content: string;
    /** Tokens Jina billed for the read (0 when not reported). */
    tokens: number;
}

/** Options for the Search endpoints (`search` / `searchJson`). All fields optional. */
export interface SearchOptions {
    /** Restrict results to a single site (domain). */
    site?: string;
    /** Fetch engine applied per result, as in `ReaderOptions.engine`. */
    engine?: string;
    /** Append a deduplicated footer of all links to each result. */
    withLinksSummary?: boolean;
    /** Bypass Jina's cache for this request. */
    noCache?: boolean;
    /** Seconds to wait per result before returning what's available (max 180). */
    timeout?: number;
    /** Browser locale for the render (affects geo/locale-sensitive pages). */
    locale?: string;
}

/** One structured search hit returned by `searchJson`. */
export interface SearchResult {
    /** Result title (empty when absent). */
    title: string;
    /** Result description / snippet (empty when absent). */
    description: string;
    /** Result URL. */
    url: string;
    /** LLM-friendly result body, as markdown (empty when absent). */
    content: string;
    /** Tokens Jina billed for this result (0 when not reported). */
    tokens: number;
}

/**
 * Read a URL through the Reader, returning Jina's LLM-friendly markdown.
 * @capability jina.ai/read { host: string }
 */
export function read(url: string, options: ReaderOptions | null = null): string {
    check("jina.ai/read", { host: parse(url).host });
    return readerResponse(url, options, false).body;
}

/**
 * Read a URL through the Reader, returning the structured JSON envelope. Jina's
 * arbitrary-key `links`/`images` maps are not representable here; use `read`
 * with `withLinksSummary`/`withImagesSummary` to get them appended to the text.
 * @capability jina.ai/read { host: string }
 */
export function readJson(url: string, options: ReaderOptions | null = null): ReaderResult {
    check("jina.ai/read", { host: parse(url).host });
    const envelope = readerResponse(url, options, true).json() as ReaderEnvelope;
    return readerResultFrom(envelope);
}

/**
 * Search the web, returning the top results concatenated as markdown.
 * @capability jina.ai/search {}
 */
export function search(query: string, options: SearchOptions | null = null): string {
    check("jina.ai/search", {});
    return searchResponse(query, options, false).body;
}

/**
 * Search the web, returning the top results as structured objects.
 * @capability jina.ai/search {}
 */
export function searchJson(query: string, options: SearchOptions | null = null): SearchResult[] {
    check("jina.ai/search", {});
    const envelope = searchResponse(query, options, true).json() as SearchEnvelope;
    return searchResultsFrom(envelope);
}

/**
 * Read a URL through the Reader and stream the markdown straight to a VFS file —
 * the body never enters Wasm memory or a JSON envelope, so payload size is bound
 * by disk, not the fuel budget. Prefer this over `read` for large pages: pair it
 * with the host's batched file-read to consume the result incrementally.
 * @capability jina.ai/read { host: string }
 */
export function downloadRead(
    url: string,
    path: string,
    options: ReaderOptions | null = null,
): DownloadResult {
    check("jina.ai/read", { host: parse(url).host });
    const headers = readerHeaders(options);
    authorize(headers);
    const downloadOptions: DownloadOptions = { headers: headers };
    return download(READER_ENDPOINT + url, path, downloadOptions);
}

/**
 * Search the web and stream the concatenated markdown results straight to a VFS
 * file, JSON-free (see `downloadRead`). Prefer this over `search` when the result
 * set is large.
 * @capability jina.ai/search {}
 */
export function downloadSearch(
    query: string,
    path: string,
    options: SearchOptions | null = null,
): DownloadResult {
    check("jina.ai/search", {});
    const headers = searchHeaders(options);
    authorize(headers);
    const downloadOptions: DownloadOptions = { headers: headers };
    return download(SEARCH_ENDPOINT + "?q=" + encodeURIComponent(query), path, downloadOptions);
}

/**
 * Encode reader options as request headers — the `x-*` option surface only.
 * Authorization is added later, in the request path, from the JINA_API_KEY
 * secret. Pure (no secrets, no network), so it is safe to unit-test directly.
 *
 * Every option is read into a local before any `put` call: a function call
 * invalidates active narrowing, so reading `options.*` after one would force a
 * re-narrow on each access.
 */
export function readerHeaders(options: ReaderOptions | null = null): Map<string, string> {
    const headers = new Map<string, string>();
    if (options !== null) {
        const engine = options.engine;
        const targetSelector = options.targetSelector;
        const removeSelector = options.removeSelector;
        const waitForSelector = options.waitForSelector;
        const withLinksSummary = options.withLinksSummary;
        const withImagesSummary = options.withImagesSummary;
        const withGeneratedAlt = options.withGeneratedAlt;
        const noCache = options.noCache;
        const timeout = options.timeout;
        const locale = options.locale;
        const tokenBudget = options.tokenBudget;

        put(headers, "x-engine", engine);
        put(headers, "x-target-selector", targetSelector);
        put(headers, "x-remove-selector", removeSelector);
        put(headers, "x-wait-for-selector", waitForSelector);
        put(headers, "x-with-links-summary", flag(withLinksSummary));
        put(headers, "x-with-images-summary", flag(withImagesSummary));
        put(headers, "x-with-generated-alt", flag(withGeneratedAlt));
        put(headers, "x-no-cache", flag(noCache));
        put(headers, "x-timeout", numStr(timeout));
        put(headers, "x-locale", locale);
        put(headers, "x-token-budget", numStr(tokenBudget));
    }
    return headers;
}

/** Encode search options as request headers. Pure, as with `readerHeaders`. */
export function searchHeaders(options: SearchOptions | null = null): Map<string, string> {
    const headers = new Map<string, string>();
    if (options !== null) {
        const site = options.site;
        const engine = options.engine;
        const withLinksSummary = options.withLinksSummary;
        const noCache = options.noCache;
        const timeout = options.timeout;
        const locale = options.locale;

        put(headers, "x-site", site);
        put(headers, "x-engine", engine);
        put(headers, "x-with-links-summary", flag(withLinksSummary));
        put(headers, "x-no-cache", flag(noCache));
        put(headers, "x-timeout", numStr(timeout));
        put(headers, "x-locale", locale);
    }
    return headers;
}

interface ReaderEnvelope {
    code: number;
    data: ReaderData;
}

interface ReaderData {
    title?: string;
    description?: string;
    url: string;
    content: string;
    usage?: Usage;
}

interface SearchEnvelope {
    code: number;
    data: SearchData[];
}

interface SearchData {
    title?: string;
    description?: string;
    url: string;
    content?: string;
    usage?: Usage;
}

interface Usage {
    tokens: number;
}

function readerResponse(url: string, options: ReaderOptions | null, json: boolean): Response {
    const headers = prepare(readerHeaders(options), json);
    const response = post(READER_ENDPOINT, { url: url }, headers);
    response.throwForStatus();
    return response;
}

function searchResponse(query: string, options: SearchOptions | null, json: boolean): Response {
    const headers = prepare(searchHeaders(options), json);
    const response = post(SEARCH_ENDPOINT, { q: query }, headers);
    response.throwForStatus();
    return response;
}

// Attach the bearer token and, for JSON responses, the Accept header. Kept out
// of the `post` call so each endpoint's URL stays a constant at the call site —
// that lets `submilli build` derive the `http.post` host filter (host == ...).
function prepare(headers: Map<string, string>, json: boolean): Map<string, string> {
    authorize(headers);
    if (json) {
        headers.set("Accept", "application/json");
    }
    return headers;
}

// Read the bearer token from the JINA_API_KEY secret and attach it. The literal
// secret name keeps the `secrets.get` capability statically filterable. Leaves
// the request keyless when the secret is absent.
function authorize(headers: Map<string, string>): void {
    const token = secrets.get("JINA_API_KEY");
    if (token !== null) {
        headers.set("Authorization", `Bearer ${token}`);
    }
}

function put(headers: Map<string, string>, key: string, value: string | null): void {
    if (value !== null) {
        headers.set(key, value);
    }
}

function flag(value: boolean | null): string | null {
    return value === true ? "true" : null;
}

function numStr(value: number | null): string | null {
    return value !== null ? value.toString() : null;
}

function readerResultFrom(envelope: ReaderEnvelope): ReaderResult {
    const data = envelope.data;
    return {
        title: orEmpty(data.title),
        description: orEmpty(data.description),
        url: data.url,
        content: data.content,
        tokens: tokensOf(data.usage),
    };
}

function searchResultsFrom(envelope: SearchEnvelope): SearchResult[] {
    const results: SearchResult[] = [];
    for (const item of envelope.data) {
        results.push({
            title: orEmpty(item.title),
            description: orEmpty(item.description),
            url: item.url,
            content: orEmpty(item.content),
            tokens: tokensOf(item.usage),
        });
    }
    return results;
}

function orEmpty(value: string | null): string {
    return value !== null ? value : "";
}

function tokensOf(usage: Usage | null): number {
    return usage !== null ? usage.tokens : 0;
}
