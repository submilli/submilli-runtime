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
    /** Creation time, an ISO-8601 instant. */
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
    /** Number of pages; request `page + 1` while it is below this. */
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
    endpoint: string;
    /** Encoded query string, starting with `?`. */
    query: string;
}

/**
 * Validate search options and encode them.
 * @param query Search text.
 * @param sort date or relevance.
 * @param exact Exact word matching.
 * @param type story or comment.
 * @param storyId Story id filter.
 * @param author Author filter.
 * @param createdAfter ISO-8601 lower bound.
 * @param createdBefore ISO-8601 upper bound.
 * @param page Page number.
 * @param hitsPerPage Page size.
 * @returns The endpoint and query string.
 */
export function searchRequest(query: string, sort: string | undefined, exact: boolean | undefined, type: string | undefined,
    storyId: number | undefined, author: string | undefined, createdAfter: string | undefined, createdBefore: string | undefined,
    page: number | undefined, hitsPerPage: number | undefined): SearchRequest {
    if (typeof query !== "string" || query.trim().length === 0 || query.length > 300) {
        throw invalidInput("query must be 1–300 characters");
    }
    const order = sort === undefined ? "date" : sort;
    if (order !== "date" && order !== "relevance") throw invalidInput("sort must be date or relevance");
    const tags: string[] = [];
    if (type !== undefined) {
        if (type !== "story" && type !== "comment") throw invalidInput("type must be story or comment");
        tags.push(type);
    }
    if (storyId !== undefined) tags.push("story_" + requireId(storyId, "storyId").toString());
    if (author !== undefined) {
        if (!/^[A-Za-z0-9_-]{1,32}$/.test(author)) throw invalidInput("author must be an HN username");
        tags.push("author_" + author);
    }
    const numeric: string[] = [];
    if (createdAfter !== undefined) numeric.push("created_at_i>=" + epochSeconds(createdAfter, "createdAfter", false).toString());
    if (createdBefore !== undefined) numeric.push("created_at_i<" + epochSeconds(createdBefore, "createdBefore", true).toString());
    const pageNumber = page === undefined ? 0 : page;
    if (!Number.isInteger(pageNumber) || pageNumber < 0 || pageNumber > 1000) throw invalidInput("page must be an integer from 0 through 1000");
    const size = hitsPerPage === undefined ? 50 : hitsPerPage;
    if (!Number.isInteger(size) || size < 1 || size > 100) throw invalidInput("hitsPerPage must be an integer from 1 through 100");
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
    if (!Number.isInteger(value) || value < 1 || value > 1000000000000) throw invalidInput(name + " must be a positive integer item id");
    return value;
}

function epochSeconds(value: string, name: string, roundUp: boolean): number {
    let ms = 0;
    try {
        ms = Temporal.Instant.from(value).epochMilliseconds;
    } catch (error) {
        throw invalidInput(name + " must be an ISO-8601 instant with a time zone, such as 2026-10-24T06:50:00Z");
    }
    return roundUp ? Math.ceil(ms / 1000) : Math.floor(ms / 1000);
}

function invalidInput(message: string): HackerNewsError {
    return new HackerNewsError("invalid_input", message, 0);
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
    const parsed = JSON.parse(body) as ApiSearch;
    const items: Item[] = [];
    for (const hit of parsed.hits ?? []) {
        const id = parseInt(hit.objectID ?? "", 10);
        if (!Number.isInteger(id)) throw new HackerNewsError("invalid_response", "search hit without a numeric objectID", 0);
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
}

/**
 * Flatten an item tree.
 * @param body Response body.
 * @returns The thread, root first.
 */
export function threadFrom(body: string): Thread {
    const root = JSON.parse(body) as ApiItem;
    const items: Item[] = [];
    const stack: unknown[] = [root];
    let storyTitle: string | null = root.type === "story" ? root.title ?? null : null;
    while (stack.length > 0) {
        const node = stack.pop() as ApiItem;
        const id = node.id;
        if (typeof id !== "number") throw new HackerNewsError("invalid_response", "item without a numeric id", 0);
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
}
