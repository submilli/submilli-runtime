import {
    FileReference,
    MeetingNotesOptions,
    MeetingNotesResult,
    NotionBlock,
    NotionComment,
    PageResult,
} from "./types";
import {
    PageContext,
    blockFrom,
    commentFrom,
    fieldJson,
    fileReferenceJson,
    idFromRef,
    listFrom,
    notionGet,
    notionPost,
    objectJson,
    pageSize,
    putQuery,
    resolvePageContext,
    validationError,
} from "./transport";

export {
    CommentTarget,
    CreateCommentInput,
    FileReference,
    MeetingNotesOptions,
    MeetingNotesResult,
    NotionBlock,
    NotionComment,
    PageOptions,
    PageResult,
} from "./types";
export { PageContext } from "./transport";

interface ApiMeetingNotes {
    results: unknown[];
    has_more: boolean;
}

interface ApiDiscussionComment {
    discussion_id?: string;
}

/** A comment to create, as the package read it from the caller's input. */
export interface CommentRequest {
    /** Kind of object the comment is attached to. */
    targetType: "page" | "block" | "discussion";
    /** URL or ID of the target. */
    targetRef: string;
    /** Comment text. */
    markdown: string;
    /** Files to attach, or null for none. */
    attachments: FileReference[] | null;
}

/**
 * Validate a comment and build its request body.
 *
 * @param request Comment target, Markdown text, and optional attachments.
 * @returns JSON request body for the create-comment call.
 */
export function createCommentBody(request: CommentRequest): string {
    const targetType = request.targetType;
    const markdown = request.markdown;
    const attachments = request.attachments;
    const targetId = idFromRef(request.targetRef);
    if (markdown.length === 0) throw validationError("invalid_comment", "comment markdown cannot be empty");
    const fields: string[] = [fieldJson("markdown", markdown)];
    if (targetType === "discussion") {
        fields.push(fieldJson("discussion_id", targetId));
    } else {
        const parentType = targetType === "page" ? "page_id" : "block_id";
        fields.push("\"parent\":{\"" + parentType + "\":" + JSON.stringify(targetId) + "}");
    }
    if (attachments !== null) {
        if (attachments.length > 3) throw validationError("invalid_attachments", "comments support at most three attachments");
        fields.push(attachmentsJson(attachments));
    }
    return objectJson(fields);
}

/**
 * Resolve the page that contains a comment target.
 * Block and discussion targets are resolved over the network, and a discussion must belong to its stated parent.
 *
 * @param targetType Kind of object the comment is attached to.
 * @param targetRef ID or Notion URL of the page, block, or discussion.
 * @param discussionParentRef Page or block that contains the discussion; required when `targetType` is "discussion", otherwise unused.
 * @returns ID of the page that contains the target, used for the page capability check.
 */
export function commentPageId(
    targetType: "page" | "block" | "discussion",
    targetRef: string,
    discussionParentRef: string | null,
): string {
    const targetId = idFromRef(targetRef);
    if (targetType === "block") return resolvePageContext(targetId).pageId;
    if (targetType !== "discussion") return targetId;
    if (discussionParentRef === null) {
        throw validationError(
            "missing_discussion_parent",
            "discussion comments require target.discussionParentRef for page capability context",
        );
    }
    const context = resolvePageContext(discussionParentRef);
    verifyDiscussionParent(context.blockId, targetId);
    return context.pageId;
}

/**
 * Send a create-comment request built by `createCommentBody`.
 *
 * @param body JSON request body from `createCommentBody`.
 * @returns The created comment.
 */
export function createComment(body: string): NotionComment {
    return commentFrom(notionPost("/comments", body).json());
}

/**
 * List open comments for the page or block a context was resolved from.
 *
 * @param context Page context of the page or block to list comments for.
 * @param requestedSize Comments per page, 1 to 100; `null` uses 100.
 * @param startCursor Cursor from a previous page's `nextCursor`; `null` starts at the first comment.
 * @returns One page of open comments; an empty `results` means there are none.
 */
export function getComments(
    context: PageContext,
    requestedSize: number | null,
    startCursor: string | null,
): PageResult<NotionComment> {
    const query = new Map<string, string>();
    query.set("block_id", context.blockId);
    putQuery(query, "start_cursor", startCursor);
    query.set("page_size", pageSize(requestedSize).toString());
    const page = listFrom(notionGet("/comments", query));
    const results: NotionComment[] = [];
    for (const raw of page.results) results.push(commentFrom(raw));
    return {
        results: results,
        hasMore: page.hasMore,
        nextCursor: page.nextCursor,
        isComplete: page.isComplete,
    };
}

/**
 * Query AI meeting-note blocks visible to the integration user.
 *
 * @param options Filter, sorts, and limit (1 to 50, default 50); `null` sends no filter or sort and uses the API defaults.
 * @returns Matching meeting-note blocks and whether more exist.
 */
export function queryMeetingNotes(options: MeetingNotesOptions | null = null): MeetingNotesResult {
    const fields: string[] = [];
    if (options !== null) {
        const actual = options as MeetingNotesOptions;
        if (actual.filter !== null) fields.push(fieldJson("filter", actual.filter));
        if (actual.sorts !== null) fields.push(fieldJson("sort", actual.sorts));
        fields.push(fieldJson("limit", pageSize(actual.limit, 50, 50)));
    }
    const data = notionPost("/blocks/meeting_notes/query", objectJson(fields)).json() as ApiMeetingNotes;
    const results: NotionBlock[] = [];
    for (const raw of data.results) results.push(blockFrom(raw));
    return { results: results, hasMore: data.has_more };
}

function attachmentsJson(files: FileReference[]): string {
    const values: string[] = [];
    for (const file of files) values.push(fileReferenceJson(file));
    return "\"attachments\":[" + values.join(",") + "]";
}

function verifyDiscussionParent(blockId: string, discussionId: string): void {
    let cursor = "";
    for (let pageNumber = 0; pageNumber < 100; pageNumber += 1) {
        const query = new Map<string, string>();
        query.set("block_id", blockId);
        query.set("page_size", "100");
        if (cursor.length > 0) query.set("start_cursor", cursor);
        const page = listFrom(notionGet("/comments", query));
        for (const raw of page.results) {
            const comment = raw as ApiDiscussionComment;
            if (comment.discussion_id === discussionId) return;
        }
        if (!page.hasMore) break;
        cursor = page.nextCursor;
    }
    throw validationError(
        "invalid_discussion_parent",
        "discussionParentRef does not contain the requested Notion discussion",
    );
}
