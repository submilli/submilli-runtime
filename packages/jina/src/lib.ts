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
import { readerHeaders, ReaderOptions, searchHeaders, SearchOptions } from "./request";
import { normalizeReaderJson, normalizeSearchJson, ReaderResult, SearchResult } from "./response";

const READER_ENDPOINT = "https://r.jina.ai/";
const SEARCH_ENDPOINT = "https://s.jina.ai/";
// The largest download written to the VFS. The caller's `fs.write` check and the transfer use the same bound.
const DOWNLOAD_MAX_BYTES = 20000000;

export { ReaderOptions, SearchOptions } from "./request";
export { ReaderResult, SearchResult } from "./response";

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
