// Request validation and response mapping for the Hacker News client.
// Internal: `src/lib.ts` re-exports the types and calls these helpers.

/** A refused argument or an unexpected answer from the search API. */
export class HackerNewsError extends Error {
    /** `invalid_input`, `http_<status>`, or `invalid_response`. */
    code: string;
    /** HTTP status, or 0 for a local error. */
    status: number;

    constructor(code: string, message: string, status: number) {
        super(message);
        this.name = "HackerNewsError";
        this.code = code;
        this.status = status;
    }
}

/** Options for `search`. `query` is required. */
export interface SearchOptions {
    /** Text to find in titles, story text, comment text and URLs. */
    query: string;
    /** `date` (default, newest first) or `relevance`. */
    sort?: "date" | "relevance";
    /** Match the words exactly, without typo tolerance or prefix matching. Defaults to true. */
    exact?: boolean;
    /** Restrict to `story` or `comment` items. Omit for both. */
    type?: "story" | "comment";
    /** Only items under this story id. */
    storyId?: number;
    /** Only items by this author (case-sensitive HN username). */
    author?: string;
    /** Only items created at or after this ISO-8601 instant. */
    createdAfter?: string;
    /** Only items created before this ISO-8601 instant. */
    createdBefore?: string;
    /** Zero-based page number. Defaults to 0. */
    page?: number;
    /** Hits per page, 1–100. Defaults to 50. */
    hitsPerPage?: number;
}

/** One item: a story, comment, poll, or job. */
export interface Item {
    /** Item id. */
    id: number;
    /** `story`, `comment`, `poll`, `pollopt`, or `job`. */
    type: string;
    /** Author's HN username, or null when deleted. */
    author: string | null;
    /** Creation time, an ISO-8601 instant, or "" when HN did not report one. */
    createdAt: string;
    /** Story title, or null for comments. */
    title: string | null;
    /** The story's link, or null. */
    url: string | null;
    /** Comment or story text as HN's HTML, or null. */
    text: string | null;
    /** Story points, or null. */
    points: number | null;
    /** Parent item id, or null for a story. */
    parentId: number | null;
    /** The story this item belongs to, or null. */
    storyId: number | null;
    /** Title of the story this item belongs to, when the search API reports it. */
    storyTitle: string | null;
    /** The item's page on news.ycombinator.com. */
    link: string;
}

/** One page of search hits. */
export interface SearchPage {
    /** Matching items on this page. */
    items: Item[];
    /** Total matches. */
    total: number;
    /** This page's number. */
    page: number;
    /** Number of pages; request `page + 1` while it is below this. The API serves at most 1,000 hits per query, so narrow the query (for example by `createdAfter`) when `total` is larger. */
    pageCount: number;
    /** Whether `total` is exact. When false, the search may have stopped early and the result is not proof of absence. */
    exhaustive: boolean;
}

/** An item and every descendant, depth-first with each parent before its children. */
export interface Thread {
    /** The root item first, then its descendants. */
    items: Item[];
}

/** The search query string after validation, and the endpoint path it uses. */
export interface SearchRequest {
    /** `search_by_date` or `search`. */
    endpoint: "search_by_date" | "search";
    /** Encoded query string, starting with `?`. */
    query: string;
}

/**
 * Validate search options and encode them.
 * @param options The caller's search options.
 * @returns The endpoint and query string.
 */
export function searchRequest(options: SearchOptions): SearchRequest {
    const { query, sort, exact, type, storyId, author, createdAfter, createdBefore, page, hitsPerPage } = options;
    if (typeof query !== "string" || query.trim().length === 0 || query.length > 300) {
        throw invalidInput("query must be 1–300 characters, got " + show(query));
    }
    const order = sort === undefined ? "date" : sort;
    if (order !== "date" && order !== "relevance") throw invalidInput("sort must be date or relevance, got " + show(sort));
    const tags: string[] = [];
    if (type !== undefined) {
        if (type !== "story" && type !== "comment") throw invalidInput("type must be story or comment, got " + show(type));
        tags.push(type);
    }
    if (storyId !== undefined) tags.push("story_" + requireId(storyId, "storyId").toString());
    if (author !== undefined) {
        if (!/^[A-Za-z0-9_-]{1,32}$/.test(author)) throw invalidInput("author must be an HN username, got " + show(author));
        tags.push("author_" + author);
    }
    const numeric: string[] = [];
    const start = createdAfter === undefined ? null : epochSeconds(createdAfter, "createdAfter");
    const end = createdBefore === undefined ? null : epochSeconds(createdBefore, "createdBefore");
    if (createdAfter !== undefined && createdBefore !== undefined
        && Temporal.Instant.compare(Temporal.Instant.from(createdBefore), Temporal.Instant.from(createdAfter)) <= 0) {
        throw invalidInput("createdBefore must be later than createdAfter, got " + show(createdAfter) + " and " + show(createdBefore));
    }
    if (start !== null) numeric.push("created_at_i>=" + start.toString());
    // A window with no whole second in it stays valid and simply matches nothing.
    if (end !== null) numeric.push("created_at_i<" + end.toString());
    const pageNumber = page === undefined ? 0 : page;
    if (!Number.isInteger(pageNumber) || pageNumber < 0 || pageNumber > 1000) throw invalidInput("page must be an integer from 0 through 1000, got " + show(pageNumber));
    const size = hitsPerPage === undefined ? 50 : hitsPerPage;
    if (!Number.isInteger(size) || size < 1 || size > 100) throw invalidInput("hitsPerPage must be an integer from 1 through 100, got " + show(size));
    let encoded = "?query=" + encodeURIComponent(query);
    if (exact !== false) encoded += "&typoTolerance=false&queryType=prefixNone";
    if (tags.length > 0) encoded += "&tags=" + encodeURIComponent(tags.join(","));
    if (numeric.length > 0) encoded += "&numericFilters=" + encodeURIComponent(numeric.join(","));
    encoded += "&page=" + pageNumber.toString() + "&hitsPerPage=" + size.toString();
    return { endpoint: order === "date" ? "search_by_date" : "search", query: encoded };
}

/**
 * Validate an item id.
 * @param value The id.
 * @param name Argument name for the error message.
 * @returns The id.
 */
export function requireId(value: number, name: string): number {
    if (!Number.isInteger(value) || value < 1 || value > 1000000000000) throw invalidInput(name + " must be a positive integer item id up to 10^12, got " + show(value));
    return value;
}

// Z or a whole-minute offset (the runtime rounds fractional-second offsets),
// up to nine fractional digits, and an optional [Zone] annotation.
const INSTANT = /^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}(:[0-5][0-9]([.,][0-9]{1,9})?)?(Z|[+-][0-9]{2}:[0-9]{2})(\[[A-Za-z0-9_+\/-]{1,64}\])?$/;

// HN times are whole seconds, so both bounds round up: `>= ceil(after)` and
// `< ceil(before)` select exactly the items in [after, before).
function epochSeconds(value: string, name: string): number {
    const invalid = invalidInput(name + " must be an ISO-8601 instant with Z or a ±HH:MM offset (a plain [Zone] annotation is ignored), such as 2026-10-24T06:50:00Z, got " + show(value));
    if (typeof value !== "string" || !INSTANT.test(value)) throw invalid;
    let ms = 0;
    try {
        ms = Temporal.Instant.from(value).epochMilliseconds;
    } catch (error) {
        throw invalid;
    }
    // A fraction finer than milliseconds still counts as after the whole second.
    if (/[.,][0-9]{3}[0-9]*[1-9]/.test(value)) ms++;
    return Math.ceil(ms / 1000);
}

// Render a value for an error message.
function show(value: unknown): string {
    if (typeof value === "number" && !Number.isFinite(value)) return value.toString();
    const rendered = JSON.stringify(value);
    if (rendered === undefined) return "undefined";
    if (rendered.length <= 200) return rendered;
    // Do not end on the first half of a surrogate pair.
    const high = rendered.charCodeAt(199);
    return rendered.substring(0, high >= 0xd800 && high <= 0xdbff ? 199 : 200) + "…";
}

function invalidInput(message: string): HackerNewsError {
    return new HackerNewsError("invalid_input", message, 0);
}

function invalidResponse(message: string): HackerNewsError {
    return new HackerNewsError("invalid_response", message, 0);
}

// A body of the wrong shape fails the checked casts with a TypeError; report
// it with the package's own error code instead.
function shaped<T>(map: () => T): T {
    try {
        return map();
    } catch (error) {
        if (error instanceof HackerNewsError) throw error;
        throw invalidResponse("HN Search API returned an unexpected response: " + error.message);
    }
}

interface ApiHit {
    objectID?: string | null;
    created_at?: string | null;
    author?: string | null;
    title?: string | null;
    url?: string | null;
    story_text?: string | null;
    comment_text?: string | null;
    points?: number | null;
    parent_id?: number | null;
    story_id?: number | null;
    story_title?: string | null;
    _tags?: string[] | null;
}

interface ApiSearch {
    hits?: ApiHit[] | null;
    nbHits?: number | null;
    page?: number | null;
    nbPages?: number | null;
    exhaustiveNbHits?: boolean | null;
}

interface ApiItem {
    id?: number | null;
    type?: string | null;
    author?: string | null;
    created_at?: string | null;
    title?: string | null;
    url?: string | null;
    text?: string | null;
    points?: number | null;
    parent_id?: number | null;
    story_id?: number | null;
    children?: unknown[] | null;
}

function decode(body: string): unknown {
    try {
        return JSON.parse(body);
    } catch (error) {
        throw invalidResponse("HN Search API returned a body that is not JSON");
    }
}

function link(id: number): string {
    return "https://news.ycombinator.com/item?id=" + id.toString();
}

function hitType(tags: string[]): string {
    for (const tag of tags) {
        if (tag === "story" || tag === "comment" || tag === "poll" || tag === "pollopt" || tag === "job") return tag;
    }
    return "unknown";
}

/**
 * Map a search response.
 * @param body Response body.
 * @returns The page.
 */
export function searchPageFrom(body: string): SearchPage {
    return shaped((): SearchPage => {
        const parsed = decode(body) as ApiSearch;
        const items: Item[] = [];
        for (const hit of parsed.hits ?? []) {
            const id = parseInt(hit.objectID ?? "", 10);
            if (!Number.isInteger(id)) throw invalidResponse("search hit without a numeric objectID");
            const text = hit.comment_text ?? hit.story_text ?? null;
            items.push({
                id: id,
                type: hitType(hit._tags ?? []),
                author: hit.author ?? null,
                createdAt: hit.created_at ?? "",
                title: hit.title ?? null,
                url: hit.url ?? null,
                text: text,
                points: hit.points ?? null,
                parentId: hit.parent_id ?? null,
                storyId: hit.story_id ?? null,
                storyTitle: hit.story_title ?? hit.title ?? null,
                link: link(id),
            });
        }
        return {
            items: items,
            total: parsed.nbHits ?? items.length,
            page: parsed.page ?? 0,
            pageCount: parsed.nbPages ?? 0,
            exhaustive: parsed.exhaustiveNbHits === true,
        };
    });
}

/**
 * Flatten an item tree.
 * @param body Response body.
 * @returns The thread, root first.
 */
export function threadFrom(body: string): Thread {
    return shaped((): Thread => {
        const root = decode(body) as ApiItem;
        const items: Item[] = [];
        const stack: unknown[] = [root];
        const storyTitle: string | null = root.type === "story" ? root.title ?? null : null;
        while (stack.length > 0) {
            const node = stack.pop() as ApiItem;
            const id = node.id;
            if (typeof id !== "number") throw invalidResponse("item without a numeric id");
            items.push({
                id: id,
                type: node.type ?? "unknown",
                author: node.author ?? null,
                createdAt: node.created_at ?? "",
                title: node.title ?? null,
                url: node.url ?? null,
                text: node.text ?? null,
                points: node.points ?? null,
                parentId: node.parent_id ?? null,
                storyId: node.story_id ?? null,
                storyTitle: storyTitle,
                link: link(id),
            });
            const children = node.children ?? [];
            for (let index = children.length - 1; index >= 0; index--) stack.push(children[index]);
        }
        return { items: items };
    });
}
