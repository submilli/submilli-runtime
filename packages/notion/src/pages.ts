import { check } from "submilli:security";
import { encodeComponent } from "submilli:url";
import {
    CreatePageInput,
    FileReference,
    MarkdownUpdate,
    MovePageInput,
    NotionPage,
    PageContent,
    PageMarkdown,
    PageParent,
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
    MarkdownUpdate,
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

/**
 * Create a page with properties and one explicit content strategy.
 * @capability submilli/notion.createPage { parentId: string }
 */
export function createPage(input: CreatePageInput): NotionPage {
    const parentId = validateCreatePageInput(input);
    check("submilli/notion.createPage", { parentId: parentId });
    return createPageRequest(input);
}

/**
 * Create pages sequentially, stopping on the first failure with completed IDs.
 * @capability submilli/notion.createPages {}
 */
export function createPages(inputs: CreatePageInput[]): NotionPage[] {
    for (const input of inputs) validateCreatePageInput(input);
    check("submilli/notion.createPages", {});
    const pages: NotionPage[] = [];
    for (let index = 0; index < inputs.length; index += 1) {
        try {
            pages.push(createPageRequest(inputs[index]));
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
 * Update page properties, icon, cover, or apply a template.
 * @capability submilli/notion.updatePage { pageId: string }
 */
export function updatePage(ref: string, input: UpdatePageInput): NotionPage {
    const pageId = idFromRef(ref, "page");
    check("submilli/notion.updatePage", { pageId: pageId });
    const fields: string[] = [];
    if (input.properties !== null) fields.push(mapFieldJson("properties", propertyObject(input.properties)));
    if (input.clearIcon === true) fields.push("\"icon\":null");
    else if (input.icon !== null) fields.push(fileField("icon", input.icon));
    if (input.clearCover === true) fields.push("\"cover\":null");
    else if (input.cover !== null) fields.push(fileField("cover", input.cover));
    if (input.templateId !== null) {
        const templateFields: string[] = [fieldJson("type", "template_id"), fieldJson("template_id", idFromRef(input.templateId))];
        if (input.templateTimeZone !== null) templateFields.push(fieldJson("timezone", input.templateTimeZone));
        fields.push("\"template\":" + objectJson(templateFields));
    }
    if (fields.length === 0) throw validationError("empty_update", "updatePage requires at least one changed field");
    return pageFrom(notionPatch("/pages/" + pathId(pageId), objectJson(fields)).json());
}

/**
 * Retrieve a page as enhanced Markdown.
 * @capability submilli/notion.readPageMarkdown { pageId: string }
 */
export function readPageMarkdown(ref: string, includeTranscript: boolean = false): PageMarkdown {
    const pageId = idFromRef(ref, "page");
    check("submilli/notion.readPageMarkdown", { pageId: pageId });
    const query = new Map<string, string>();
    if (includeTranscript) query.set("include_transcript", "true");
    return markdownFrom(notionGet("/pages/" + pathId(pageId) + "/markdown", query).json());
}

/**
 * Replace matching enhanced Markdown content.
 * @capability submilli/notion.updatePageMarkdown { pageId: string }
 */
export function updatePageMarkdown(ref: string, update: MarkdownUpdate): PageMarkdown {
    const pageId = idFromRef(ref, "page");
    check("submilli/notion.updatePageMarkdown", { pageId: pageId });
    if (update.oldText.length === 0) throw validationError("invalid_markdown_update", "oldText cannot be empty");
    const content: string[] = [fieldJson("old_str", update.oldText), fieldJson("new_str", update.newText)];
    if (update.replaceAll === true) content.push(fieldJson("replace_all_matches", true));
    const body = "{\"type\":\"update_content\",\"update_content\":{\"content_updates\":["
        + objectJson(content) + "]}}";
    return markdownFrom(notionPatch("/pages/" + pathId(pageId) + "/markdown", body).json());
}

/**
 * Replace all page content with enhanced Markdown.
 * @capability submilli/notion.replacePageMarkdown { pageId: string }
 */
export function replacePageMarkdown(ref: string, markdown: string, allowDeletingContent: boolean = false): PageMarkdown {
    const pageId = idFromRef(ref, "page");
    check("submilli/notion.replacePageMarkdown", { pageId: pageId });
    const body = "{\"type\":\"replace_content\",\"replace_content\":{\"new_str\":" + JSON.stringify(markdown)
        + ",\"allow_deleting_content\":" + JSON.stringify(allowDeletingContent) + "}}";
    return markdownFrom(notionPatch("/pages/" + pathId(pageId) + "/markdown", body).json());
}

/**
 * Append enhanced Markdown to the end of a page.
 * @capability submilli/notion.appendPageMarkdown { pageId: string }
 */
export function appendPageMarkdown(ref: string, markdown: string): PageMarkdown {
    const pageId = idFromRef(ref, "page");
    check("submilli/notion.appendPageMarkdown", { pageId: pageId });
    const body = "{\"type\":\"insert_content\",\"insert_content\":{\"content\":" + JSON.stringify(markdown)
        + ",\"position\":{\"type\":\"end\"}}}";
    return markdownFrom(notionPatch("/pages/" + pathId(pageId) + "/markdown", body).json());
}

/**
 * Move a page under another page or data source.
 * @capability submilli/notion.movePage { pageId: string, parentId: string }
 */
export function movePage(ref: string, parent: PageParent): NotionPage {
    const pageId = idFromRef(ref, "page");
    const parentId = parentIdFrom(parent);
    check("submilli/notion.movePage", { pageId: pageId, parentId: parentId });
    return movePageRequest(pageId, parent);
}

/**
 * Move pages sequentially, stopping on the first failure with completed IDs.
 * @capability submilli/notion.movePages {}
 */
export function movePages(inputs: MovePageInput[]): NotionPage[] {
    for (const input of inputs) {
        idFromRef(input.page, "page");
        parentIdFrom(input.parent);
    }
    check("submilli/notion.movePages", {});
    const pages: NotionPage[] = [];
    for (let index = 0; index < inputs.length; index += 1) {
        const input = inputs[index];
        try {
            pages.push(movePageRequest(idFromRef(input.page, "page"), input.parent));
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
 * @capability submilli/notion.getPageProperty { pageId: string }
 */
export function getPageProperty(pageRef: string, propertyId: string): unknown {
    const pageId = idFromRef(pageRef, "page");
    check("submilli/notion.getPageProperty", { pageId: pageId });
    if (propertyId.length === 0) throw validationError("invalid_property_id", "propertyId cannot be empty");
    return notionGet("/pages/" + pathId(pageId) + "/properties/" + encodeComponent(propertyId)).json();
}

/**
 * Move a page to trash.
 * @capability submilli/notion.trashPage { pageId: string }
 */
export function trashPage(ref: string): NotionPage {
    const pageId = idFromRef(ref, "page");
    check("submilli/notion.trashPage", { pageId: pageId });
    return pageFrom(notionPatch("/pages/" + pathId(pageId), { in_trash: true }).json());
}

/**
 * Restore a page from trash.
 * @capability submilli/notion.restorePage { pageId: string }
 */
export function restorePage(ref: string): NotionPage {
    const pageId = idFromRef(ref, "page");
    check("submilli/notion.restorePage", { pageId: pageId });
    return pageFrom(notionPatch("/pages/" + pathId(pageId), { in_trash: false }).json());
}

function createPageRequest(input: CreatePageInput): NotionPage {
    const fields: string[] = [parentJson(input.parent)];
    if (input.properties !== null) fields.push(mapFieldJson("properties", propertyObject(input.properties)));
    if (input.content !== null) fields.push(contentJson(input.content));
    if (input.icon !== null) fields.push(fileField("icon", input.icon));
    if (input.cover !== null) fields.push(fileField("cover", input.cover));
    return pageFrom(notionPost("/pages", objectJson(fields)).json());
}

function validateCreatePageInput(input: CreatePageInput): string {
    const parentId = parentIdFrom(input.parent);
    if (input.properties !== null) propertyObject(input.properties);
    if (input.content !== null) contentJson(input.content);
    if (input.icon !== null) fileReferenceJson(input.icon);
    if (input.cover !== null) fileReferenceJson(input.cover);
    return parentId;
}

function movePageRequest(pageId: string, parent: PageParent): NotionPage {
    const body = objectJson([parentJson(parent)]);
    return pageFrom(notionPost("/pages/" + pathId(pageId) + "/move", body).json());
}

function parentJson(parent: PageParent): string {
    if (parent.type === "workspace") return "\"parent\":{\"type\":\"workspace\",\"workspace\":true}";
    const id = parentIdFrom(parent);
    return "\"parent\":{\"type\":" + JSON.stringify(parent.type) + "," + JSON.stringify(parent.type) + ":" + JSON.stringify(id) + "}";
}

function parentIdFrom(parent: PageParent): string {
    if (parent.type === "workspace") return "workspace";
    const parentType = parent.type as string;
    if (parent.id === null) throw validationError("invalid_parent", parentType + " parent requires an ID");
    const expected = parent.type === "page_id" ? "page" : "data_source";
    return idFromRef(parent.id, expected);
}

function contentJson(content: PageContent): string {
    if (content.type === "none") return "\"template\":{\"type\":\"none\"}";
    if (content.type === "markdown") {
        if (content.markdown === null) throw validationError("invalid_content", "markdown content requires markdown");
        return fieldJson("markdown", content.markdown);
    }
    if (content.type === "blocks") {
        if (content.children === null) throw validationError("invalid_content", "blocks content requires children");
        return fieldJson("children", content.children);
    }
    if (content.template === null) throw validationError("invalid_template", "template content requires a template type");
    const fields: string[] = [fieldJson("type", content.template)];
    if (content.template === "template_id") {
        if (content.templateId === null) throw validationError("invalid_template", "template_id content requires templateId");
        fields.push(fieldJson("template_id", idFromRef(content.templateId)));
    }
    if (content.timeZone !== null) fields.push(fieldJson("timezone", content.timeZone));
    return "\"template\":" + objectJson(fields);
}

function fileField(name: string, file: FileReference | null): string {
    if (file === null) return JSON.stringify(name) + ":null";
    return JSON.stringify(name) + ":" + fileReferenceJson(file);
}
