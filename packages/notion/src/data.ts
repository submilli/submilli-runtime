import { encodeComponent } from "submilli:url";
import { check } from "submilli:security";
import {
    CreateDatabaseInput,
    DataSourceQueryItem,
    DataSourceTemplate,
    ListTemplateOptions,
    NotionDatabase,
    NotionDataSource,
    PageResult,
    QueryDataSourceOptions,
    UpdateDataSourceInput,
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
 * Create a database container, initial data source, and initial table view.
 * @capability submilli/notion.createDatabase { parentId: string }
 */
export function createDatabase(input: CreateDatabaseInput): NotionDatabase {
    const parentId = idFromRef(input.parentPage, "page");
    check("submilli/notion.createDatabase", { parentId: parentId });
    if (input.properties.size === 0) throw validationError("invalid_database", "database properties cannot be empty");
    const fields: string[] = [
        "\"parent\":{\"type\":\"page_id\",\"page_id\":" + JSON.stringify(parentId) + "}",
        richTextField("title", input.title),
        "\"initial_data_source\":{" + mapFieldJson("properties", input.properties) + "}",
    ];
    if (input.description !== null) fields.push(richTextField("description", input.description));
    if (input.isInline !== null) fields.push(fieldJson("is_inline", input.isInline));
    return databaseFrom(notionPost("/databases", objectJson(fields)).json());
}

/**
 * Update a data source title, schema, or parent database.
 * @capability submilli/notion.updateDataSource { dataSourceId: string }
 */
export function updateDataSource(ref: string, input: UpdateDataSourceInput): NotionDataSource {
    const dataSourceId = idFromRef(ref, "data_source");
    check("submilli/notion.updateDataSource", { dataSourceId: dataSourceId });
    const fields: string[] = [];
    if (input.title !== null) fields.push(richTextField("title", input.title));
    if (input.properties !== null) fields.push(mapFieldJson("properties", input.properties));
    if (input.databaseId !== null) {
        fields.push("\"parent\":{\"type\":\"database_id\",\"database_id\":" + JSON.stringify(idFromRef(input.databaseId, "database")) + "}");
    }
    if (fields.length === 0) throw validationError("empty_update", "updateDataSource requires at least one changed field");
    return dataSourceFrom(notionPatch("/data_sources/" + pathId(dataSourceId), objectJson(fields)).json());
}

/**
 * Query pages and nested data sources using structured Notion filters and sorts.
 * @capability submilli/notion.queryDataSource { dataSourceId: string }
 */
export function queryDataSource(ref: string, options: QueryDataSourceOptions | null = null): PageResult<DataSourceQueryItem> {
    const dataSourceId = idFromRef(ref, "data_source");
    check("submilli/notion.queryDataSource", { dataSourceId: dataSourceId });
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
 * @capability submilli/notion.listDataSourceTemplates { dataSourceId: string }
 */
export function listDataSourceTemplates(ref: string, options: ListTemplateOptions | null = null): PageResult<DataSourceTemplate> {
    const dataSourceId = idFromRef(ref, "data_source");
    check("submilli/notion.listDataSourceTemplates", { dataSourceId: dataSourceId });
    const query = new Map<string, string>();
    let requestedSize: number | null = null;
    if (options !== null) {
        const actual = options as ListTemplateOptions;
        requestedSize = actual.pageSize;
        putQuery(query, "name", actual.name);
        putQuery(query, "start_cursor", actual.startCursor);
    }
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
 * @capability submilli/notion.trashDatabase { databaseId: string }
 */
export function trashDatabase(ref: string): NotionDatabase {
    const databaseId = idFromRef(ref, "database");
    check("submilli/notion.trashDatabase", { databaseId: databaseId });
    return databaseFrom(notionPatch("/databases/" + pathId(databaseId), { in_trash: true }).json());
}

/**
 * Restore a database container from trash.
 * @capability submilli/notion.restoreDatabase { databaseId: string }
 */
export function restoreDatabase(ref: string): NotionDatabase {
    const databaseId = idFromRef(ref, "database");
    check("submilli/notion.restoreDatabase", { databaseId: databaseId });
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
