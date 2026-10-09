import {
    CreateViewInput,
    NotionView,
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

/** The resolved view and cached result set that continueViewQuery pages through. */
export interface PreparedViewQuery {
    /** UUID of the queried view. */
    viewId: string;
    /** ID of the cached result set. */
    queryId: string;
}

/**
 * Create a configured database view on a resolved database and data source.
 *
 * @param databaseId Resolved ID of the database the view belongs to.
 * @param dataSourceId Resolved ID of the data source the view displays.
 * @param input View name, type, and optional filter, sorts, configuration, and position.
 * @returns The created view.
 */
export function createView(databaseId: string, dataSourceId: string, input: CreateViewInput): NotionView {
    const fields: string[] = [
        fieldJson("database_id", databaseId),
        fieldJson("data_source_id", dataSourceId),
        fieldJson("name", input.name),
        fieldJson("type", input.type),
    ];
    if (input.filter !== null && input.filter !== undefined) fields.push(fieldJson("filter", input.filter));
    if (input.sorts !== undefined) fields.push(fieldJson("sorts", input.sorts));
    if (input.configuration !== null && input.configuration !== undefined) fields.push(fieldJson("configuration", input.configuration));
    if (input.position !== null && input.position !== undefined) fields.push(fieldJson("position", input.position));
    return viewFrom(notionPost("/views", objectJson(fields)).json());
}

/**
 * Update a view's saved query or presentation.
 *
 * @param viewId View ID or Notion URL.
 * @param input Fields to change; at least one must be set, and the clear flags remove the saved filter, sorts, or quick filters.
 * @returns The updated view.
 */
export function updateView(viewId: string, input: UpdateViewInput): NotionView {
    const fields: string[] = [];
    if (input.name !== undefined) fields.push(fieldJson("name", input.name));
    if (input.clearFilter === true) fields.push("\"filter\":null");
    else if (input.filter !== null && input.filter !== undefined) fields.push(fieldJson("filter", input.filter));
    if (input.clearSorts === true) fields.push("\"sorts\":null");
    else if (input.sorts !== undefined) fields.push(fieldJson("sorts", input.sorts));
    if (input.clearQuickFilters === true) fields.push("\"quick_filters\":null");
    else if (input.quickFilters !== null && input.quickFilters !== undefined) fields.push(fieldJson("quick_filters", input.quickFilters));
    if (input.configuration !== null && input.configuration !== undefined) fields.push(fieldJson("configuration", input.configuration));
    if (fields.length === 0) throw validationError("empty_update", "updateView requires at least one changed field");
    return viewFrom(notionPatch("/views/" + pathId(viewId), objectJson(fields)).json());
}

/**
 * List views belonging to a database.
 *
 * @param databaseId Resolved ID of the database.
 * @param requestedSize Views per page, 1 to 100; undefined uses 100.
 * @param startCursor Cursor from a previous page's `nextCursor`; undefined starts at the first view.
 * @returns One page of views; an empty `results` means the database has none.
 */
export function listViews(
    databaseId: string,
    requestedSize: number | undefined,
    startCursor: string | undefined,
): PageResult<NotionView> {
    const query = new Map<string, string>();
    query.set("database_id", databaseId);
    putQuery(query, "start_cursor", startCursor);
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
 *
 * @param viewId View ID or Notion URL.
 * @param resultPageSize Results in the first page, 1 to 100; defaults to 100.
 * @returns The cached query with its ID, first page of results, total count, and expiry time.
 */
export function queryView(viewId: string, resultPageSize: number = 100): ViewQuery {
    return viewQueryFrom(notionPost("/views/" + pathId(viewId) + "/queries", { page_size: pageSize(resultPageSize) }).json());
}

/**
 * Resolve the view and the cached result set of a continued view query before any request is sent.
 *
 * @param viewRef View ID or Notion URL.
 * @param queryRef ID of the cached view query returned by `queryView`.
 * @returns The validated view and query IDs.
 */
export function prepareContinueViewQuery(viewRef: string, queryRef: string): PreparedViewQuery {
    const viewId = idFromRef(viewRef, "view");
    const queryId = idFromRef(queryRef);
    return { viewId: viewId, queryId: queryId };
}

/**
 * Continue a cached view query.
 *
 * @param viewId Resolved view ID.
 * @param queryId Resolved ID of the cached view query.
 * @param startCursor Cursor from the previous page's `nextCursor`; empty starts at the beginning.
 * @param resultPageSize Results per page, 1 to 100; defaults to 100.
 * @returns One page of page and data source references; fetch them for full objects.
 */
export function continueViewQuery(
    viewId: string,
    queryId: string,
    startCursor: string = "",
    resultPageSize: number = 100,
): PageResult<ResourceRef> {
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
