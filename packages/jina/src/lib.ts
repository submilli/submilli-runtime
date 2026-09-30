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
// The largest download written to the VFS. The caller's `fs.write` check and the transfer use the same bound.
const DOWNLOAD_MAX_BYTES = 20000000;

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
    const engine = options === null ? null : options.engine;
    const targetSelector = options === null ? null : options.targetSelector;
    const removeSelector = options === null ? null : options.removeSelector;
    const waitForSelector = options === null ? null : options.waitForSelector;
    const withLinksSummary = options === null ? null : options.withLinksSummary;
    const withImagesSummary = options === null ? null : options.withImagesSummary;
    const withGeneratedAlt = options === null ? null : options.withGeneratedAlt;
    const noCache = options === null ? null : options.noCache;
    const timeout = options === null ? null : options.timeout;
    const locale = options === null ? null : options.locale;
    const tokenBudget = options === null ? null : options.tokenBudget;
    const headers = readerHeadersFrom({
        engine: engine, targetSelector: targetSelector, removeSelector: removeSelector,
        waitForSelector: waitForSelector, withLinksSummary: withLinksSummary,
        withImagesSummary: withImagesSummary, withGeneratedAlt: withGeneratedAlt, noCache: noCache,
        timeout: timeout, locale: locale, tokenBudget: tokenBudget,
    });
    check("jina.ai/read", { host: parse(url).host });
    return readerResponse(url, headers, false).body;
}

/**
 * Read a URL through the Reader, returning the structured JSON envelope. Jina's
 * arbitrary-key `links`/`images` maps are not representable here; use `read`
 * with `withLinksSummary`/`withImagesSummary` to get them appended to the text.
 * @capability jina.ai/read { host: string }
 */
export function readJson(url: string, options: ReaderOptions | null = null): ReaderResult {
    const engine = options === null ? null : options.engine;
    const targetSelector = options === null ? null : options.targetSelector;
    const removeSelector = options === null ? null : options.removeSelector;
    const waitForSelector = options === null ? null : options.waitForSelector;
    const withLinksSummary = options === null ? null : options.withLinksSummary;
    const withImagesSummary = options === null ? null : options.withImagesSummary;
    const withGeneratedAlt = options === null ? null : options.withGeneratedAlt;
    const noCache = options === null ? null : options.noCache;
    const timeout = options === null ? null : options.timeout;
    const locale = options === null ? null : options.locale;
    const tokenBudget = options === null ? null : options.tokenBudget;
    const headers = readerHeadersFrom({
        engine: engine, targetSelector: targetSelector, removeSelector: removeSelector,
        waitForSelector: waitForSelector, withLinksSummary: withLinksSummary,
        withImagesSummary: withImagesSummary, withGeneratedAlt: withGeneratedAlt, noCache: noCache,
        timeout: timeout, locale: locale, tokenBudget: tokenBudget,
    });
    check("jina.ai/read", { host: parse(url).host });
    const envelope = readerResponse(url, headers, true).json() as ReaderEnvelope;
    return readerResultFrom(envelope);
}

/**
 * Search the web, returning the top results concatenated as markdown.
 * @capability jina.ai/search {}
 */
export function search(query: string, options: SearchOptions | null = null): string {
    const site = options === null ? null : options.site;
    const engine = options === null ? null : options.engine;
    const withLinksSummary = options === null ? null : options.withLinksSummary;
    const noCache = options === null ? null : options.noCache;
    const timeout = options === null ? null : options.timeout;
    const locale = options === null ? null : options.locale;
    const headers = searchHeadersFrom({
        site: site, engine: engine, withLinksSummary: withLinksSummary,
        noCache: noCache, timeout: timeout, locale: locale,
    });
    check("jina.ai/search", {});
    return searchResponse(query, headers, false).body;
}

/**
 * Search the web, returning the top results as structured objects.
 * @capability jina.ai/search {}
 */
export function searchJson(query: string, options: SearchOptions | null = null): SearchResult[] {
    const site = options === null ? null : options.site;
    const engine = options === null ? null : options.engine;
    const withLinksSummary = options === null ? null : options.withLinksSummary;
    const noCache = options === null ? null : options.noCache;
    const timeout = options === null ? null : options.timeout;
    const locale = options === null ? null : options.locale;
    const headers = searchHeadersFrom({
        site: site, engine: engine, withLinksSummary: withLinksSummary,
        noCache: noCache, timeout: timeout, locale: locale,
    });
    check("jina.ai/search", {});
    const envelope = searchResponse(query, headers, true).json() as SearchEnvelope;
    return searchResultsFrom(envelope);
}

/**
 * Read a URL through the Reader and stream the markdown straight to a VFS file —
 * the body never enters Wasm memory or a JSON envelope, so payload size is bound
 * by disk, not the fuel budget. Prefer this over `read` for large pages: pair it
 * with the host's batched file-read to consume the result incrementally.
 * The file is written for the caller: its own `fs.write` rule decides whether `path` is allowed.
 * A response over 20 MB is refused.
 * @capability jina.ai/read { host: string }
 * @capability fs.write { path: string, max_bytes: number }
 */
export function downloadRead(
    url: string,
    path: string,
    options: ReaderOptions | null = null,
): DownloadResult {
    const engine = options === null ? null : options.engine;
    const targetSelector = options === null ? null : options.targetSelector;
    const removeSelector = options === null ? null : options.removeSelector;
    const waitForSelector = options === null ? null : options.waitForSelector;
    const withLinksSummary = options === null ? null : options.withLinksSummary;
    const withImagesSummary = options === null ? null : options.withImagesSummary;
    const withGeneratedAlt = options === null ? null : options.withGeneratedAlt;
    const noCache = options === null ? null : options.noCache;
    const timeout = options === null ? null : options.timeout;
    const locale = options === null ? null : options.locale;
    const tokenBudget = options === null ? null : options.tokenBudget;
    const headers = readerHeadersFrom({
        engine: engine, targetSelector: targetSelector, removeSelector: removeSelector,
        waitForSelector: waitForSelector, withLinksSummary: withLinksSummary,
        withImagesSummary: withImagesSummary, withGeneratedAlt: withGeneratedAlt, noCache: noCache,
        timeout: timeout, locale: locale, tokenBudget: tokenBudget,
    });
    check("jina.ai/read", { host: parse(url).host });
    check("fs.write", { path: path, max_bytes: DOWNLOAD_MAX_BYTES });
    authorize(headers);
    const downloadOptions: DownloadOptions = { headers: headers, maxBytes: DOWNLOAD_MAX_BYTES };
    // Keep target path segments, queries, and fragments out of the Reader URL's
    // syntax so normalization cannot replace the host checked above.
    return download(READER_ENDPOINT + encodeURIComponent(url), path, downloadOptions);
}

/**
 * Search the web and stream the concatenated markdown results straight to a VFS
 * file, JSON-free (see `downloadRead`). Prefer this over `search` when the result
 * set is large.
 * @capability jina.ai/search {}
 * @capability fs.write { path: string, max_bytes: number }
 */
export function downloadSearch(
    query: string,
    path: string,
    options: SearchOptions | null = null,
): DownloadResult {
    const site = options === null ? null : options.site;
    const engine = options === null ? null : options.engine;
    const withLinksSummary = options === null ? null : options.withLinksSummary;
    const noCache = options === null ? null : options.noCache;
    const timeout = options === null ? null : options.timeout;
    const locale = options === null ? null : options.locale;
    const headers = searchHeadersFrom({
        site: site, engine: engine, withLinksSummary: withLinksSummary,
        noCache: noCache, timeout: timeout, locale: locale,
    });
    check("jina.ai/search", {});
    check("fs.write", { path: path, max_bytes: DOWNLOAD_MAX_BYTES });
    authorize(headers);
    const downloadOptions: DownloadOptions = { headers: headers, maxBytes: DOWNLOAD_MAX_BYTES };
    return download(SEARCH_ENDPOINT + "?q=" + encodeURIComponent(query), path, downloadOptions);
}

/**
 * Encode reader options as request headers — the `x-*` option surface only.
 * Authorization is added later, in the request path, from the JINA_API_KEY
 * secret. Pure (no secrets, no network), so it is safe to unit-test directly.
 *
 * Each option is read from `options` exactly once, into a local, and the
 * headers are encoded from those locals by `readerHeadersFrom`.
 */
export function readerHeaders(options: ReaderOptions | null = null): Map<string, string> {
    if (options === null) return new Map<string, string>();
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
    return readerHeadersFrom({
        engine: engine, targetSelector: targetSelector, removeSelector: removeSelector,
        waitForSelector: waitForSelector, withLinksSummary: withLinksSummary,
        withImagesSummary: withImagesSummary, withGeneratedAlt: withGeneratedAlt, noCache: noCache,
        timeout: timeout, locale: locale, tokenBudget: tokenBudget,
    });
}

/** Encode search options as request headers. Pure, as with `readerHeaders`. */
export function searchHeaders(options: SearchOptions | null = null): Map<string, string> {
    if (options === null) return new Map<string, string>();
    const site = options.site;
    const engine = options.engine;
    const withLinksSummary = options.withLinksSummary;
    const noCache = options.noCache;
    const timeout = options.timeout;
    const locale = options.locale;
    return searchHeadersFrom({
        site: site, engine: engine, withLinksSummary: withLinksSummary,
        noCache: noCache, timeout: timeout, locale: locale,
    });
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

interface ReaderFields {
    engine: string | null;
    targetSelector: string | null;
    removeSelector: string | null;
    waitForSelector: string | null;
    withLinksSummary: boolean | null;
    withImagesSummary: boolean | null;
    withGeneratedAlt: boolean | null;
    noCache: boolean | null;
    timeout: number | null;
    locale: string | null;
    tokenBudget: number | null;
}

interface SearchFields {
    site: string | null;
    engine: string | null;
    withLinksSummary: boolean | null;
    noCache: boolean | null;
    timeout: number | null;
    locale: string | null;
}

function readerHeadersFrom(fields: ReaderFields): Map<string, string> {
    const headers = new Map<string, string>();
    put(headers, "x-engine", fields.engine);
    put(headers, "x-target-selector", fields.targetSelector);
    put(headers, "x-remove-selector", fields.removeSelector);
    put(headers, "x-wait-for-selector", fields.waitForSelector);
    put(headers, "x-with-links-summary", flag(fields.withLinksSummary));
    put(headers, "x-with-images-summary", flag(fields.withImagesSummary));
    put(headers, "x-with-generated-alt", flag(fields.withGeneratedAlt));
    put(headers, "x-no-cache", flag(fields.noCache));
    put(headers, "x-timeout", numStr(fields.timeout));
    put(headers, "x-locale", fields.locale);
    put(headers, "x-token-budget", numStr(fields.tokenBudget));
    return headers;
}

function searchHeadersFrom(fields: SearchFields): Map<string, string> {
    const headers = new Map<string, string>();
    put(headers, "x-site", fields.site);
    put(headers, "x-engine", fields.engine);
    put(headers, "x-with-links-summary", flag(fields.withLinksSummary));
    put(headers, "x-no-cache", flag(fields.noCache));
    put(headers, "x-timeout", numStr(fields.timeout));
    put(headers, "x-locale", fields.locale);
    return headers;
}

function readerResponse(url: string, optionHeaders: Map<string, string>, json: boolean): Response {
    const headers = prepare(optionHeaders, json);
    const response = post(READER_ENDPOINT, { url: url }, headers);
    response.throwForStatus();
    return response;
}

function searchResponse(query: string, optionHeaders: Map<string, string>, json: boolean): Response {
    const headers = prepare(optionHeaders, json);
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
