import { check } from "submilli:security";
import {
    FetchedResource,
    NotionBlock,
    NotionDatabase,
    NotionDataSource,
    NotionPage,
    NotionUser,
    NotionView,
    PageOptions,
    PageResult,
    ResourceRef,
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
    ResourceRef,
    ResourceKind,
    SearchOptions,
    SearchResult,
} from "./types";

interface ApiSearchItem {
    object: string;
    id: string;
    url?: string;
    last_edited_time?: string;
    properties?: unknown;
    title?: unknown[];
}

/** Extract and validate a Notion ID from an ID, URL, or collection:// reference. */
export function notionId(ref: string): string {
    return idFromRef(ref);
}

/**
 * Retrieve the integration bot user.
 * @capability submilli/notion.getSelf {}
 */
export function getSelf(): NotionUser {
    check("submilli/notion.getSelf", {});
    return userFrom(notionGet("/users/me").json());
}

/**
 * Search titles visible to the connection.
 * @capability submilli/notion.search {}
 */
export function search(options: SearchOptions | null = null): PageResult<SearchResult> {
    check("submilli/notion.search", {});
    const fields: string[] = [];
    if (options !== null) {
        const actual = options as SearchOptions;
        if (actual.query !== null && actual.query.length > 0) fields.push(fieldJson("query", actual.query));
        if (actual.kind !== null) {
            fields.push("\"filter\":{\"property\":\"object\",\"value\":" + JSON.stringify(actual.kind) + "}");
        }
        if (actual.direction !== null) {
            const timestamp = actual.timestamp === null ? "last_edited_time" : actual.timestamp;
            fields.push("\"sort\":{\"direction\":" + JSON.stringify(actual.direction) + ",\"timestamp\":" + JSON.stringify(timestamp) + "}");
        }
        fields.push(fieldJson("page_size", pageSize(actual.pageSize)));
        if (actual.startCursor !== null) fields.push(fieldJson("start_cursor", actual.startCursor));
    } else {
        fields.push(fieldJson("page_size", 100));
    }
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
 * Fetch one resource using its explicit kind.
 * @capability submilli/notion.fetch { kind: string, id: string }
 */
export function fetch(resource: ResourceRef): FetchedResource {
    const id = idFromRef(resource.ref, resource.kind);
    check("submilli/notion.fetch", { kind: resource.kind as string, id: id });
    if (resource.kind === "page") return pageFrom(notionGet("/pages/" + pathId(id)).json());
    if (resource.kind === "database") return databaseFrom(notionGet("/databases/" + pathId(id)).json());
    if (resource.kind === "data_source") return dataSourceFrom(notionGet("/data_sources/" + pathId(id)).json());
    if (resource.kind === "block") return blockFrom(notionGet("/blocks/" + pathId(id)).json());
    if (resource.kind === "view") return viewFrom(notionGet("/views/" + pathId(id)).json());
    return userFrom(notionGet("/users/" + pathId(id)).json());
}

/**
 * Retrieve a page by ID or Notion URL.
 * @capability submilli/notion.fetchPage { pageId: string }
 */
export function fetchPage(ref: string): NotionPage {
    const id = idFromRef(ref, "page");
    check("submilli/notion.fetchPage", { pageId: id });
    return pageFrom(notionGet("/pages/" + pathId(id)).json());
}

/**
 * Retrieve a database container by ID or Notion URL.
 * @capability submilli/notion.fetchDatabase { databaseId: string }
 */
export function fetchDatabase(ref: string): NotionDatabase {
    const id = idFromRef(ref, "database");
    check("submilli/notion.fetchDatabase", { databaseId: id });
    return databaseFrom(notionGet("/databases/" + pathId(id)).json());
}

/**
 * Retrieve a data source by ID, Notion URL, or collection:// reference.
 * @capability submilli/notion.fetchDataSource { dataSourceId: string }
 */
export function fetchDataSource(ref: string): NotionDataSource {
    const id = idFromRef(ref, "data_source");
    check("submilli/notion.fetchDataSource", { dataSourceId: id });
    return dataSourceFrom(notionGet("/data_sources/" + pathId(id)).json());
}

/**
 * Retrieve a workspace user.
 * @capability submilli/notion.getUser { userId: string }
 */
export function getUser(ref: string): NotionUser {
    const id = idFromRef(ref, "user");
    check("submilli/notion.getUser", { userId: id });
    return userFrom(notionGet("/users/" + pathId(id)).json());
}

/**
 * List users visible to the connection.
 * @capability submilli/notion.listUsers {}
 */
export function listUsers(options: PageOptions | null = null): PageResult<NotionUser> {
    check("submilli/notion.listUsers", {});
    const query = new Map<string, string>();
    let requestedSize: number | null = null;
    if (options !== null) {
        const actual = options as PageOptions;
        requestedSize = actual.pageSize;
        putQuery(query, "start_cursor", actual.startCursor);
    }
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
        url: text(item.url),
        title: titleFrom(item),
        lastEditedTime: text(item.last_edited_time),
        raw: raw,
    };
}

function titleFrom(item: ApiSearchItem): string {
    if (item.title !== null) return richText(item.title);
    if (item.properties === null) return "";
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

function text(value: string | null): string {
    return value === null ? "" : value;
}
