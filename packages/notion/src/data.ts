import { encodeComponent } from "submilli:url";
import {
    DataSourceQueryItem,
    DataSourceTemplate,
    NotionDatabase,
    NotionDataSource,
    PageResult,
    QueryDataSourceOptions,
} from "./types";
import {
    dataSourceFrom,
    databaseFrom,
    fieldJson,
    idFromRef,
    listFrom,
    mapFieldJson,
    notionGet,
    notionPatch,
    notionPost,
    objectJson,
    pageFrom,
    pageSize,
    pathId,
    putQuery,
    validationError,
} from "./transport";

export {
    CreateDatabaseInput,
    DataSourceReference,
    DataSourceQueryItem,
    DataSourceTemplate,
    ListTemplateOptions,
    NotionDatabase,
    NotionDataSource,
    NotionPage,
    NotionParent,
    PageResult,
    QueryDataSourceOptions,
    UpdateDataSourceInput,
} from "./types";

interface ApiTemplateList {
    templates: ApiTemplate[];
    has_more: boolean;
    next_cursor: string | null;
}

interface ApiTemplate {
    id: string;
    name: string;
    is_default: boolean;
}

interface ApiObject {
    object: string;
    id: string;
}

/**
 * Create a database container, initial data source, and initial table view under a resolved parent page.
 *
 * @param parentId Resolved ID of the parent page.
 * @param title Plain-text database title.
 * @param description Plain-text description; `null` omits it.
 * @param isInline True creates the database inline in the parent page; `null` uses the Notion default.
 * @param properties Property schema for the initial data source, keyed by property name; must not be empty.
 * @returns The created database, including its initial data source reference.
 */
export function createDatabase(
    parentId: string,
    title: string,
    description: string | null,
    isInline: boolean | null,
    properties: Map<string, unknown>,
): NotionDatabase {
    if (properties.size === 0) throw validationError("invalid_database", "database properties cannot be empty");
    const fields: string[] = [
        "\"parent\":{\"type\":\"page_id\",\"page_id\":" + JSON.stringify(parentId) + "}",
        richTextField("title", title),
        "\"initial_data_source\":{" + mapFieldJson("properties", properties) + "}",
    ];
    if (description !== null) fields.push(richTextField("description", description));
    if (isInline !== null) fields.push(fieldJson("is_inline", isInline));
    return databaseFrom(notionPost("/databases", objectJson(fields)).json());
}

/**
 * Update a data source title, schema, or parent database.
 *
 * @param dataSourceId Data source ID or Notion URL.
 * @param title New plain-text title; `null` leaves it unchanged.
 * @param properties Property schema changes keyed by property name; `null` leaves the schema unchanged.
 * @param databaseId ID of the database to move the data source into; `null` keeps its current parent.
 * @returns The updated data source.
 */
export function updateDataSource(
    dataSourceId: string,
    title: string | null,
    properties: Map<string, unknown> | null,
    databaseId: string | null,
): NotionDataSource {
    const fields: string[] = [];
    if (title !== null) fields.push(richTextField("title", title));
    if (properties !== null) fields.push(mapFieldJson("properties", properties));
    if (databaseId !== null) {
        fields.push("\"parent\":{\"type\":\"database_id\",\"database_id\":" + JSON.stringify(idFromRef(databaseId, "database")) + "}");
    }
    if (fields.length === 0) throw validationError("empty_update", "updateDataSource requires at least one changed field");
    return dataSourceFrom(notionPatch("/data_sources/" + pathId(dataSourceId), objectJson(fields)).json());
}

/**
 * Query pages and nested data sources using structured Notion filters and sorts.
 *
 * @param dataSourceId Data source ID or Notion URL.
 * @param options Filter, sorts, and pagination; `null` returns the first 100 results unfiltered.
 * @returns One page of pages and nested data sources; an empty `results` means nothing matched.
 */
export function queryDataSource(dataSourceId: string, options: QueryDataSourceOptions | null = null): PageResult<DataSourceQueryItem> {
    const fields: string[] = [];
    let path = "/data_sources/" + pathId(dataSourceId) + "/query";
    if (options !== null) {
        const actual = options as QueryDataSourceOptions;
        if (actual.filter !== null) fields.push(fieldJson("filter", actual.filter));
        if (actual.sorts !== null) fields.push(fieldJson("sorts", actual.sorts));
        if (actual.inTrash !== null) fields.push(fieldJson("in_trash", actual.inTrash));
        if (actual.resultType !== null) fields.push(fieldJson("result_type", actual.resultType));
        if (actual.startCursor !== null) fields.push(fieldJson("start_cursor", actual.startCursor));
        fields.push(fieldJson("page_size", pageSize(actual.pageSize)));
        if (actual.filterProperties !== null) path += filterPropertiesQuery(actual.filterProperties);
    } else {
        fields.push(fieldJson("page_size", 100));
    }
    const page = listFrom(notionPost(path, objectJson(fields)));
    const results: DataSourceQueryItem[] = [];
    for (const raw of page.results) results.push(queryItemFrom(raw));
    return {
        results: results,
        hasMore: page.hasMore,
        nextCursor: page.nextCursor,
        isComplete: page.isComplete,
    };
}

/**
 * List page templates available to a data source.
 *
 * @param dataSourceId Data source ID or Notion URL.
 * @param name Template name to match; `null` lists all templates.
 * @param requestedSize Templates per page, 1 to 100; `null` uses 100.
 * @param startCursor Cursor from a previous page's `nextCursor`; `null` starts at the first template.
 * @returns One page of templates; an empty `results` means none are defined.
 */
export function listDataSourceTemplates(
    dataSourceId: string,
    name: string | null,
    requestedSize: number | null,
    startCursor: string | null,
): PageResult<DataSourceTemplate> {
    const query = new Map<string, string>();
    putQuery(query, "name", name);
    putQuery(query, "start_cursor", startCursor);
    query.set("page_size", pageSize(requestedSize).toString());
    const data = notionGet("/data_sources/" + pathId(dataSourceId) + "/templates", query).json() as ApiTemplateList;
    const results: DataSourceTemplate[] = [];
    for (const item of data.templates) results.push({ id: item.id, name: item.name, isDefault: item.is_default });
    return {
        results: results,
        hasMore: data.has_more,
        nextCursor: data.next_cursor === null ? "" : data.next_cursor,
        isComplete: true,
    };
}

/**
 * Move a database container to trash.
 *
 * @param databaseId Database ID or Notion URL.
 * @returns The database as returned after trashing, with `inTrash` true.
 */
export function trashDatabase(databaseId: string): NotionDatabase {
    return databaseFrom(notionPatch("/databases/" + pathId(databaseId), { in_trash: true }).json());
}

/**
 * Restore a database container from trash.
 *
 * @param databaseId Database ID or Notion URL.
 * @returns The restored database with `inTrash` false.
 */
export function restoreDatabase(databaseId: string): NotionDatabase {
    return databaseFrom(notionPatch("/databases/" + pathId(databaseId), { in_trash: false }).json());
}

function queryItemFrom(raw: unknown): DataSourceQueryItem {
    const item = raw as ApiObject;
    if (item.object === "data_source") {
        return { kind: "data_source", id: item.id, page: null, dataSource: dataSourceFrom(raw), raw: raw };
    }
    return { kind: "page", id: item.id, page: pageFrom(raw), dataSource: null, raw: raw };
}

function richTextField(name: string, text: string): string {
    return JSON.stringify(name) + ":[{\"type\":\"text\",\"text\":{\"content\":" + JSON.stringify(text) + "}}]";
}

function filterPropertiesQuery(properties: string[]): string {
    if (properties.length === 0) return "";
    const values: string[] = [];
    for (const property of properties) {
        values.push(encodeComponent("filter_properties[]") + "=" + encodeComponent(property));
    }
    return "?" + values.join("&");
}
