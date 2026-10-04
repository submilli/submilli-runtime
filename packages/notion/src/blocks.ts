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

/**
 * Return the block a page context was resolved from; the context retains its response.
 *
 * @param context Page context resolved from a block or page reference.
 * @returns The block the context was resolved from.
 */
export function getBlock(context: PageContext): NotionBlock {
    return blockFrom(context.raw);
}

/**
 * List direct children of the block or page a context was resolved from.
 *
 * @param context Page context of the parent block or page.
 * @param requestedSize Children per page, 1 to 100; `null` uses 100.
 * @param startCursor Cursor from a previous page's `nextCursor`; `null` starts at the first child.
 * @returns One page of child blocks with pagination state.
 */
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
 *
 * @param ref Parent page or block ID, or a Notion URL.
 * @param childrenJson JSON-encoded block objects to append, 1 to 100 entries; each must parse as JSON.
 * @param positionJson JSON-encoded Notion position object; empty appends at the end.
 * @returns Page context of the target, to pass to `appendBlockChildrenJson`.
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

/**
 * Append validated block children, optionally at an explicit Notion position.
 *
 * @param context Page context of the parent block or page.
 * @param childrenJson JSON-encoded block objects to append.
 * @param positionJson JSON-encoded Notion position object; empty appends at the end.
 * @returns The newly appended blocks as one page of results.
 */
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

/**
 * Reject an empty block update before any request is sent.
 *
 * @param fields Changed block fields keyed by Notion block property name.
 */
export function validateUpdateBlock(fields: Map<string, unknown>): void {
    if (fields.size === 0) throw validationError("empty_update", "updateBlock requires at least one changed field");
}

/**
 * Update a block using fields from its Notion block type.
 *
 * @param context Page context of the block to update.
 * @param fields Changed block fields keyed by Notion block property name, such as `paragraph`.
 * @returns The updated block.
 */
export function updateBlock(context: PageContext, fields: Map<string, unknown>): NotionBlock {
    return blockFrom(notionPatch("/blocks/" + pathId(context.blockId), mapJson(fields)).json());
}

/**
 * Move a block to trash.
 *
 * @param context Page context of the block to trash.
 * @returns The block as returned after trashing, with `inTrash` true.
 */
export function trashBlock(context: PageContext): NotionBlock {
    return blockFrom(notionPatch("/blocks/" + pathId(context.blockId), { in_trash: true }).json());
}

/**
 * Restore a block from trash.
 *
 * @param context Page context of the block to restore.
 * @returns The restored block with `inTrash` false.
 */
export function restoreBlock(context: PageContext): NotionBlock {
    return blockFrom(notionPatch("/blocks/" + pathId(context.blockId), { in_trash: false }).json());
}
