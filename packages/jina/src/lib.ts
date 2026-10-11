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
 * @param url Absolute URL of the page to read.
 * @param options Optional Reader settings; omit it to use Jina's defaults.
 * @returns The page content as markdown text.
 * @capability jina.ai/read { host: string }
 */
export function read(url: string, options: ReaderOptions = {}): string {
    const headers = readerHeaders(options);
    check("jina.ai/read", { host: parse(url).host });
    return readerResponse(url, headers, false).body;
}

/**
 * Read a URL through the Reader, returning the structured JSON envelope. Jina's
 * arbitrary-key `links`/`images` maps are not representable here; use `read`
 * with `withLinksSummary`/`withImagesSummary` to get them appended to the text.
 * @param url Absolute URL of the page to read.
 * @param options Optional Reader settings; omit it to use Jina's defaults.
 * @returns The page title, description, resolved URL, markdown content and billed token count.
 * @capability jina.ai/read { host: string }
 */
export function readJson(url: string, options: ReaderOptions = {}): ReaderResult {
    const headers = readerHeaders(options);
    check("jina.ai/read", { host: parse(url).host });
    return normalizeReaderJson(readerResponse(url, headers, true).body);
}

/**
 * Search the web, returning the top results concatenated as markdown.
 * @param query Search query text.
 * @param options Optional search settings; omit it to use Jina's defaults.
 * @returns The top results concatenated as markdown text.
 * @capability jina.ai/search {}
 */
export function search(query: string, options: SearchOptions = {}): string {
    const headers = searchHeaders(options);
    check("jina.ai/search", {});
    return searchResponse(query, headers, false).body;
}

/**
 * Search the web, returning the top results as structured objects.
 * @param query Search query text.
 * @param options Optional search settings; omit it to use Jina's defaults.
 * @returns One entry per search hit, in Jina's ranking order; empty when there are no hits.
 * @capability jina.ai/search {}
 */
export function searchJson(query: string, options: SearchOptions = {}): SearchResult[] {
    const headers = searchHeaders(options);
    check("jina.ai/search", {});
    return normalizeSearchJson(searchResponse(query, headers, true).body);
}

/**
 * Read a URL through the Reader and stream the markdown straight to a VFS file —
 * the body never enters Wasm memory or a JSON envelope, so payload size is bound
 * by disk, not the fuel budget. Prefer this over `read` for large pages: pair it
 * with the host's batched file-read to consume the result incrementally.
 * The file is written for the caller: its own `fs.write` rule decides whether `path` is allowed.
 * A response over 20 MB is refused.
 * @param url Absolute URL of the page to read.
 * @param path VFS path the markdown is written to.
 * @param options Optional Reader settings; omit it to use Jina's defaults.
 * @returns The download result for the file written to `path`.
 * @capability jina.ai/read { host: string }
 * @capability fs.write { path: string, max_bytes: number }
 */
export function downloadRead(
    url: string,
    path: string,
    options: ReaderOptions = {},
): DownloadResult {
    const headers = readerHeaders(options);
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
 * @param query Search query text.
 * @param path VFS path the markdown results are written to.
 * @param options Optional search settings; omit it to use Jina's defaults.
 * @returns The download result for the file written to `path`.
 * @capability jina.ai/search {}
 * @capability fs.write { path: string, max_bytes: number }
 */
export function downloadSearch(
    query: string,
    path: string,
    options: SearchOptions = {},
): DownloadResult {
    const headers = searchHeaders(options);
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
 * Each option is read from `options` exactly once.
 * @param options Reader options to encode; omit it for no headers.
 * @returns Map of `x-*` request header names to values; only options that are set appear.
 */
export function readerHeaders(options: ReaderOptions = {}): Map<string, string> {
    const headers = new Map<string, string>();
    put(headers, "x-engine", options.engine);
    put(headers, "x-target-selector", options.targetSelector);
    put(headers, "x-remove-selector", options.removeSelector);
    put(headers, "x-wait-for-selector", options.waitForSelector);
    put(headers, "x-with-links-summary", flag(options.withLinksSummary));
    put(headers, "x-with-images-summary", flag(options.withImagesSummary));
    put(headers, "x-with-generated-alt", flag(options.withGeneratedAlt));
    put(headers, "x-no-cache", flag(options.noCache));
    put(headers, "x-timeout", options.timeout?.toString());
    put(headers, "x-locale", options.locale);
    put(headers, "x-token-budget", options.tokenBudget?.toString());
    return headers;
}

/**
 * Encode search options as request headers. Pure, as with `readerHeaders`.
 * @param options Search options to encode; omit it for no headers.
 * @returns Map of `x-*` request header names to values; only options that are set appear.
 */
export function searchHeaders(options: SearchOptions = {}): Map<string, string> {
    const headers = new Map<string, string>();
    put(headers, "x-site", options.site);
    put(headers, "x-engine", options.engine);
    put(headers, "x-with-links-summary", flag(options.withLinksSummary));
    put(headers, "x-no-cache", flag(options.noCache));
    put(headers, "x-timeout", options.timeout?.toString());
    put(headers, "x-locale", options.locale);
    return headers;
}

/**
 * Decode a Reader JSON response (`readJson`) without network access. Fields Jina
 * omits or reports as `null` become empty strings and zero tokens.
 * @param body Raw JSON response body from the Reader endpoint.
 * @returns The normalized page; throws `Error` if the body is not a Reader envelope.
 */
export function normalizeReaderJson(body: string): ReaderResult {
    try {
        const data = (JSON.parse(body) as ReaderEnvelope).data;
        return {
            title: data.title ?? "",
            description: data.description ?? "",
            url: data.url,
            content: data.content ?? "",
            tokens: data.usage?.tokens ?? 0,
        };
    } catch (cause) {
        throw invalidResponse();
    }
}

/**
 * Decode a Search JSON response (`searchJson`) without network access. A `null`
 * result list means no hits; per-result fields Jina omits or reports as `null`
 * become empty strings and zero tokens.
 * @param body Raw JSON response body from the Search endpoint.
 * @returns One entry per hit, in Jina's order; throws `Error` if the body is not a Search envelope.
 */
export function normalizeSearchJson(body: string): SearchResult[] {
    try {
        const envelope = JSON.parse(body) as SearchEnvelope;
        const results: SearchResult[] = [];
        const data = envelope.data;
        if (data === null) return results;
        for (const item of data) {
            results.push({
                title: item.title ?? "",
                description: item.description ?? "",
                url: item.url,
                content: item.content ?? "",
                tokens: item.usage?.tokens ?? 0,
            });
        }
        return results;
    } catch (cause) {
        throw invalidResponse();
    }
}

interface ReaderEnvelope {
    data: ReaderData;
}

interface ReaderData {
    title?: string | null;
    description?: string | null;
    url: string;
    content?: string | null;
    usage?: Usage | null;
}

interface SearchEnvelope {
    data: SearchData[] | null;
}

interface SearchData {
    title?: string | null;
    description?: string | null;
    url: string;
    content?: string | null;
    usage?: Usage | null;
}

interface Usage {
    tokens?: number | null;
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
    if (token !== undefined) {
        headers.set("Authorization", `Bearer ${token}`);
    }
}

function put(headers: Map<string, string>, key: string, value: string | undefined): void {
    if (value !== undefined) {
        headers.set(key, value);
    }
}

function flag(value: boolean | undefined): string | undefined {
    return value === true ? "true" : undefined;
}

function invalidResponse(): Error {
    return new Error("Jina returned an invalid response");
}
