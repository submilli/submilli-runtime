import {
    FetchedResource,
    NotionBlock,
    NotionDatabase,
    NotionDataSource,
    NotionPage,
    NotionUser,
    NotionView,
    PageResult,
    ResourceKind,
    SearchOptions,
    SearchResult,
} from "./types";
import {
    blockFrom,
    dataSourceFrom,
    databaseFrom,
    fieldJson,
    idFromRef,
    listFrom,
    notionGet,
    notionPost,
    objectJson,
    pageFrom,
    pageSize,
    pathId,
    putQuery,
    userFrom,
    viewFrom,
} from "./transport";

export {
    DataSourceReference,
    FetchedResource,
    NotionBlock,
    NotionDatabase,
    NotionDataSource,
    NotionPage,
    NotionParent,
    NotionUser,
    NotionView,
    PageOptions,
    PageResult,
    ResourceKind,
    SearchOptions,
    SearchResult,
} from "./types";

interface ApiSearchItem {
    object: string;
    id: string;
    url?: string | null;
    last_edited_time?: string | null;
    properties?: unknown;
    title?: unknown[] | null;
}

/**
 * Extract and validate a Notion ID from an ID, URL, or collection:// reference.
 *
 * @param ref Notion ID, Notion URL, or `collection://` data source reference.
 * @returns The validated Notion ID.
 */
export function notionId(ref: string): string {
    return idFromRef(ref);
}

/**
 * Retrieve the integration bot user.
 *
 * @returns The bot user the access token belongs to.
 */
export function getSelf(): NotionUser {
    return userFrom(notionGet("/users/me").json());
}

/**
 * Search titles visible to the connection.
 *
 * @param options Search text, filter, sort, and pagination fields; omitted fields use the Notion defaults.
 * @returns One page of matching pages and data sources; an empty `results` means nothing matched.
 */
export function search(options: SearchOptions): PageResult<SearchResult> {
    const { query, kind, direction, timestamp, pageSize: requestedSize, startCursor } = options;
    const fields: string[] = [];
    if (query !== undefined && query.length > 0) fields.push(fieldJson("query", query));
    if (kind !== undefined) {
        fields.push("\"filter\":{\"property\":\"object\",\"value\":" + JSON.stringify(kind) + "}");
    }
    if (direction !== undefined) {
        const sortTimestamp = timestamp ?? "last_edited_time";
        fields.push("\"sort\":{\"direction\":" + JSON.stringify(direction) + ",\"timestamp\":" + JSON.stringify(sortTimestamp) + "}");
    }
    fields.push(fieldJson("page_size", pageSize(requestedSize)));
    if (startCursor !== undefined) fields.push(fieldJson("start_cursor", startCursor));
    const page = listFrom(notionPost("/search", objectJson(fields)));
    const results: SearchResult[] = [];
    for (const raw of page.results) results.push(searchResultFrom(raw));
    return {
        results: results,
        hasMore: page.hasMore,
        nextCursor: page.nextCursor,
        isComplete: page.isComplete,
    };
}

/**
 * Fetch one resource of an explicit kind by its resolved ID.
 *
 * @param kind Kind of resource to fetch, which selects the API endpoint.
 * @param id Resolved Notion ID or reference of the resource.
 * @returns The resource converted to the type matching `kind`.
 */
export function fetch(kind: ResourceKind, id: string): FetchedResource {
    if (kind === "page") return pageFrom(notionGet("/pages/" + pathId(id)).json());
    if (kind === "database") return databaseFrom(notionGet("/databases/" + pathId(id)).json());
    if (kind === "data_source") return dataSourceFrom(notionGet("/data_sources/" + pathId(id)).json());
    if (kind === "block") return blockFrom(notionGet("/blocks/" + pathId(id)).json());
    if (kind === "view") return viewFrom(notionGet("/views/" + pathId(id)).json());
    return userFrom(notionGet("/users/" + pathId(id)).json());
}

/**
 * Retrieve a page by its resolved ID.
 *
 * @param id Resolved Notion ID or reference of the page.
 * @returns The page.
 */
export function fetchPage(id: string): NotionPage {
    return pageFrom(notionGet("/pages/" + pathId(id)).json());
}

/**
 * Retrieve a database container by its resolved ID.
 *
 * @param id Resolved Notion ID or reference of the database.
 * @returns The database with its data source references.
 */
export function fetchDatabase(id: string): NotionDatabase {
    return databaseFrom(notionGet("/databases/" + pathId(id)).json());
}

/**
 * Retrieve a data source by its resolved ID.
 *
 * @param id Resolved Notion ID or reference of the data source.
 * @returns The data source with its property schema.
 */
export function fetchDataSource(id: string): NotionDataSource {
    return dataSourceFrom(notionGet("/data_sources/" + pathId(id)).json());
}

/**
 * Retrieve a workspace user by its resolved ID.
 *
 * @param id Resolved Notion ID or reference of the user.
 * @returns The user.
 */
export function getUser(id: string): NotionUser {
    return userFrom(notionGet("/users/" + pathId(id)).json());
}

/**
 * List users visible to the connection.
 *
 * @param requestedSize Users per page, 1 to 100; undefined uses 100.
 * @param startCursor Cursor from a previous page's `nextCursor`; undefined starts at the first user.
 * @returns One page of users with pagination state.
 */
export function listUsers(requestedSize: number | undefined, startCursor: string | undefined): PageResult<NotionUser> {
    const query = new Map<string, string>();
    putQuery(query, "start_cursor", startCursor);
    query.set("page_size", pageSize(requestedSize).toString());
    const page = listFrom(notionGet("/users", query));
    const results: NotionUser[] = [];
    for (const raw of page.results) results.push(userFrom(raw));
    return {
        results: results,
        hasMore: page.hasMore,
        nextCursor: page.nextCursor,
        isComplete: page.isComplete,
    };
}

function searchResultFrom(raw: unknown): SearchResult {
    const item = raw as ApiSearchItem;
    return {
        kind: item.object === "data_source" ? "data_source" : "page",
        id: item.id,
        url: item.url ?? "",
        title: titleFrom(item),
        lastEditedTime: item.last_edited_time ?? "",
        raw: raw,
    };
}

function titleFrom(item: ApiSearchItem): string {
    if (item.title) return richText(item.title);
    if (item.properties === null || item.properties === undefined) return "";
    for (const entry of Object.entries(item.properties)) {
        const value = entry[1];
        if ("type" in value) {
            if (value.type === "title" && "title" in value) return richText(value.title as unknown[]);
        }
    }
    return "";
}

function richText(items: unknown[]): string {
    const parts: string[] = [];
    for (const item of items) {
        if ("plain_text" in item && typeof item.plain_text === "string") parts.push(item.plain_text);
    }
    return parts.join("");
}
