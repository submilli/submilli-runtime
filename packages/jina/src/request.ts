// Request encoding for the Jina client: reader and search options become `x-*`
// request headers. Internal — `src/lib.ts` re-exports the option types and
// calls the encoders; programs never import this module.

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
/**
 * Encode reader options as request headers — the `x-*` option surface only.
 * Authorization is added later, in the request path, from the JINA_API_KEY
 * secret. Pure: no secrets, no network.
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
function put(headers: Map<string, string>, key: string, value: string | undefined): void {
    if (value !== undefined) {
        headers.set(key, value);
    }
}

function flag(value: boolean | undefined): string | undefined {
    return value === true ? "true" : undefined;
}
