import { get, post, delete as del, download, Response, DownloadResult } from "submilli:http";
import secrets from "submilli:secrets";
import { check } from "submilli:security";
import { parse } from "submilli:url";

const BASE = "https://api.firecrawl.dev/v2";

/** Select the batch-scrape or crawl job family. */
export type JobKind = "batch" | "crawl";
/** Control sitemap discovery. */
export type SitemapMode = "include" | "skip" | "only";
/** Ordinary retrieval formats, without provider synthesis. */
export type Format = "markdown" | "html";

/** Optional provider-generated extraction. Ordinary markdown/HTML needs no LLM. */
export interface JsonOptions {
    /** JSON Schema object; preserved as JSON, not interpreted by this package. */
    schema?: unknown;
    /** Instructions for optional provider extraction. */
    prompt?: string;
}

/** Deliberately excludes headers, actions, webhooks and browser sessions. */
export interface ScrapeOptions {
    /** Default ["markdown"]. Use [] with json for JSON-only extraction. */
    formats?: Format[];
    /** Optional structured extraction; absent by default. */
    json?: JsonOptions;
    /** Select main content, excluding page boilerplate when true. */
    onlyMainContent?: boolean;
    /** CSS selectors to include. */
    includeTags?: string[];
    /** CSS selectors to exclude. */
    excludeTags?: string[];
    /** Cache age in milliseconds; 0 requests fresh content. Omit for provider default. */
    maxAge?: number;
    /** Whether the provider may cache the result. */
    storeInCache?: boolean;
    /** Provider timeout, milliseconds; transport has its own timeout. */
    timeout?: number;
}

/** Web discovery controls. Scraping is opt-in and requires separate authorization. */
export interface SearchOptions {
    /** Maximum web results, 1–100; default 10. There is no pagination parameter. */
    limit?: number;
    /** Hostnames only; mutually exclusive with excludeDomains. Not a security boundary. */
    includeDomains?: string[];
    /** Hostnames only; mutually exclusive with includeDomains. */
    excludeDomains?: string[];
    /** Provider time filter, e.g. qdr:w, sbd:1, or cdr:1,cd_min:MM/DD/YYYY,cd_max:MM/DD/YYYY. */
    tbs?: string;
    /** Provider geo-targeting location, e.g. San Francisco,California,United States. */
    location?: string;
    /** Two-letter country code, e.g. US or UK. */
    country?: string;
    /** Apply SafeSearch when true; omitted preserves provider behavior. */
    safe?: boolean;
    /** Provider timeout in milliseconds; the HTTP transport has its own timeout. */
    timeout?: number;
    /** Explicit native result scraping; adds latency/credits and needs search.scrape plus delegatedFetch. */
    scrapeOptions?: ScrapeOptions;
}

/** One web result, preserving discovery details even when extraction fails. */
export interface SearchResult {
    /** Search source URL; independent of any redirected content URL. */
    url: string;
    /** Search title, empty when absent. */
    title: string;
    /** Provider description/snippet, empty when absent. */
    description: string;
    /** Provider result position when available. */
    position: number | null;
    /** Available extraction and metadata, or null for bare discovery results. */
    content: Page | null;
    /** Per-result error outside metadata, when supplied by the provider. */
    error: string | null;
}

/** One bounded search response. No invented cursor or completeness guarantee. */
export interface SearchResponse {
    /** Provider search request/job ID when returned; not a crawl or batch job handle. */
    id: string | null;
    /** All returned web results in provider order; never locally truncated. */
    results: SearchResult[];
    /** Provider-reported credits, or null when absent. */
    creditsUsed: number | null;
    /** Provider warning when present. */
    warning: string | null;
    /** Additional provider warning details, preserved without assuming their shape. */
    warnings: unknown;
}

/** Bounded URL-discovery controls. */
export interface MapOptions {
    /** Explicit ceiling, default 100. Map has no pagination or completeness guarantee. */
    limit?: number;
    /** Order discovered URLs by relevance to this text. */
    search?: string;
    /** Sitemap discovery mode; omitted uses provider default. */
    sitemap?: SitemapMode;
    /** Default false (narrower than Firecrawl's default). */
    includeSubdomains?: boolean;
    /** Whether the provider should deduplicate query variants. */
    ignoreQueryParameters?: boolean;
    /** Bypass the provider sitemap cache. */
    ignoreCache?: boolean;
}

/** Required page ceiling and optional provider crawl scope. */
export interface CrawlOptions {
    /** Required explicit page ceiling, 1–10000. */
    limit: number;
    /** Link discovery depth; sitemap entries count as depth zero. */
    maxDiscoveryDepth?: number;
    /** Provider Rust-regex pathname allowlist; also applies to seed. */
    includePaths?: string[];
    /** Provider Rust-regex pathname exclusions. */
    excludePaths?: string[];
    /** Sitemap discovery mode; omitted uses provider default. */
    sitemap?: SitemapMode;
    /** Whether the provider should deduplicate query variants. */
    ignoreQueryParameters?: boolean;
    /** Allow sibling and parent paths; default false. */
    crawlEntireDomain?: boolean;
    /** Follow subdomain links; default false. */
    allowSubdomains?: boolean;
    /** Follow external links; default false. */
    allowExternalLinks?: boolean;
    /** Content controls applied to every crawled page. */
    scrapeOptions?: ScrapeOptions;
}

/** Typed common metadata plus the complete provider object in Page.metadata. */
export interface PageMetadata {
    /** Provider source URL when available; retain for attribution. */
    sourceURL?: string | null;
    /** Provider URL; for page metadata this can be the resolved URL. */
    url?: string | null;
    /** Page title when provided. */
    title?: string | null;
    /** Page description when provided. */
    description?: string | null;
    /** Provider-reported page language. */
    language?: string | null;
    /** Source HTTP status, including per-page failures. */
    statusCode?: number | null;
    /** Provider-reported error detail when available. */
    error?: string | null;
}

/** Page content with source attribution and complete metadata. */
export interface Page {
    /** Provider source URL when available; retain for attribution. */
    sourceURL: string | null;
    /** Provider URL; for page metadata this can be the resolved URL. */
    url: string | null;
    /** Page title when provided. */
    title: string | null;
    /** Page description when provided. */
    description: string | null;
    /** Provider-reported page language. */
    language: string | null;
    /** Source HTTP status, including per-page failures. */
    statusCode: number | null;
    /** Provider-reported error detail when available. */
    error: string | null;
    /** Provider warning, such as incomplete content. */
    warning: string | null;
    /** Complete returned markdown, or null when absent. */
    markdown: string | null;
    /** Complete returned HTML, or null when absent. */
    html: string | null;
    /** Optional structured extraction; absent by default. */
    json: unknown;
    /** All provider metadata, including fields outside the typed projection. */
    metadata: unknown;
}

/** One discovered source URL. */
export interface MapLink {
    /** Provider URL; for page metadata this can be the resolved URL. */
    url: string;
    /** Page title when provided. */
    title?: string;
    /** Page description when provided. */
    description?: string;
}
/** A bounded discovery response, without a pagination cursor. */
export interface MapResponse {
    /** Discovered URLs in provider order; never locally truncated. */
    links: MapLink[];
    /** Echo of the requested limit; reaching it may mean more URLs exist. */
    limit: number;
}
/** Asynchronous submission acknowledgement. */
export interface Job {
    /** Provider-issued identifier. */
    id: string;
    /** Provider rejects retained even though submissions request strict URL validation. */
    invalidURLs: string[];
}
/** One explicit status/results page. */
export interface JobPage {
    /** Preserve future states; normally scraping, completed, failed or cancelled. */
    status: string;
    /** Provider total counter; not proof that every discovered page succeeded. */
    total: number;
    /** Provider successful-page counter. */
    completed: number;
    /** Provider-reported credits used, or null. */
    creditsUsed: number | null;
    /** Provider result expiry timestamp, or null. */
    expiresAt: string | null;
    /** Provider start timestamp, or null. */
    createdAt: string | null;
    /** Provider terminal timestamp, or null. */
    completedAt: string | null;
    /** Provider duration in seconds, or null. */
    duration: number | null;
    /** Provider-reported error detail when available. */
    error: string | null;
    /** Validated same-job URL, or null. Fetch explicitly; never automatically followed. */
    next: string | null;
    /** All pages in this response, including available partial results. */
    data: Page[];
}
/** One provider-reported failed source. */
export interface PageFailure {
    /** Provider-issued identifier, or null when absent. */
    id: string | null;
    /** Provider failure timestamp, or null when absent. */
    timestamp: string | null;
    /** Provider URL; for page metadata this can be the resolved URL. */
    url: string;
    /** Provider-reported error detail when available. */
    error: string;
}
/** Failures and robots exclusions returned by the errors endpoint. */
export interface JobErrors {
    /** Provider-reported page failures; may not cover all failure classes. */
    errors: PageFailure[];
    /** URLs reported blocked by robots.txt. */
    robotsBlocked: string[];
}
/** Acknowledgement of a cancellation request. */
export interface Cancellation {
    /** Cancellation acknowledgement: cancelled. */
    status: string;
}
/** VFS download controls; no arbitrary HTTP options. */
export interface SaveOptions {
    /** Default false. */
    overwrite?: boolean;
    /** Explicit byte cap; default 20000000. Oversize downloads fail, never truncate. */
    maxBytes?: number;
}

/** API/validation error; runtime permission and transport errors pass through. */
export class FirecrawlError extends Error {
    code: string;
    status: number;
    retryAfter: string | null;
    constructor(code: string, message: string, status: number = 0, retryAfter: string | null = null) {
        super(message);
        this.name = "FirecrawlError";
        this.code = code;
        this.status = status;
        this.retryAfter = retryAfter;
    }
}

/** Search web sources. Native result scraping authorizes unknown hosts via an explicit separate grant.
 * @param query Web search query, at most 500 characters.
 * @param options Optional search controls (result limit, domain filters, geo/time filters); omit it to use the provider defaults, with up to 10 web results.
 * @returns Web results in provider order, each with title, description and URL; `content` is set only when `scrapeOptions` was requested. `results` is empty when nothing matched.
 * @capability firecrawl.dev/search { limit: number }
 * @capability firecrawl.dev/search.scrape {}
 * @capability firecrawl.dev/delegatedFetch {}
 */
export function search(query: string, options: SearchOptions = {}): SearchResponse {
    const { limit, includeDomains, excludeDomains, tbs, location, country, safe, timeout, scrapeOptions } = options;
    // Snapshot nested inputs before schema serialization can invoke caller code.
    const opts: SearchOptions = {};
    if (limit !== undefined) opts.limit = limit;
    if (includeDomains !== undefined) {
        const copied: string[] = [];
        for (const domain of includeDomains) copied.push(domain);
        opts.includeDomains = copied;
    }
    if (excludeDomains !== undefined) {
        const copied: string[] = [];
        for (const domain of excludeDomains) copied.push(domain);
        opts.excludeDomains = copied;
    }
    if (tbs !== undefined) opts.tbs = tbs;
    if (location !== undefined) opts.location = location;
    if (country !== undefined) opts.country = country;
    if (safe !== undefined) opts.safe = safe;
    if (timeout !== undefined) opts.timeout = timeout;
    if (scrapeOptions !== undefined) {
        const { formats, json, onlyMainContent, includeTags, excludeTags, maxAge, storeInCache, timeout: scrapeTimeout } = scrapeOptions;
        const snapshot: ScrapeOptions = {};
        if (formats !== undefined) {
            const copied: Format[] = [];
            for (const format of formats) copied.push(format);
            snapshot.formats = copied;
        }
        if (includeTags !== undefined) {
            const copied: string[] = [];
            for (const tag of includeTags) copied.push(tag);
            snapshot.includeTags = copied;
        }
        if (excludeTags !== undefined) {
            const copied: string[] = [];
            for (const tag of excludeTags) copied.push(tag);
            snapshot.excludeTags = copied;
        }
        if (onlyMainContent !== undefined) snapshot.onlyMainContent = onlyMainContent;
        if (maxAge !== undefined) snapshot.maxAge = maxAge;
        if (storeInCache !== undefined) snapshot.storeInCache = storeInCache;
        if (scrapeTimeout !== undefined) snapshot.timeout = scrapeTimeout;
        if (json !== undefined) {
            const { schema, prompt } = json;
            const copied: JsonOptions = {};
            if (prompt !== undefined) copied.prompt = prompt;
            if (schema !== null && schema !== undefined) {
                copied.schema = snapshotSchema(schema);
            }
            snapshot.json = copied;
        }
        opts.scrapeOptions = snapshot;
    }
    const body = buildSearchBody(query, opts);
    check("firecrawl.dev/search", { limit: limit ?? 10 });
    if (scrapeOptions !== undefined) {
        // Search results are unknown before submission. This grant deliberately
        // authorizes native extraction across result hosts; it is not a host-scoped scrape grant.
        check("firecrawl.dev/search.scrape", {});
        check("firecrawl.dev/delegatedFetch", {});
    }
    return normalizeSearchJson(requireOk(post(BASE + "/search", body, authHeaders())));
}

function snapshotSchema(schema: unknown): unknown {
    const encoded = JSON.stringify(schema);
    if (encoded === undefined || !encoded.startsWith("{")) throw invalidArgument("json.schema must be an object");
    return JSON.parse(encoded);
}

/**
 * Build a web-only search request. No extraction or generated highlights by default.
 * @param query Web search query, at most 500 characters.
 * @param options Optional search controls; omit it to use the defaults, with a limit of 10.
 * @returns The JSON request body for the search endpoint.
 */
export function buildSearchBody(query: string, options: SearchOptions = {}): string {
    requireText(query, "query");
    if (query.length > 500) throw invalidArgument("query must be at most 500 characters");
    const { includeDomains, excludeDomains, tbs, location, country, safe, timeout, scrapeOptions } = options;
    const limit = options.limit ?? 10;
    integerRange(limit, 1, 100, "limit");
    const fields = [field("query", JSON.stringify(query)), field("limit", JSON.stringify(limit)),
        '"sources":["web"]', '"highlights":false', '"domainTools":false'];
    if (includeDomains !== undefined && excludeDomains !== undefined) throw invalidArgument("includeDomains and excludeDomains are mutually exclusive");
    addSearchDomains(fields, "includeDomains", includeDomains);
    addSearchDomains(fields, "excludeDomains", excludeDomains);
    if (tbs !== undefined) { requireText(tbs, "tbs"); fields.push(field("tbs", JSON.stringify(tbs))); }
    if (location !== undefined) { requireText(location, "location"); fields.push(field("location", JSON.stringify(location))); }
    if (country !== undefined) {
        if (!/^[a-zA-Z]{2}$/.test(country)) throw invalidArgument("country must be a two-letter code");
        fields.push(field("country", JSON.stringify(country.toUpperCase())));
    }
    addBoolean(fields, "safe", safe);
    if (timeout !== undefined) {
        integerRange(timeout, 1, 300000, "timeout");
        fields.push(field("timeout", JSON.stringify(timeout)));
    }
    if (scrapeOptions !== undefined) fields.push(field("scrapeOptions", objectJson(scrapeFields(scrapeOptions))));
    return objectJson(fields);
}

/** Retrieve one page. Host constrains the submitted URL, not provider redirects/subresources.
 * @param url Absolute HTTP(S) URL of the page, without credentials or whitespace.
 * @param options Optional content controls (formats, selectors, caching); omit it for markdown only.
 * @returns The page's markdown and/or HTML, title, source URL, status code and full provider metadata; fields the provider did not return are `null`.
 * @capability firecrawl.dev/scrape { host: string }
 * @capability firecrawl.dev/delegatedFetch {}
 */
export function scrape(url: string, options: ScrapeOptions = {}): Page {
    const body = buildScrapeBody(url, options);
    check("firecrawl.dev/scrape", { host: urlHost(url) });
    check("firecrawl.dev/delegatedFetch", {});
    return normalizeScrapeJson(requireOk(post(BASE + "/scrape", body, authHeaders())));
}

/** Discover URLs; the seed host is not a downstream network allowlist.
 * @param url Absolute HTTP(S) URL of the site or page to discover links from.
 * @param options Optional discovery controls (limit, search text, sitemap mode); omit it to use the defaults, with a limit of 100 and no subdomains.
 * @returns The discovered links in provider order and the limit that was applied; a result as long as `limit` may mean more URLs exist.
 * @capability firecrawl.dev/map { host: string, limit: number, includeSubdomains: boolean }
 * @capability firecrawl.dev/delegatedFetch {}
 */
export function map(url: string, options: MapOptions = {}): MapResponse {
    const { limit: requestedLimit, search, sitemap, includeSubdomains, ignoreQueryParameters, ignoreCache } = options;
    // The body builder reads its options again, so it gets these reads, not the caller's object.
    const body = buildMapBody(url, { limit: requestedLimit, search, sitemap, includeSubdomains, ignoreQueryParameters, ignoreCache });
    const limit = requestedLimit ?? 100;
    check("firecrawl.dev/map", { host: urlHost(url), limit: limit, includeSubdomains: includeSubdomains ?? false });
    check("firecrawl.dev/delegatedFetch", {});
    return normalizeMapJson(requireOk(post(BASE + "/map", body, authHeaders())), limit);
}

/** Submit once and return promptly. Every requested host is checked before any HTTP request.
 * @param urls Absolute HTTP(S) URLs to scrape, 1-1000; each host is checked before any request.
 * @param options Optional content controls applied to every URL; omit it for markdown only.
 * @returns The job with its ID, to pass to `getJob` with kind `"batch"`, and any URLs the provider rejected.
 * @capability firecrawl.dev/batch.start { host: string, count: number }
 * @capability firecrawl.dev/delegatedFetch {}
 */
export function startBatch(urls: string[], options: ScrapeOptions = {}): Job {
    const ownedUrls: string[] = [];
    for (const url of urls) ownedUrls.push(url);
    // Validated before the checks, as `buildBatchBody` does; the body is built after them.
    validateBatchUrls(ownedUrls);
    const fields = scrapeFields(options);
    for (const url of ownedUrls) check("firecrawl.dev/batch.start", { host: urlHost(url), count: ownedUrls.length });
    check("firecrawl.dev/delegatedFetch", {});
    const body = batchBody(ownedUrls, fields);
    return normalizeJobJson(requireOk(post(BASE + "/batch/scrape", body, authHeaders())));
}

/** Submit a bounded crawl. Scope flags are provider instructions, not a network sandbox.
 * @param url Absolute HTTP(S) URL where the crawl starts.
 * @param options Crawl scope controls; `limit` is the required page ceiling, 1-10000.
 * @returns The job with its ID, to pass to `getJob` with kind `"crawl"`, and any URLs the provider rejected.
 * @capability firecrawl.dev/crawl.start { host: string, limit: number, allowSubdomains: boolean, allowExternalLinks: boolean, crawlEntireDomain: boolean }
 * @capability firecrawl.dev/delegatedFetch {}
 */
export function startCrawl(url: string, options: CrawlOptions): Job {
    const { limit, maxDiscoveryDepth, includePaths, excludePaths, sitemap, ignoreQueryParameters,
        crawlEntireDomain, allowSubdomains, allowExternalLinks, scrapeOptions } = options;
    // The body builder reads its options again, so it gets these reads, not the caller's object.
    const body = buildCrawlBody(url, { limit, maxDiscoveryDepth, includePaths, excludePaths, sitemap,
        ignoreQueryParameters, crawlEntireDomain, allowSubdomains, allowExternalLinks, scrapeOptions });
    check("firecrawl.dev/crawl.start", { host: urlHost(url), limit: limit,
        allowSubdomains: allowSubdomains ?? false, allowExternalLinks: allowExternalLinks ?? false,
        crawlEntireDomain: crawlEntireDomain ?? false });
    check("firecrawl.dev/delegatedFetch", {});
    return normalizeJobJson(requireOk(post(BASE + "/crawl", body, authHeaders())));
}

/** Read status and exactly one results page, including partial results. No polling or pagination loop.
 * @param kind Job family: `"batch"` or `"crawl"`.
 * @param jobId UUID returned by `startBatch` or `startCrawl`.
 * @param next The `next` URL from a previous page of the same job to fetch the following page; `null` fetches the first page.
 * @returns The job status, counters, timestamps and one page of results; `next` is `null` on the last page.
 * @capability firecrawl.dev/jobs.read { kind: string, jobId: string }
 */
export function getJob(kind: JobKind, jobId: string, next: string | null = null): JobPage {
    const path = jobPagePath(kind, jobId, next);
    check("firecrawl.dev/jobs.read", { kind: kind, jobId: jobId });
    return normalizeJobPageJson(requireOk(get(BASE + path, authHeaders())), kind, jobId);
}

/** Retrieve failures omitted from results. Provider error accounting is not guaranteed complete.
 * @param kind Job family: `"batch"` or `"crawl"`.
 * @param jobId UUID returned by `startBatch` or `startCrawl`.
 * @returns The provider-reported page failures and the URLs blocked by robots.txt; both are empty when none were reported.
 * @capability firecrawl.dev/jobs.read { kind: string, jobId: string }
 */
export function getJobErrors(kind: JobKind, jobId: string): JobErrors {
    const path = jobPagePath(kind, jobId) + "/errors";
    check("firecrawl.dev/jobs.read", { kind: kind, jobId: jobId });
    return normalizeJobErrorsJson(requireOk(get(BASE + path, authHeaders())));
}

/** Cancel one existing job; reading permission does not grant cancellation.
 * @param kind Job family: `"batch"` or `"crawl"`.
 * @param jobId UUID returned by `startBatch` or `startCrawl`.
 * @returns The cancellation acknowledgement, whose `status` is `"cancelled"`.
 * @capability firecrawl.dev/jobs.cancel { kind: string, jobId: string }
 */
export function cancelJob(kind: JobKind, jobId: string): Cancellation {
    const path = jobPagePath(kind, jobId);
    check("firecrawl.dev/jobs.cancel", { kind: kind, jobId: jobId });
    return normalizeCancellationJson(requireOk(del(BASE + path, authHeaders())));
}

/** Save one raw status/results envelope to VFS. Inspect status and JSON next explicitly.
 * @param kind Job family: `"batch"` or `"crawl"`.
 * @param jobId UUID returned by `startBatch` or `startCrawl`.
 * @param path VFS path the raw JSON response is written to.
 * @param next The `next` URL from a previous page of the same job to fetch the following page; `null` fetches the first page.
 * @param options Optional `overwrite` (default false) and `maxBytes` (default 20000000); omit it to use the defaults.
 * @returns The download result for the file written to `path`.
 * @capability firecrawl.dev/jobs.read { kind: string, jobId: string }
 * @capability fs.write { path: string, max_bytes: number }
 */
export function downloadJobPage(kind: JobKind, jobId: string, path: string, next: string | null = null,
    options: SaveOptions = {}): DownloadResult {
    const endpoint = jobPagePath(kind, jobId, next);
    const { maxBytes: requestedMaxBytes, overwrite } = options;
    const maxBytes = requestedMaxBytes ?? 20000000;
    integerRange(maxBytes, 1, 9007199254740991, "maxBytes");
    check("firecrawl.dev/jobs.read", { kind: kind, jobId: jobId });
    check("fs.write", { path: path, max_bytes: maxBytes });
    return download(BASE + endpoint, path, { headers: authHeaders(), maxBytes: maxBytes, overwrite: overwrite ?? false });
}

/**
 * Pure builder used by offline tests.
 * @param url Absolute HTTP(S) URL of the page.
 * @param options Optional content controls (formats, selectors, caching); omit it for markdown only.
 * @returns The JSON request body for the scrape endpoint.
 */
export function buildScrapeBody(url: string, options: ScrapeOptions = {}): string {
    urlHost(url);
    const fields = scrapeFields(options);
    fields.push(field("url", JSON.stringify(url)));
    return objectJson(fields);
}

/**
 * Build a strict batch request without network access.
 * @param urls Absolute HTTP(S) URLs to scrape, 1-1000.
 * @param options Optional content controls (formats, selectors, caching); omit it for markdown only.
 * @returns The JSON request body for the batch-scrape endpoint.
 */
export function buildBatchBody(urls: string[], options: ScrapeOptions = {}): string {
    validateBatchUrls(urls);
    return batchBody(urls, scrapeFields(options));
}

function validateBatchUrls(urls: string[]): void {
    integerRange(urls.length, 1, 1000, "URL count");
    for (const url of urls) urlHost(url);
}

function batchBody(urls: string[], fields: string[]): string {
    fields.push(field("urls", JSON.stringify(urls)));
    fields.push('"ignoreInvalidURLs":false');
    return objectJson(fields);
}

/**
 * Build a bounded map request without network access.
 * @param url Absolute HTTP(S) URL of the site or page to map.
 * @param options Optional discovery controls; omit it to use the defaults, with a limit of 100.
 * @returns The JSON request body for the map endpoint.
 */
export function buildMapBody(url: string, options: MapOptions = {}): string {
    urlHost(url);
    const { search, sitemap, ignoreQueryParameters, ignoreCache } = options;
    const limit = options.limit ?? 100;
    integerRange(limit, 1, 100000, "limit");
    const fields = [field("url", JSON.stringify(url)), field("limit", JSON.stringify(limit)),
        field("includeSubdomains", JSON.stringify(options.includeSubdomains ?? false))];
    if (search !== undefined) { requireText(search, "search"); fields.push(field("search", JSON.stringify(search))); }
    addSitemap(fields, sitemap);
    addBoolean(fields, "ignoreQueryParameters", ignoreQueryParameters);
    addBoolean(fields, "ignoreCache", ignoreCache);
    return objectJson(fields);
}

/**
 * Build a bounded crawl request without network access.
 * @param url Absolute HTTP(S) URL where the crawl starts.
 * @param options Crawl scope controls; `limit` is the required page ceiling, 1-10000.
 * @returns The JSON request body for the crawl endpoint.
 */
export function buildCrawlBody(url: string, options: CrawlOptions): string {
    urlHost(url);
    integerRange(options.limit, 1, 10000, "limit");
    const fields = [field("url", JSON.stringify(url)), field("limit", JSON.stringify(options.limit)),
        field("allowSubdomains", JSON.stringify(options.allowSubdomains ?? false)),
        field("allowExternalLinks", JSON.stringify(options.allowExternalLinks ?? false)),
        field("crawlEntireDomain", JSON.stringify(options.crawlEntireDomain ?? false)),
        field("scrapeOptions", objectJson(scrapeFields(options.scrapeOptions)))];
    if (options.maxDiscoveryDepth !== undefined) {
        integerRange(options.maxDiscoveryDepth, 0, 1000, "maxDiscoveryDepth");
        fields.push(field("maxDiscoveryDepth", JSON.stringify(options.maxDiscoveryDepth)));
    }
    addStrings(fields, "includePaths", options.includePaths);
    addStrings(fields, "excludePaths", options.excludePaths);
    addSitemap(fields, options.sitemap);
    addBoolean(fields, "ignoreQueryParameters", options.ignoreQueryParameters);
    return objectJson(fields);
}

/**
 * Strict same-origin, same-kind, same-job pagination. Rebuild the destination from trusted parts.
 * @param kind Job family: `"batch"` or `"crawl"`.
 * @param jobId Firecrawl job UUID.
 * @param next Pagination URL from a previous page of the same job, or `null` for the first page.
 * @returns The API path (relative to the base URL) for the job, with the `skip`/`limit` query when `next` is given.
 */
export function jobPagePath(kind: JobKind, jobId: string, next: string | null = null): string {
    if (kind !== "batch" && kind !== "crawl") throw invalidArgument("kind must be batch or crawl");
    if (!/^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$/.test(jobId)) throw invalidArgument("Expected a Firecrawl UUID job ID");
    const path = (kind === "batch" ? "/batch/scrape/" : "/crawl/") + jobId;
    if (next === null) return path;
    // Exact prefix avoids userinfo, ports, fragments, encoded paths, normalization and lookalike hosts.
    const prefix = BASE + path + "?";
    if (!next.startsWith(prefix)) throw invalidArgument("Pagination URL must target the same Firecrawl job");
    const query = next.slice(prefix.length);
    if (!/^skip=[0-9]+(&limit=[0-9]+)?$/.test(query)) throw invalidArgument("Unsupported pagination query; expected skip and optional limit");
    return path + "?" + query;
}

/**
 * Validate a submitted URL and return its canonical permission host.
 * @param url Absolute HTTP(S) URL, without credentials or whitespace.
 * @returns The URL's host, as used in capability checks.
 */
export function urlHost(url: string): string {
    try {
        if (url.length > 8192 || url.trim() !== url || /[\s\\]/.test(url)) throw invalidArgument("Invalid URL");
        if (!/^https?:\/\/[^/?#@]+([/?#]|$)/.test(url)) throw invalidArgument("Invalid URL");
        const parts = parse(url);
        if ((parts.protocol !== "https" && parts.protocol !== "http") || /^\.*$/.test(parts.host)) throw invalidArgument("Invalid URL");
        return parts.host;
    } catch (cause) { throw invalidArgument("Expected an absolute HTTP(S) URL without credentials or whitespace"); }
}

interface ApiSearchResult {
    url: string; title?: string | null; description?: string | null; position?: number | null;
    markdown?: string | null; html?: string | null; json?: unknown; metadata?: unknown; warning?: string | null; error?: string | null;
}
interface ApiSearchData { web: ApiSearchResult[]; }
interface ApiSearch { success: boolean; data: ApiSearchData; id?: string | null; creditsUsed?: number | null; warning?: string | null; warnings?: unknown; }

/**
 * Validate v2 data.web and retain available extraction, metadata, warnings and partial failures.
 * @param body Raw JSON body of the search response.
 * @returns The normalized search response.
 */
export function normalizeSearchJson(body: string): SearchResponse {
    try {
        const data = JSON.parse(body) as ApiSearch;
        if (!data.success) throw invalidResponse();
        const results: SearchResult[] = [];
        for (const item of data.data.web) {
            requireText(item.url, "result URL");
            results.push({ url: item.url, title: item.title ?? "", description: item.description ?? "",
                position: item.position ?? null, content: searchContent(item), error: item.error ?? null });
        }
        return { id: data.id ?? null, results: results, creditsUsed: data.creditsUsed ?? null, warning: data.warning ?? null, warnings: data.warnings ?? null };
    } catch (cause) { throw invalidResponse(); }
}

function searchContent(item: ApiSearchResult): Page | null {
    if ((item.markdown === null || item.markdown === undefined)
        && (item.html === null || item.html === undefined)
        && (item.json === null || item.json === undefined)
        && (item.metadata === null || item.metadata === undefined)
        && (item.warning === null || item.warning === undefined)) return null;
    return pageFrom({ markdown: item.markdown, html: item.html, json: item.json,
        metadata: item.metadata ?? {}, warning: item.warning });
}

function addSearchDomains(fields: string[], name: string, domains: string[] | undefined): void {
    if (domains === undefined) return;
    const normalized: string[] = [];
    for (const domain of domains) {
        if (domain.length > 253 || !/^([a-zA-Z0-9]([a-zA-Z0-9-]*[a-zA-Z0-9])?\.)*[a-zA-Z0-9]([a-zA-Z0-9-]*[a-zA-Z0-9])?$/.test(domain)) {
            throw invalidArgument(name + " entries must be hostnames without a protocol, path, port or wildcard");
        }
        for (const part of domain.split(".")) {
            if (part.length > 63) throw invalidArgument(name + " hostname labels must be at most 63 characters");
        }
        normalized.push(domain.toLowerCase());
    }
    fields.push(field(name, JSON.stringify(normalized)));
}

interface ApiPage { markdown?: string | null; html?: string | null; json?: unknown; metadata: unknown; warning?: string | null; }
interface ApiScrape { success: boolean; data: ApiPage; }
interface ApiMapLink { url: string; title?: string | null; description?: string | null; }
interface ApiMap { success: boolean; links: ApiMapLink[]; }
interface ApiJob { success: boolean; id: string; invalidURLs?: string[] | null; }
interface ApiJobPage {
    status: string; total: number; completed: number; creditsUsed?: number | null;
    expiresAt?: string | null; createdAt?: string | null; completedAt?: string | null; duration?: number | null;
    error?: string | null; next?: string | null; data: ApiPage[];
}
interface ApiPageFailure { id?: string | null; timestamp?: string | null; url: string; error: string; }
interface ApiJobErrors { errors: ApiPageFailure[]; robotsBlocked: string[]; }

/**
 * Validate and normalize a scrape envelope.
 * @param body Raw JSON body of the scrape response.
 * @returns The normalized page.
 */
export function normalizeScrapeJson(body: string): Page {
    try {
        const data = JSON.parse(body) as ApiScrape;
        if (!data.success) throw invalidResponse();
        return pageFrom(data.data);
    } catch (cause) { throw invalidResponse(); }
}
/**
 * Validate URL discovery without trimming results.
 * @param body Raw JSON body of the map response.
 * @param limit The limit that was requested, echoed in the result.
 * @returns The discovered links and the requested limit.
 */
export function normalizeMapJson(body: string, limit: number): MapResponse {
    try {
        const data = JSON.parse(body) as ApiMap;
        if (!data.success) throw invalidResponse();
        const links: MapLink[] = [];
        for (const item of data.links) {
            requireText(item.url, "url");
            const link: MapLink = { url: item.url };
            if (item.title !== null && item.title !== undefined) link.title = item.title;
            if (item.description !== null && item.description !== undefined) link.description = item.description;
            links.push(link);
        }
        return { links: links, limit: limit };
    } catch (cause) { throw invalidResponse(); }
}
/**
 * Validate a submission acknowledgement.
 * @param body Raw JSON body of the batch or crawl submission response.
 * @returns The job ID and any URLs the provider rejected.
 */
export function normalizeJobJson(body: string): Job {
    try {
        const data = JSON.parse(body) as ApiJob;
        if (!data.success) throw invalidResponse();
        jobPagePath("batch", data.id);
        const invalidURLs: string[] = (data.invalidURLs === null || data.invalidURLs === undefined) ? [] : data.invalidURLs;
        return { id: data.id, invalidURLs: invalidURLs };
    } catch (cause) { throw invalidResponse(); }
}
/**
 * Preserve partial results and validate any next-page destination.
 * @param body Raw JSON body of the job status response.
 * @param kind Job family the request was made for.
 * @param jobId ID of the job the request was made for; any `next` URL must belong to it.
 * @returns The job status with one page of results.
 */
export function normalizeJobPageJson(body: string, kind: JobKind, jobId: string): JobPage {
    try {
        const data = JSON.parse(body) as ApiJobPage;
        requireText(data.status, "status");
        integerRange(data.total, 0, 9007199254740991, "total");
        integerRange(data.completed, 0, 9007199254740991, "completed");
        const next = data.next ?? null;
        if (next !== null) jobPagePath(kind, jobId, next);
        const pages: Page[] = [];
        for (const page of data.data) pages.push(pageFrom(page));
        return { status: data.status, total: data.total, completed: data.completed,
            creditsUsed: data.creditsUsed ?? null, expiresAt: data.expiresAt ?? null, createdAt: data.createdAt ?? null,
            completedAt: data.completedAt ?? null, duration: data.duration ?? null, error: data.error ?? null, next: next, data: pages };
    } catch (cause) { throw invalidResponse(); }
}
/**
 * Validate provider page failures and robots exclusions.
 * @param body Raw JSON body of the job errors response.
 * @returns The page failures and robots.txt-blocked URLs.
 */
export function normalizeJobErrorsJson(body: string): JobErrors {
    try {
        const data = JSON.parse(body) as ApiJobErrors;
        const errors: PageFailure[] = [];
        for (const failure of data.errors) {
            requireText(failure.url, "url");
            requireText(failure.error, "error");
            errors.push({ id: failure.id ?? null, timestamp: failure.timestamp ?? null, url: failure.url, error: failure.error });
        }
        return { errors: errors, robotsBlocked: data.robotsBlocked };
    } catch (cause) { throw invalidResponse(); }
}
/**
 * Validate the current v2 cancellation acknowledgement.
 * @param body Raw JSON body of the cancellation response.
 * @returns The cancellation acknowledgement.
 */
export function normalizeCancellationJson(body: string): Cancellation {
    try {
        const data = JSON.parse(body) as Cancellation;
        if (data.status !== "cancelled") throw invalidResponse();
        return data;
    } catch (cause) { throw invalidResponse(); }
}

/**
 * HTTP errors never include response bodies or credentials. Retry-After is advisory only.
 * @param status HTTP status code of the failed response.
 * @param retryAfter The `retry-after` response header value, or `null` when absent.
 * @returns A `FirecrawlError` whose code is derived from the status.
 */
export function firecrawlHttpError(status: number, retryAfter: string | null = null): FirecrawlError {
    let code = "http_error";
    if (status === 400 || status === 422) code = "invalid_request";
    else if (status === 401) code = "unauthorized";
    else if (status === 402) code = "quota_exceeded";
    else if (status === 403) code = "forbidden";
    else if (status === 404) code = "not_found";
    else if (status === 429) code = "rate_limited";
    return new FirecrawlError(code, "Firecrawl request failed: HTTP " + status.toString() + " (" + code + ")", status, retryAfter);
}

function scrapeFields(options: ScrapeOptions = {}): string[] {
    const { json, onlyMainContent, storeInCache, includeTags, excludeTags, maxAge, timeout } = options;
    const formats = options.formats ?? ["markdown"];
    const encoded: string[] = [];
    for (const format of formats) {
        if (format !== "markdown" && format !== "html") throw invalidArgument("formats supports markdown and html");
        encoded.push(JSON.stringify(format));
    }
    if (json !== undefined) {
        const fields = ['"type":"json"'];
        if (json.schema !== null && json.schema !== undefined) {
            const schema = JSON.stringify(json.schema);
            if (schema === undefined || !schema.startsWith("{")) throw invalidArgument("json.schema must be an object");
            fields.push(field("schema", schema));
        }
        if (json.prompt !== undefined) { requireText(json.prompt, "json.prompt"); fields.push(field("prompt", JSON.stringify(json.prompt))); }
        encoded.push(objectJson(fields));
    }
    if (encoded.length === 0) throw invalidArgument("Request at least one output format");
    const fields = [field("formats", "[" + encoded.join(",") + "]")];
    addBoolean(fields, "onlyMainContent", onlyMainContent);
    addBoolean(fields, "storeInCache", storeInCache);
    addStrings(fields, "includeTags", includeTags);
    addStrings(fields, "excludeTags", excludeTags);
    if (maxAge !== undefined) { integerRange(maxAge, 0, 9007199254740991, "maxAge"); fields.push(field("maxAge", JSON.stringify(maxAge))); }
    if (timeout !== undefined) { integerRange(timeout, 1, 300000, "timeout"); fields.push(field("timeout", JSON.stringify(timeout))); }
    return fields;
}
function pageFrom(data: ApiPage): Page {
    const metadata = data.metadata as PageMetadata;
    return { sourceURL: metadata.sourceURL ?? null, url: metadata.url ?? null, title: metadata.title ?? null,
        description: metadata.description ?? null, language: metadata.language ?? null, statusCode: metadata.statusCode ?? null,
        error: metadata.error ?? null, warning: data.warning ?? null, markdown: data.markdown ?? null, html: data.html ?? null,
        json: data.json ?? null, metadata: data.metadata };
}
function authHeaders(): Map<string, string> {
    const key = secrets.get("FIRECRAWL_API_KEY");
    if (key === undefined || key.trim().length === 0) throw new FirecrawlError("missing_credentials", "Bind FIRECRAWL_API_KEY before using Firecrawl");
    const headers = new Map<string, string>();
    headers.set("Authorization", "Bearer " + key);
    headers.set("Content-Type", "application/json");
    headers.set("Accept", "application/json");
    return headers;
}
function requireOk(response: Response): string {
    if (!response.ok) throw firecrawlHttpError(response.status, response.headers.get("retry-after") ?? null);
    return response.body;
}
function addSitemap(fields: string[], mode: SitemapMode | undefined): void {
    if (mode === undefined) return;
    if (mode !== "include" && mode !== "skip" && mode !== "only") throw invalidArgument("Invalid sitemap mode");
    fields.push(field("sitemap", JSON.stringify(mode)));
}
function addStrings(fields: string[], name: string, values: string[] | undefined): void {
    if (values === undefined) return;
    integerRange(values.length, 0, 1000, name + " count");
    for (const value of values) requireText(value, name);
    fields.push(field(name, JSON.stringify(values)));
}
function addBoolean(fields: string[], name: string, value: boolean | undefined): void {
    if (value !== undefined) fields.push(field(name, JSON.stringify(value)));
}
function integerRange(value: number, min: number, max: number, name: string): void {
    if (!Number.isFinite(value) || value !== Math.floor(value) || value < min || value > max) throw invalidArgument(name + " is outside its integer range");
}
function requireText(value: string, name: string): void {
    if (value.trim().length === 0) throw invalidArgument(name + " cannot be empty");
}
function field(name: string, json: string): string { return JSON.stringify(name) + ":" + json; }
function objectJson(fields: string[]): string { return "{" + fields.join(",") + "}"; }
function invalidArgument(message: string): FirecrawlError { return new FirecrawlError("invalid_argument", message); }
function invalidResponse(): FirecrawlError { return new FirecrawlError("invalid_response", "Firecrawl returned an invalid response"); }
