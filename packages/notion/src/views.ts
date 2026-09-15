import { check } from "submilli:security";
import {
    CreateViewInput,
    NotionView,
    PageOptions,
    PageResult,
    ResourceRef,
    UpdateViewInput,
    ViewQuery,
} from "./types";
import {
    fieldJson,
    idFromRef,
    listFrom,
    notionGet,
    notionPatch,
    notionPost,
    objectJson,
    pageSize,
    pathId,
    putQuery,
    validationError,
    viewFrom,
    viewQueryFrom,
} from "./transport";

export {
    CreateViewInput,
    NotionView,
    PageOptions,
    PageResult,
    ResourceKind,
    ResourceRef,
    UpdateViewInput,
    ViewQuery,
} from "./types";

/**
 * Create a configured database view.
 * @capability submilli/notion.createView { databaseId: string, dataSourceId: string }
 */
export function createView(input: CreateViewInput): NotionView {
    const databaseId = idFromRef(input.databaseId, "database");
    const dataSourceId = idFromRef(input.dataSourceId, "data_source");
    check("submilli/notion.createView", { databaseId: databaseId, dataSourceId: dataSourceId });
    const fields: string[] = [
        fieldJson("database_id", databaseId),
        fieldJson("data_source_id", dataSourceId),
        fieldJson("name", input.name),
        fieldJson("type", input.type),
    ];
    if (input.filter !== null) fields.push(fieldJson("filter", input.filter));
    if (input.sorts !== null) fields.push(fieldJson("sorts", input.sorts));
    if (input.configuration !== null) fields.push(fieldJson("configuration", input.configuration));
    if (input.position !== null) fields.push(fieldJson("position", input.position));
    return viewFrom(notionPost("/views", objectJson(fields)).json());
}

/**
 * Update a view's saved query or presentation.
 * @capability submilli/notion.updateView { viewId: string }
 */
export function updateView(ref: string, input: UpdateViewInput): NotionView {
    const viewId = idFromRef(ref, "view");
    check("submilli/notion.updateView", { viewId: viewId });
    const fields: string[] = [];
    if (input.name !== null) fields.push(fieldJson("name", input.name));
    if (input.clearFilter === true) fields.push("\"filter\":null");
    else if (input.filter !== null) fields.push(fieldJson("filter", input.filter));
    if (input.clearSorts === true) fields.push("\"sorts\":null");
    else if (input.sorts !== null) fields.push(fieldJson("sorts", input.sorts));
    if (input.clearQuickFilters === true) fields.push("\"quick_filters\":null");
    else if (input.quickFilters !== null) fields.push(fieldJson("quick_filters", input.quickFilters));
    if (input.configuration !== null) fields.push(fieldJson("configuration", input.configuration));
    if (fields.length === 0) throw validationError("empty_update", "updateView requires at least one changed field");
    return viewFrom(notionPatch("/views/" + pathId(viewId), objectJson(fields)).json());
}

/**
 * List views belonging to a database.
 * @capability submilli/notion.listViews { databaseId: string }
 */
export function listViews(databaseRef: string, options: PageOptions | null = null): PageResult<NotionView> {
    const databaseId = idFromRef(databaseRef, "database");
    check("submilli/notion.listViews", { databaseId: databaseId });
    const query = new Map<string, string>();
    query.set("database_id", databaseId);
    let requestedSize: number | null = null;
    if (options !== null) {
        const actual = options as PageOptions;
        requestedSize = actual.pageSize;
        putQuery(query, "start_cursor", actual.startCursor);
    }
    query.set("page_size", pageSize(requestedSize).toString());
    const page = listFrom(notionGet("/views", query));
    const results: NotionView[] = [];
    for (const raw of page.results) results.push(viewFrom(raw));
    return {
        results: results,
        hasMore: page.hasMore,
        nextCursor: page.nextCursor,
        isComplete: page.isComplete,
    };
}

/**
 * Execute a view's saved filters and sorts.
 * @capability submilli/notion.queryView { viewId: string }
 */
export function queryView(ref: string, resultPageSize: number = 100): ViewQuery {
    const viewId = idFromRef(ref, "view");
    check("submilli/notion.queryView", { viewId: viewId });
    return viewQueryFrom(notionPost("/views/" + pathId(viewId) + "/queries", { page_size: pageSize(resultPageSize) }).json());
}

/**
 * Continue a cached view query.
 * @capability submilli/notion.continueViewQuery { viewId: string }
 */
export function continueViewQuery(
    viewRef: string,
    queryRef: string,
    startCursor: string = "",
    resultPageSize: number = 100,
): PageResult<ResourceRef> {
    const viewId = idFromRef(viewRef, "view");
    const queryId = idFromRef(queryRef);
    check("submilli/notion.continueViewQuery", { viewId: viewId });
    const query = new Map<string, string>();
    query.set("page_size", pageSize(resultPageSize).toString());
    putQuery(query, "start_cursor", startCursor);
    const page = listFrom(notionGet("/views/" + pathId(viewId) + "/queries/" + pathId(queryId), query));
    const results: ResourceRef[] = [];
    for (const raw of page.results) {
        if ("object" in raw && "id" in raw) {
            results.push({ kind: raw.object === "data_source" ? "data_source" : "page", ref: raw.id as string });
        }
    }
    return {
        results: results,
        hasMore: page.hasMore,
        nextCursor: page.nextCursor,
        isComplete: page.isComplete,
    };
}
