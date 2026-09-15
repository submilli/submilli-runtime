import { check } from "submilli:security";
import {
    CreateCommentInput,
    FileReference,
    MeetingNotesOptions,
    MeetingNotesResult,
    NotionBlock,
    NotionComment,
    PageOptions,
    PageResult,
} from "./types";
import {
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

interface ApiMeetingNotes {
    results: unknown[];
    has_more: boolean;
}

interface ApiDiscussionComment {
    discussion_id?: string;
}

/**
 * Create a Markdown comment on a page, block, or existing discussion.
 * @capability submilli/notion.createComment { pageId: string }
 */
export function createComment(input: CreateCommentInput): NotionComment {
    const targetId = idFromRef(input.target.id);
    if (input.markdown.length === 0) throw validationError("invalid_comment", "comment markdown cannot be empty");
    const fields: string[] = [fieldJson("markdown", input.markdown)];
    if (input.target.type === "discussion") {
        fields.push(fieldJson("discussion_id", targetId));
    } else {
        const parentType = input.target.type === "page" ? "page_id" : "block_id";
        fields.push("\"parent\":{\"" + parentType + "\":" + JSON.stringify(targetId) + "}");
    }
    if (input.attachments !== null) {
        if (input.attachments.length > 3) throw validationError("invalid_attachments", "comments support at most three attachments");
        fields.push(attachmentsJson(input.attachments));
    }
    let pageId = targetId;
    if (input.target.type === "block") {
        pageId = resolvePageContext(targetId).pageId;
    } else if (input.target.type === "discussion") {
        const parentRef = input.target.discussionParentRef;
        if (parentRef === null) {
            throw validationError(
                "missing_discussion_parent",
                "discussion comments require target.discussionParentRef for page capability context",
            );
        }
        const context = resolvePageContext(parentRef);
        verifyDiscussionParent(context.blockId, targetId);
        pageId = context.pageId;
    }
    check("submilli/notion.createComment", { pageId: pageId });
    return commentFrom(notionPost("/comments", objectJson(fields)).json());
}

/**
 * List open comments for a page or block.
 * @capability submilli/notion.getComments { pageId: string }
 */
export function getComments(ref: string, options: PageOptions | null = null): PageResult<NotionComment> {
    const context = resolvePageContext(ref);
    check("submilli/notion.getComments", { pageId: context.pageId });
    const query = new Map<string, string>();
    query.set("block_id", context.blockId);
    let requestedSize: number | null = null;
    if (options !== null) {
        const actual = options as PageOptions;
        requestedSize = actual.pageSize;
        putQuery(query, "start_cursor", actual.startCursor);
    }
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
 * @capability submilli/notion.queryMeetingNotes {}
 */
export function queryMeetingNotes(options: MeetingNotesOptions | null = null): MeetingNotesResult {
    check("submilli/notion.queryMeetingNotes", {});
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
