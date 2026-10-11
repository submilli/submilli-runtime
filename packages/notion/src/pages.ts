import { encodeComponent } from "submilli:url";
import {
    CreatePageInput,
    FileReference,
    MovePageInput,
    NotionPage,
    PageContent,
    PageMarkdown,
    PropertyBag,
    UpdatePageInput,
} from "./types";
import {
    BatchNotionError,
    NotionError,
    fieldJson,
    fileReferenceJson,
    idFromRef,
    mapFieldJson,
    markdownFrom,
    notionGet,
    notionPatch,
    notionPost,
    objectJson,
    pageFrom,
    pathId,
    propertyObject,
    validationError,
} from "./transport";

export {
    CreatePageInput,
    FileReference,
    MovePageInput,
    NotionPage,
    NotionParent,
    PageContent,
    PageMarkdown,
    PageParent,
    PropertyBag,
    PropertyValue,
    UpdatePageInput,
} from "./types";

/** One validated page move with its page and parent resolved. */
export interface PreparedPageMove {
    /** UUID of the page to move. */
    pageId: string;
    /** Parent type: "page_id", "data_source_id", or "workspace". */
    parentType: string;
    /** Resolved parent ID; "workspace" for a workspace parent. */
    parentId: string;
}

/**
 * Validate a page creation input, its parent included, before any request is sent.
 *
 * @param input Page creation input to validate.
 */
export function validateCreatePageInput(input: CreatePageInput): void {
    // Called for its validation; `createPage` resolves the ID again when it creates the page.
    parentIdFrom(input.parent.type, input.parent.id);
    validatePageFields(input);
}

/**
 * Validate the properties, content, icon, and cover of a page creation before any request is sent.
 *
 * @param input Page creation input whose properties, content, icon, and cover are validated.
 */
export function validatePageFields(input: CreatePageInput): void {
    if (input.properties !== undefined) propertyObject(input.properties);
    if (input.content !== undefined) contentJson(input.content);
    if (input.icon !== undefined) fileReferenceJson(input.icon);
    if (input.cover !== undefined) fileReferenceJson(input.cover);
}

/**
 * Create a page from a validated input under its resolved parent.
 *
 * @param parentId Resolved parent ID from `parentIdFrom`; "workspace" for a workspace parent.
 * @param input Validated page creation input.
 * @returns The created page.
 */
export function createPage(parentId: string, input: CreatePageInput): NotionPage {
    const fields: string[] = [parentJson(input.parent.type, parentId)];
    if (input.properties !== undefined) fields.push(mapFieldJson("properties", propertyObject(input.properties)));
    if (input.content !== undefined) fields.push(contentJson(input.content));
    if (input.icon !== undefined) fields.push(fileField("icon", input.icon));
    if (input.cover !== undefined) fields.push(fileField("cover", input.cover));
    return pageFrom(notionPost("/pages", objectJson(fields)).json());
}

/**
 * Update page properties, icon, cover, or apply a template.
 *
 * @param pageId Page ID or Notion URL.
 * @param input Fields to change; at least one must be set. A `null` icon or cover removes it, as the clear flags do.
 * @returns The updated page.
 */
export function updatePage(pageId: string, input: UpdatePageInput): NotionPage {
    const fields: string[] = [];
    const icon = input.icon;
    const cover = input.cover;
    if (input.properties !== undefined) fields.push(mapFieldJson("properties", propertyObject(input.properties)));
    if (input.clearIcon === true || icon === null) fields.push("\"icon\":null");
    else if (icon !== undefined) fields.push(fileField("icon", icon));
    if (input.clearCover === true || cover === null) fields.push("\"cover\":null");
    else if (cover !== undefined) fields.push(fileField("cover", cover));
    if (input.templateId !== undefined) {
        const templateFields: string[] = [fieldJson("type", "template_id"), fieldJson("template_id", idFromRef(input.templateId))];
        if (input.templateTimeZone !== undefined) templateFields.push(fieldJson("timezone", input.templateTimeZone));
        fields.push("\"template\":" + objectJson(templateFields));
    }
    if (fields.length === 0) throw validationError("empty_update", "updatePage requires at least one changed field");
    return pageFrom(notionPatch("/pages/" + pathId(pageId), objectJson(fields)).json());
}

/**
 * Retrieve a page as enhanced Markdown.
 *
 * @param pageId Page ID or Notion URL.
 * @param includeTranscript Whether to include meeting transcripts in the Markdown; defaults to false.
 * @returns The page content as enhanced Markdown with truncation state.
 */
export function readPageMarkdown(pageId: string, includeTranscript: boolean = false): PageMarkdown {
    const query = new Map<string, string>();
    if (includeTranscript) query.set("include_transcript", "true");
    return markdownFrom(notionGet("/pages/" + pathId(pageId) + "/markdown", query).json());
}

/**
 * Replace matching enhanced Markdown content.
 *
 * @param pageId Page ID or Notion URL.
 * @param oldText Existing Markdown text to find; must not be empty.
 * @param newText Replacement Markdown text.
 * @param replaceAll True replaces every match; false, the default, replaces one.
 * @returns The updated page Markdown.
 */
export function updatePageMarkdown(
    pageId: string,
    oldText: string,
    newText: string,
    replaceAll: boolean = false,
): PageMarkdown {
    if (oldText.length === 0) throw validationError("invalid_markdown_update", "oldText cannot be empty");
    const content: string[] = [fieldJson("old_str", oldText), fieldJson("new_str", newText)];
    if (replaceAll) content.push(fieldJson("replace_all_matches", true));
    const body = "{\"type\":\"update_content\",\"update_content\":{\"content_updates\":["
        + objectJson(content) + "]}}";
    return markdownFrom(notionPatch("/pages/" + pathId(pageId) + "/markdown", body).json());
}

/**
 * Replace all page content with enhanced Markdown.
 *
 * @param pageId Page ID or Notion URL.
 * @param markdown New enhanced Markdown content for the whole page.
 * @param allowDeletingContent Whether child pages or databases may be deleted by the replacement; defaults to false.
 * @returns The page Markdown after replacement.
 */
export function replacePageMarkdown(pageId: string, markdown: string, allowDeletingContent: boolean = false): PageMarkdown {
    const body = "{\"type\":\"replace_content\",\"replace_content\":{\"new_str\":" + JSON.stringify(markdown)
        + ",\"allow_deleting_content\":" + JSON.stringify(allowDeletingContent) + "}}";
    return markdownFrom(notionPatch("/pages/" + pathId(pageId) + "/markdown", body).json());
}

/**
 * Append enhanced Markdown to the end of a page.
 *
 * @param pageId Page ID or Notion URL.
 * @param markdown Enhanced Markdown to insert at the end of the page.
 * @returns The page Markdown after the append.
 */
export function appendPageMarkdown(pageId: string, markdown: string): PageMarkdown {
    const body = "{\"type\":\"insert_content\",\"insert_content\":{\"content\":" + JSON.stringify(markdown)
        + ",\"position\":{\"type\":\"end\"}}}";
    return markdownFrom(notionPatch("/pages/" + pathId(pageId) + "/markdown", body).json());
}

/**
 * Move a page under its resolved parent page, data source, or the workspace.
 *
 * @param pageId Page ID or Notion URL of the page to move.
 * @param parentType Destination parent type: "page_id", "data_source_id", or "workspace".
 * @param parentId Resolved destination parent ID; unused for a workspace parent.
 * @returns The moved page.
 */
export function movePage(pageId: string, parentType: string, parentId: string): NotionPage {
    const body = objectJson([parentJson(parentType, parentId)]);
    return pageFrom(notionPost("/pages/" + pathId(pageId) + "/move", body).json());
}

/**
 * Move prepared pages sequentially, stopping on the first failure with completed IDs.
 *
 * @param moves Prepared moves, applied in order.
 * @returns The moved pages in the same order.
 */
export function movePages(moves: PreparedPageMove[]): NotionPage[] {
    const pages: NotionPage[] = [];
    for (let index = 0; index < moves.length; index += 1) {
        const move = moves[index];
        try {
            pages.push(movePage(move.pageId, move.parentType, move.parentId));
        } catch (error) {
            const ids: string[] = [];
            for (const page of pages) ids.push(page.id);
            if (error instanceof NotionError) throw new BatchNotionError(error, ids, index);
            throw error;
        }
    }
    return pages;
}

/**
 * Retrieve one page property item, including paginated property values.
 *
 * @param pageId Page ID or Notion URL.
 * @param propertyId Notion property ID or name; must not be empty.
 * @returns Raw Notion property item, or a paginated list of items for multi-value properties.
 */
export function getPageProperty(pageId: string, propertyId: string): unknown {
    if (propertyId.length === 0) throw validationError("invalid_property_id", "propertyId cannot be empty");
    return notionGet("/pages/" + pathId(pageId) + "/properties/" + encodeComponent(propertyId)).json();
}

/**
 * Move a page to trash.
 *
 * @param pageId Page ID or Notion URL.
 * @returns The page as returned after trashing, with `inTrash` true.
 */
export function trashPage(pageId: string): NotionPage {
    return pageFrom(notionPatch("/pages/" + pathId(pageId), { in_trash: true }).json());
}

/**
 * Restore a page from trash.
 *
 * @param pageId Page ID or Notion URL.
 * @returns The restored page with `inTrash` false.
 */
export function restorePage(pageId: string): NotionPage {
    return pageFrom(notionPatch("/pages/" + pathId(pageId), { in_trash: false }).json());
}

/**
 * Resolve the ID of a page parent from its type and reference.
 * A workspace parent has no ID and resolves to "workspace"; any other parent requires a reference.
 *
 * @param parentType Parent type: "page_id", "data_source_id", or "workspace".
 * @param parentRef Parent ID, URL, or reference; may be omitted only for a workspace parent.
 * @returns The validated parent ID, or "workspace".
 */
export function parentIdFrom(parentType: string, parentRef: string | undefined): string {
    if (parentType === "workspace") return "workspace";
    if (parentRef === undefined) throw validationError("invalid_parent", parentType + " parent requires an ID");
    const expected = parentType === "page_id" ? "page" : "data_source";
    return idFromRef(parentRef, expected);
}

function parentJson(parentType: string, parentId: string): string {
    if (parentType === "workspace") return "\"parent\":{\"type\":\"workspace\",\"workspace\":true}";
    return "\"parent\":{\"type\":" + JSON.stringify(parentType) + "," + JSON.stringify(parentType) + ":" + JSON.stringify(parentId) + "}";
}

function contentJson(content: PageContent): string {
    if (content.type === "none") return "\"template\":{\"type\":\"none\"}";
    if (content.type === "markdown") {
        if (content.markdown === undefined) throw validationError("invalid_content", "markdown content requires markdown");
        return fieldJson("markdown", content.markdown);
    }
    if (content.type === "blocks") {
        if (content.children === undefined) throw validationError("invalid_content", "blocks content requires children");
        return fieldJson("children", content.children);
    }
    if (content.template === undefined) throw validationError("invalid_template", "template content requires a template type");
    const fields: string[] = [fieldJson("type", content.template)];
    if (content.template === "template_id") {
        if (content.templateId === undefined) throw validationError("invalid_template", "template_id content requires templateId");
        fields.push(fieldJson("template_id", idFromRef(content.templateId)));
    }
    if (content.timeZone !== undefined) fields.push(fieldJson("timezone", content.timeZone));
    return "\"template\":" + objectJson(fields);
}

function fileField(name: string, file: FileReference): string {
    return JSON.stringify(name) + ":" + fileReferenceJson(file);
}
