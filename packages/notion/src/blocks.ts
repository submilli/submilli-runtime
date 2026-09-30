import {
    NotionBlock,
    PageResult,
} from "./types";
import {
    PageContext,
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
export { PageContext } from "./transport";

/** Return the block a page context was resolved from; the context retains its response. */
export function getBlock(context: PageContext): NotionBlock {
    return blockFrom(context.raw);
}

/** List direct children of the block or page a context was resolved from. */
export function listBlockChildren(
    context: PageContext,
    requestedSize: number | null,
    startCursor: string | null,
): PageResult<NotionBlock> {
    const query = new Map<string, string>();
    putQuery(query, "start_cursor", startCursor);
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
 * Validate the children and position of an append, then resolve the page context of its target.
 * An empty positionJson appends at the end.
 */
export function prepareAppendBlockChildren(
    ref: string,
    childrenJson: string[],
    positionJson: string,
): PageContext {
    if (childrenJson.length === 0 || childrenJson.length > 100) {
        throw validationError("invalid_children", "appendBlockChildren requires between 1 and 100 children");
    }
    for (const childJson of childrenJson) JSON.parse(childJson);
    if (positionJson.length > 0) JSON.parse(positionJson);
    return resolvePageContext(ref);
}

/** Append validated block children, optionally at an explicit Notion position. */
export function appendBlockChildrenJson(
    context: PageContext,
    childrenJson: string[],
    positionJson: string,
): PageResult<NotionBlock> {
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

/** Reject an empty block update before any request is sent. */
export function validateUpdateBlock(fields: Map<string, unknown>): void {
    if (fields.size === 0) throw validationError("empty_update", "updateBlock requires at least one changed field");
}

/** Update a block using fields from its Notion block type. */
export function updateBlock(context: PageContext, fields: Map<string, unknown>): NotionBlock {
    return blockFrom(notionPatch("/blocks/" + pathId(context.blockId), mapJson(fields)).json());
}

/** Move a block to trash. */
export function trashBlock(context: PageContext): NotionBlock {
    return blockFrom(notionPatch("/blocks/" + pathId(context.blockId), { in_trash: true }).json());
}

/** Restore a block from trash. */
export function restoreBlock(context: PageContext): NotionBlock {
    return blockFrom(notionPatch("/blocks/" + pathId(context.blockId), { in_trash: false }).json());
}
