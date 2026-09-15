import { check } from "submilli:security";
import {
    NotionBlock,
    PageOptions,
    PageResult,
} from "./types";
import {
    blockFrom,
    fieldJson,
    listFrom,
    mapJson,
    notionGet,
    notionPatch,
    objectJson,
    pageSize,
    pathId,
    putQuery,
    resolvePageContext,
    validationError,
} from "./transport";

export {
    NotionBlock,
    PageOptions,
    PageResult,
} from "./types";

/**
 * Retrieve one block.
 * @capability submilli/notion.getBlock { pageId: string }
 */
export function getBlock(ref: string): NotionBlock {
    const context = resolvePageContext(ref);
    check("submilli/notion.getBlock", { pageId: context.pageId });
    return blockFrom(context.raw);
}

/**
 * List direct children of a block or page.
 * @capability submilli/notion.listBlockChildren { pageId: string }
 */
export function listBlockChildren(ref: string, options: PageOptions | null = null): PageResult<NotionBlock> {
    const context = resolvePageContext(ref);
    check("submilli/notion.listBlockChildren", { pageId: context.pageId });
    const query = new Map<string, string>();
    let requestedSize: number | null = null;
    if (options !== null) {
        const actual = options as PageOptions;
        requestedSize = actual.pageSize;
        putQuery(query, "start_cursor", actual.startCursor);
    }
    query.set("page_size", pageSize(requestedSize).toString());
    const page = listFrom(notionGet("/blocks/" + pathId(context.blockId) + "/children", query));
    const results: NotionBlock[] = [];
    for (const raw of page.results) results.push(blockFrom(raw));
    return {
        results: results,
        hasMore: page.hasMore,
        nextCursor: page.nextCursor,
        isComplete: page.isComplete,
    };
}

/**
 * Append block children, optionally at an explicit Notion position.
 * @capability submilli/notion.appendBlockChildren { pageId: string }
 */
export function appendBlockChildrenJson(
    ref: string,
    childrenJson: string[],
    positionJson: string,
): PageResult<NotionBlock> {
    if (childrenJson.length === 0 || childrenJson.length > 100) {
        throw validationError("invalid_children", "appendBlockChildren requires between 1 and 100 children");
    }
    for (const childJson of childrenJson) JSON.parse(childJson);
    if (positionJson.length > 0) JSON.parse(positionJson);
    const context = resolvePageContext(ref);
    check("submilli/notion.appendBlockChildren", { pageId: context.pageId });
    const fields: string[] = ["\"children\":[" + childrenJson.join(",") + "]"];
    if (positionJson.length > 0) fields.push("\"position\":" + positionJson);
    const page = listFrom(notionPatch("/blocks/" + pathId(context.blockId) + "/children", objectJson(fields)));
    const results: NotionBlock[] = [];
    for (const raw of page.results) results.push(blockFrom(raw));
    return {
        results: results,
        hasMore: page.hasMore,
        nextCursor: page.nextCursor,
        isComplete: page.isComplete,
    };
}

/**
 * Update a block using fields from its Notion block type.
 * @capability submilli/notion.updateBlock { pageId: string }
 */
export function updateBlock(ref: string, fields: Map<string, unknown>): NotionBlock {
    if (fields.size === 0) throw validationError("empty_update", "updateBlock requires at least one changed field");
    const context = resolvePageContext(ref);
    check("submilli/notion.updateBlock", { pageId: context.pageId });
    return blockFrom(notionPatch("/blocks/" + pathId(context.blockId), mapJson(fields)).json());
}

/**
 * Move a block to trash.
 * @capability submilli/notion.trashBlock { pageId: string }
 */
export function trashBlock(ref: string): NotionBlock {
    const context = resolvePageContext(ref);
    check("submilli/notion.trashBlock", { pageId: context.pageId });
    return blockFrom(notionPatch("/blocks/" + pathId(context.blockId), { in_trash: true }).json());
}

/**
 * Restore a block from trash.
 * @capability submilli/notion.restoreBlock { pageId: string }
 */
export function restoreBlock(ref: string): NotionBlock {
    const context = resolvePageContext(ref);
    check("submilli/notion.restoreBlock", { pageId: context.pageId });
    return blockFrom(notionPatch("/blocks/" + pathId(context.blockId), { in_trash: false }).json());
}
