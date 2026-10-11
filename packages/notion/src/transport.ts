import { get, post, patch, Response } from "submilli:http";
import { encodeComponent, encodeQuery, parse } from "submilli:url";
import secrets from "submilli:secrets";
import {
    DataSourceReference,
    FileReference,
    FileUpload,
    NotionBlock,
    NotionComment,
    NotionDatabase,
    NotionDataSource,
    NotionPage,
    NotionParent,
    NotionUser,
    NotionView,
    PageMarkdown,
    PageResult,
    PropertyBag,
    PropertyValue,
    ResourceKind,
    ResourceRef,
    ViewQuery,
} from "./types";

export {
    DataSourceReference,
    FileReference,
    FileUpload,
    NotionBlock,
    NotionComment,
    NotionDatabase,
    NotionDataSource,
    NotionPage,
    NotionParent,
    NotionUser,
    NotionView,
    PageMarkdown,
    PageResult,
    PropertyBag,
    PropertyValue,
    ResourceKind,
    ResourceRef,
    ViewQuery,
} from "./types";

const API = "https://api.notion.com/v1";
const VERSION = "2026-03-11";

interface ApiErrorEnvelope {
    code?: string | null;
    message?: string | null;
    request_id?: string | null;
}

interface ApiList {
    results: unknown[];
    has_more: boolean;
    next_cursor: string | null;
    request_status?: ApiRequestStatus | null;
}

interface ApiRequestStatus {
    type: string;
}

interface ApiParent {
    type: string;
    page_id?: string | null;
    database_id?: string | null;
    data_source_id?: string | null;
    block_id?: string | null;
    agent_id?: string | null;
    workspace?: boolean | null;
}

interface ApiPage {
    id: string;
    url?: string | null;
    created_time?: string | null;
    last_edited_time?: string | null;
    in_trash?: boolean | null;
    parent?: ApiParent | null;
    properties?: unknown;
}

interface ApiDatabase {
    id: string;
    url?: string | null;
    title?: unknown[] | null;
    description?: unknown[] | null;
    created_time?: string | null;
    last_edited_time?: string | null;
    in_trash?: boolean | null;
    data_sources?: ApiDataSourceReference[] | null;
}

interface ApiDataSourceReference {
    id: string;
    name?: string | null;
}

interface ApiDataSource {
    id: string;
    url?: string | null;
    title?: unknown[] | null;
    created_time?: string | null;
    last_edited_time?: string | null;
    in_trash?: boolean | null;
    parent?: ApiParent | null;
    properties?: unknown;
}

interface ApiBlock {
    id: string;
    type?: string | null;
    created_time?: string | null;
    last_edited_time?: string | null;
    has_children?: boolean | null;
    in_trash?: boolean | null;
    parent?: ApiParent | null;
}

interface ApiUser {
    id: string;
    type?: string | null;
    name?: string | null;
    avatar_url?: string | null;
    person?: ApiPerson | null;
}

interface ApiPerson {
    email?: string | null;
}

interface ApiView {
    id: string;
    type?: string | null;
    name?: string | null;
    parent?: ApiParent | null;
}

interface ApiMarkdown {
    id: string;
    markdown: string;
    truncated?: boolean | null;
    unknown_block_ids?: string[] | null;
}

interface ApiComment {
    id: string;
    discussion_id?: string | null;
    created_time?: string | null;
    last_edited_time?: string | null;
    markdown?: string | null;
    rich_text?: ApiRichText[] | null;
}

interface ApiRichText {
    plain_text?: string | null;
}

interface ApiViewQuery {
    id: string;
    view_id: string;
    expires_at?: string | null;
    total_count?: number | null;
    results: ApiResourceReference[];
    next_cursor: string | null;
    has_more: boolean;
    request_status?: ApiRequestStatus | null;
}

interface ApiResourceReference {
    object: string;
    id: string;
}

interface ApiFileUpload {
    id: string;
    status: string;
    filename?: string | null;
    content_type?: string | null;
    content_length?: number | null;
    expiry_time?: string | null;
    upload_url?: string | null;
    complete_url?: string | null;
}

/** A Notion API or package validation error with retry and request metadata. */
export class NotionError extends Error {
    code: string;
    status: number;
    requestId: string;
    retryAfter: string;

    constructor(code: string, message: string, status: number, requestId: string = "", retryAfter: string = "") {
        super(message);
        this.name = "NotionError";
        this.code = code;
        this.status = status;
        this.requestId = requestId;
        this.retryAfter = retryAfter;
    }
}

/** A stopped sequential batch with the successfully completed IDs. */
export class BatchNotionError extends NotionError {
    completedIds: string[];
    failedIndex: number;

    constructor(cause: NotionError, completedIds: string[], failedIndex: number) {
        super(cause.code, cause.message, cause.status, cause.requestId, cause.retryAfter);
        this.name = "BatchNotionError";
        this.completedIds = completedIds;
        this.failedIndex = failedIndex;
    }
}

/** Resolved page policy coordinates for a page or nested block. */
export class PageContext {
    blockId: string;
    pageId: string;
    raw: unknown;

    constructor(blockId: string, pageId: string, raw: unknown) {
        this.blockId = blockId;
        this.pageId = pageId;
        this.raw = raw;
    }
}

export function notionGet(path: string, query?: Map<string, string>): Response {
    const suffix = query === undefined ? "" : querySuffix(query);
    return requireOk(get(API + path + suffix, authHeaders()));
}

// An omitted body sends an empty request body.
export function notionPost(path: string, body?: string | Uint8Array | {} | unknown[]): Response {
    return requireOk(post(API + path, body, authHeaders()));
}

export function notionPatch(path: string, body: string | Uint8Array | {} | unknown[]): Response {
    return requireOk(patch(API + path, body, authHeaders()));
}

/**
 * Resolve a page or nested block to the containing page used by policy.
 * The first block response is retained so getBlock does not fetch it twice.
 *
 * @param ref Page or block ID, or a Notion URL.
 * @returns Context holding the target block ID, the ID of its containing page, and the fetched block response.
 */
export function resolvePageContext(ref: string): PageContext {
    const blockId = idFromRef(ref);
    let currentId = blockId;
    let targetRaw: unknown = null;
    const visited = new Map<string, boolean>();
    for (let depth = 0; depth < 100; depth += 1) {
        if (visited.has(currentId)) {
            throw validationError("invalid_parent_chain", "Notion block parent chain contains a cycle");
        }
        visited.set(currentId, true);
        const raw = notionGet("/blocks/" + pathId(currentId)).json();
        if (depth === 0) targetRaw = raw;
        const block = raw as ApiBlock;
        const blockType = str(block.type);
        if (blockType === "child_page") return new PageContext(blockId, currentId, targetRaw);
        if (blockType === "child_database") {
            throw validationError("block_has_no_page_context", "Database blocks do not have a page capability context");
        }

        const parent = block.parent;
        if (!parent) {
            throw validationError("missing_parent", "Notion block does not include parent metadata");
        }
        if (parent.type === "page_id") {
            return new PageContext(blockId, requiredParentId(parent.page_id, "page_id"), targetRaw);
        }
        if (parent.type !== "block_id") {
            throw validationError(
                "block_has_no_page_context",
                "Notion block parent type " + parent.type + " does not provide a page capability context",
            );
        }
        currentId = requiredParentId(parent.block_id, "block_id");
    }
    throw validationError("parent_chain_too_deep", "Notion block parent chain exceeds 100 levels");
}

export function authHeaders(): Map<string, string> {
    const token = secrets.get("NOTION_ACCESS_TOKEN");
    if (token === undefined) throw validationError("missing_token", "NOTION_ACCESS_TOKEN is not bound");
    const headers = new Map<string, string>();
    headers.set("Authorization", "Bearer " + token);
    headers.set("Notion-Version", VERSION);
    headers.set("Content-Type", "application/json");
    return headers;
}

export function requireOk(response: Response): Response {
    if (response.ok) return response;
    let code = response.status === 401 ? "unauthorized" : "http_error";
    let message = "Notion request failed: HTTP " + response.status.toString() + " " + response.statusText;
    let requestId = header(response, "x-request-id");
    if (response.body.startsWith("{")) {
        const envelope = response.json() as ApiErrorEnvelope;
        code = envelope.code ?? code;
        message = envelope.message ?? message;
        requestId = envelope.request_id ?? requestId;
    }
    throw new NotionError(code, message, response.status, requestId, header(response, "retry-after"));
}

export function validationError(code: string, message: string): NotionError {
    return new NotionError(code, message, 0, "", "");
}

/**
 * Extract and validate a Notion ID from an ID, a Notion URL, or a collection:// reference.
 * A collection:// reference is rejected when a kind other than "data_source" is expected.
 *
 * @param ref Notion ID (dashed or undashed), Notion URL, or `collection://` data source reference; surrounding whitespace is ignored.
 * @param expected Resource kind the caller expects; omit it to skip the kind check. Only `"data_source"` accepts a `collection://` reference.
 * @returns The validated Notion ID extracted from the reference.
 */
export function idFromRef(ref: string, expected?: string): string {
    const trimmed = ref.trim();
    if (trimmed.length === 0) throw validationError("invalid_reference", "Notion reference cannot be empty");
    if (trimmed.startsWith("collection://")) {
        if (expected !== undefined && expected !== "data_source") {
            throw validationError("invalid_reference_kind", "collection:// references identify data sources");
        }
        return validateId(trimmed.slice("collection://".length));
    }
    if (trimmed.startsWith("http://") || trimmed.startsWith("https://")) return idFromUrl(trimmed);
    return validateId(trimmed);
}

export function pathId(ref: string, expected?: string): string {
    return encodeComponent(idFromRef(ref, expected));
}

export function pageSize(value: number | undefined, fallback: number = 100, max: number = 100): number {
    const actual = value ?? fallback;
    if (actual < 1 || actual > max) {
        throw validationError("invalid_page_size", "page size must be between 1 and " + max.toString());
    }
    return actual;
}

export function putQuery(query: Map<string, string>, key: string, value: string | undefined): void {
    if (value !== undefined && value.length > 0) query.set(key, value);
}

export function listFrom(response: Response): PageResult<unknown> {
    const data = response.json() as ApiList;
    return {
        results: data.results,
        hasMore: data.has_more,
        nextCursor: str(data.next_cursor),
        isComplete: data.request_status?.type !== "incomplete",
    };
}

export function pageFrom(raw: unknown): NotionPage {
    const data = raw as ApiPage;
    return {
        id: data.id,
        url: str(data.url),
        createdTime: str(data.created_time),
        lastEditedTime: str(data.last_edited_time),
        inTrash: data.in_trash === true,
        parent: parentFrom(data.parent),
        properties: unknownMap(data.properties),
        raw: raw,
    };
}

export function databaseFrom(raw: unknown): NotionDatabase {
    const data = raw as ApiDatabase;
    const dataSources: DataSourceReference[] = [];
    if (data.data_sources) {
        for (const item of data.data_sources) dataSources.push({ id: item.id, name: str(item.name) });
    }
    return {
        id: data.id,
        url: str(data.url),
        title: array(data.title),
        description: array(data.description),
        createdTime: str(data.created_time),
        lastEditedTime: str(data.last_edited_time),
        inTrash: data.in_trash === true,
        dataSources: dataSources,
        raw: raw,
    };
}

export function dataSourceFrom(raw: unknown): NotionDataSource {
    const data = raw as ApiDataSource;
    const parent = parentFrom(data.parent);
    return {
        id: data.id,
        databaseId: parent.id,
        url: str(data.url),
        title: array(data.title),
        createdTime: str(data.created_time),
        lastEditedTime: str(data.last_edited_time),
        inTrash: data.in_trash === true,
        properties: unknownMap(data.properties),
        raw: raw,
    };
}

export function blockFrom(raw: unknown): NotionBlock {
    const data = raw as ApiBlock;
    return {
        id: data.id,
        type: str(data.type),
        createdTime: str(data.created_time),
        lastEditedTime: str(data.last_edited_time),
        hasChildren: data.has_children === true,
        inTrash: data.in_trash === true,
        raw: raw,
    };
}

export function userFrom(raw: unknown): NotionUser {
    const data = raw as ApiUser;
    let email = "";
    if (data.person) email = str(data.person.email);
    return {
        id: data.id,
        type: str(data.type),
        name: str(data.name),
        avatarUrl: str(data.avatar_url),
        personEmail: email,
        raw: raw,
    };
}

export function viewFrom(raw: unknown): NotionView {
    const data = raw as ApiView;
    return {
        id: data.id,
        type: str(data.type),
        name: str(data.name),
        parentDatabaseId: parentFrom(data.parent).id,
        raw: raw,
    };
}

export function markdownFrom(raw: unknown): PageMarkdown {
    const data = raw as ApiMarkdown;
    let unknownBlockIds: string[] = [];
    if (data.unknown_block_ids) unknownBlockIds = data.unknown_block_ids;
    return {
        id: data.id,
        markdown: data.markdown,
        truncated: data.truncated === true,
        unknownBlockIds: unknownBlockIds,
    };
}

export function commentFrom(raw: unknown): NotionComment {
    const data = raw as ApiComment;
    let markdown = str(data.markdown);
    if (markdown.length === 0 && data.rich_text) {
        const parts: string[] = [];
        for (const item of data.rich_text) parts.push(str(item.plain_text));
        markdown = parts.join("");
    }
    return {
        id: data.id,
        discussionId: str(data.discussion_id),
        createdTime: str(data.created_time),
        lastEditedTime: str(data.last_edited_time),
        markdown: markdown,
        raw: raw,
    };
}

export function viewQueryFrom(raw: unknown): ViewQuery {
    const data = raw as ApiViewQuery;
    const results: ResourceRef[] = [];
    for (const item of data.results) results.push({ kind: kindFromObject(item.object), ref: item.id });
    return {
        id: data.id,
        viewId: data.view_id,
        expiresAt: str(data.expires_at),
        totalCount: data.total_count ?? 0,
        results: results,
        nextCursor: str(data.next_cursor),
        hasMore: data.has_more,
        isComplete: data.request_status?.type !== "incomplete",
    };
}

export function fileUploadFrom(raw: unknown): FileUpload {
    const data = raw as ApiFileUpload;
    return {
        id: data.id,
        status: data.status,
        filename: str(data.filename),
        contentType: str(data.content_type),
        contentLength: data.content_length ?? 0,
        expiryTime: str(data.expiry_time),
        uploadUrl: str(data.upload_url),
        completeUrl: str(data.complete_url),
        raw: raw,
    };
}

export function propertyObject(properties: PropertyBag): Map<string, unknown> {
    const result = new Map<string, unknown>();
    if (properties.values !== undefined) {
        for (const property of properties.values) {
            if (result.has(property.name)) throw validationError("duplicate_property", "duplicate property: " + property.name);
            result.set(property.name, propertyValue(property));
        }
    }
    if (properties.custom !== undefined) {
        for (const entry of properties.custom) {
            if (result.has(entry[0])) throw validationError("duplicate_property", "duplicate property: " + entry[0]);
            result.set(entry[0], entry[1]);
        }
    }
    return result;
}

// Like `fieldJson`, every entry must have a JSON value: an `undefined` value throws
// rather than silently dropping the key.
export function mapJson(map: Map<string, unknown>): string {
    const fields: string[] = [];
    for (const entry of map) fields.push(fieldJson(entry[0], entry[1]));
    return "{" + fields.join(",") + "}";
}

export function objectJson(fields: string[]): string {
    return "{" + fields.join(",") + "}";
}

export function fieldJson(name: string, value: unknown): string {
    const encoded = JSON.stringify(value);
    if (encoded === undefined) throw validationError("invalid_body", name + " must have a JSON value");
    return JSON.stringify(name) + ":" + encoded;
}

export function mapFieldJson(name: string, value: Map<string, unknown>): string {
    return JSON.stringify(name) + ":" + mapJson(value);
}

export function fileReferenceJson(file: FileReference): string {
    if (file.type === "external") {
        if (file.url === undefined || !file.url.startsWith("https://")) {
            throw validationError("invalid_file_reference", "external files require an HTTPS URL");
        }
        return "{\"type\":\"external\",\"external\":{\"url\":" + JSON.stringify(file.url) + "}}";
    }
    if (file.uploadId === undefined) throw validationError("invalid_file_reference", "file_upload references require uploadId");
    return "{\"type\":\"file_upload\",\"file_upload\":{\"id\":" + JSON.stringify(idFromRef(file.uploadId)) + "}}";
}

export function trustedUploadPath(url: string): string {
    const parsed = parse(url);
    if (parsed.protocol !== "https" || parsed.host !== "api.notion.com" || parsed.port !== undefined || !url.startsWith(API + "/")) {
        throw validationError("invalid_upload_url", "Notion returned an untrusted upload URL");
    }
    return url.slice(API.length);
}

function idFromUrl(value: string): string {
    const url = parse(value);
    if (url.protocol !== "https" || (url.host !== "www.notion.so" && url.host !== "notion.so")) {
        throw validationError("invalid_reference_url", "Notion URLs must use https://notion.so");
    }
    const parts = url.path.split("/");
    for (let index = parts.length - 1; index >= 0; index -= 1) {
        const candidate = idAtEnd(parts[index]);
        if (candidate.length > 0) return validateId(candidate);
    }
    throw validationError("invalid_reference_url", "Notion URL does not contain a resource ID");
}

function idAtEnd(segment: string): string {
    const clean = segment.split("?")[0].split("#")[0];
    if (clean.length >= 32) {
        const candidate = clean.slice(clean.length - 32);
        if (isHex(candidate)) return candidate;
    }
    if (clean.length >= 36) {
        const candidate = clean.slice(clean.length - 36);
        if (isUuid(candidate)) return candidate;
    }
    return "";
}

function validateId(value: string): string {
    const clean = value.trim();
    if (!isHex(clean) && !isUuid(clean)) {
        throw validationError("invalid_reference", "Notion IDs must be 32 hexadecimal characters, with optional UUID dashes");
    }
    return clean;
}

function isHex(value: string): boolean {
    if (value.length !== 32) return false;
    for (let index = 0; index < value.length; index += 1) {
        const code = value.charCodeAt(index);
        const digit = code >= 48 && code <= 57;
        const lower = code >= 97 && code <= 102;
        const upper = code >= 65 && code <= 70;
        if (!digit && !lower && !upper) return false;
    }
    return true;
}

function isUuid(value: string): boolean {
    if (value.length !== 36) return false;
    if (value.charAt(8) !== "-" || value.charAt(13) !== "-" || value.charAt(18) !== "-" || value.charAt(23) !== "-") return false;
    return isHex(value.replaceAll("-", ""));
}

function propertyValue(property: PropertyValue): unknown {
    const propertyType = property.type as string;
    if (property.type === "title" || property.type === "rich_text") {
        const text = property.text;
        if (text === undefined) throw validationError("invalid_property", propertyType + " property requires text");
        return JSON.parse("{\"" + propertyType + "\":[{\"type\":\"text\",\"text\":{\"content\":" + JSON.stringify(text) + "}}]}");
    }
    if (property.type === "number") {
        // An omitted number clears the property, as an explicit null does.
        return JSON.parse("{\"number\":" + JSON.stringify(property.number ?? null) + "}");
    }
    if (property.type === "checkbox") {
        const checked = property.checked;
        if (checked === undefined) throw validationError("invalid_property", "checkbox property requires checked");
        return { checkbox: checked };
    }
    if (property.type === "select" || property.type === "status") {
        const value = property.value ?? null;
        const selection = value === null ? "null" : "{\"name\":" + JSON.stringify(value) + "}";
        return JSON.parse("{\"" + propertyType + "\":" + selection + "}");
    }
    if (property.type === "multi_select") {
        if (property.values === undefined) throw validationError("invalid_property", "multi_select property requires values");
        const selections: string[] = [];
        for (const value of property.values) selections.push("{\"name\":" + JSON.stringify(value) + "}");
        return JSON.parse("{\"multi_select\":[" + selections.join(",") + "]}");
    }
    if (property.type === "date") {
        const start = property.start ?? null;
        if (start === null) return JSON.parse("{\"date\":null}");
        const fields: string[] = [fieldJson("start", start)];
        if (property.end !== undefined) fields.push(fieldJson("end", property.end));
        if (property.timeZone !== undefined) fields.push(fieldJson("time_zone", property.timeZone));
        return JSON.parse("{\"date\":" + objectJson(fields) + "}");
    }
    if (property.type === "people" || property.type === "relation") {
        if (property.ids === undefined) throw validationError("invalid_property", propertyType + " property requires ids");
        const ids: string[] = [];
        for (const id of property.ids) ids.push("{\"id\":" + JSON.stringify(idFromRef(id)) + "}");
        return JSON.parse("{\"" + propertyType + "\":[" + ids.join(",") + "]}");
    }
    if (property.type === "files") {
        if (property.files === undefined) throw validationError("invalid_property", "files property requires files");
        const files: string[] = [];
        for (const file of property.files) {
            const name = file.name === undefined ? "" : ",\"name\":" + JSON.stringify(file.name);
            const encoded = fileReferenceJson(file);
            files.push(encoded.slice(0, encoded.length - 1) + name + "}");
        }
        return JSON.parse("{\"files\":[" + files.join(",") + "]}");
    }
    return JSON.parse("{\"" + propertyType + "\":" + JSON.stringify(property.value ?? null) + "}");
}

function unknownMap(value: unknown): Map<string, unknown> {
    const result = new Map<string, unknown>();
    if (value === null || value === undefined) return result;
    for (const entry of Object.entries(value)) result.set(entry[0], entry[1]);
    return result;
}

function parentFrom(parent: ApiParent | null | undefined): NotionParent {
    if (!parent) return { type: "", id: "" };
    const id = parent.page_id ?? parent.database_id ?? parent.data_source_id ?? parent.block_id ?? "";
    return { type: parent.type, id: id };
}

function kindFromObject(value: string): ResourceKind {
    if (value === "database") return "database";
    if (value === "data_source") return "data_source";
    if (value === "block") return "block";
    if (value === "view") return "view";
    if (value === "user") return "user";
    return "page";
}

function querySuffix(query: Map<string, string>): string {
    const encoded = encodeQuery(query);
    return encoded.length === 0 ? "" : "?" + encoded;
}

function header(response: Response, name: string): string {
    const value = response.headers.get(name);
    return value === undefined ? "" : value;
}

function str(value: string | null | undefined): string {
    return value ?? "";
}

function requiredParentId(value: string | null | undefined, parentType: string): string {
    if (value === undefined || value === null) throw validationError("missing_parent_id", "Notion " + parentType + " parent does not include its ID");
    return validateId(value);
}

function array(value: unknown[] | null | undefined): unknown[] {
    if (!value) return [];
    return value;
}
