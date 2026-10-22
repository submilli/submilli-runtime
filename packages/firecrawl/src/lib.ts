import { get, post, delete as del, download, Response, DownloadResult } from "submilli:http";
import secrets from "submilli:secrets";
import { check } from "submilli:security";
import {
    BASE,
    batchBody,
    buildCrawlBody,
    buildMapBody,
    buildScrapeBody,
    buildSearchBody,
    Cancellation,
    CrawlOptions,
    FirecrawlError,
    firecrawlHttpError,
    Format,
    integerRange,
    invalidArgument,
    Job,
    JobErrors,
    JobKind,
    JobPage,
    jobPagePath,
    JsonOptions,
    MapOptions,
    MapResponse,
    normalizeCancellationJson,
    normalizeJobErrorsJson,
    normalizeJobJson,
    normalizeJobPageJson,
    normalizeMapJson,
    normalizeScrapeJson,
    normalizeSearchJson,
    Page,
    SaveOptions,
    scrapeFields,
    ScrapeOptions,
    SearchOptions,
    SearchResponse,
    urlHost,
    validateBatchUrls,
} from "./internal";

export {
    JobKind,
    SitemapMode,
    Format,
    JsonOptions,
    ScrapeOptions,
    SearchOptions,
    SearchResult,
    SearchResponse,
    MapOptions,
    CrawlOptions,
    PageMetadata,
    Page,
    MapLink,
    MapResponse,
    Job,
    JobPage,
    PageFailure,
    JobErrors,
    Cancellation,
    SaveOptions,
    FirecrawlError,
} from "./internal";

/** Search web sources. With `scrapeOptions`, the provider also fetches each result page, whatever its host.
 * @param query Web search query, at most 500 characters.
 * @param options Optional search controls (result limit, domain filters, geo/time filters); omit it to use the provider defaults, with up to 10 web results.
 * @returns Web results in provider order, each with title, description and URL; `content` is set only when `scrapeOptions` was requested. `results` is empty when nothing matched.
 * @capability firecrawl.dev/search { limit: number }
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
    // Search results are unknown before submission, so a search grant also
    // authorizes native extraction across result hosts when scrapeOptions asks for it.
    check("firecrawl.dev/search", { limit: limit ?? 10 });
    return normalizeSearchJson(requireOk(post(BASE + "/search", body, authHeaders())));
}

function snapshotSchema(schema: unknown): unknown {
    const encoded = JSON.stringify(schema);
    if (encoded === undefined || !encoded.startsWith("{")) throw invalidArgument("json.schema must be an object");
    return JSON.parse(encoded);
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
