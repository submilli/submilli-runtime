import { post } from "submilli:http";
import { readBytes, stat } from "submilli:fs";
import {
    FileUpload,
    FileUploadOptions,
    PageResult,
} from "./types";
import {
    authHeaders,
    fieldJson,
    fileUploadFrom,
    listFrom,
    notionGet,
    notionPost,
    objectJson,
    pageSize,
    pathId,
    putQuery,
    requireOk,
    trustedUploadPath,
    validationError,
} from "./transport";

const API = "https://api.notion.com/v1";
const SINGLE_PART_LIMIT = 20971520;
const DEFAULT_CHUNK_SIZE = 10485760;
const MIN_CHUNK_SIZE = 5242880;
const MAX_CHUNK_SIZE = 20971520;

export {
    FileUpload,
    FileUploadOptions,
    PageOptions,
    PageResult,
} from "./types";

/**
 * Upload a VFS file, automatically selecting Notion single- or multipart mode.
 *
 * @param sourcePath Path of a file in the virtual filesystem; files over 20 MiB use a multipart upload.
 * @param options Filename, content type, and optional multipart chunk size.
 * @returns The completed file upload, whose ID can be attached to pages, blocks, or comments.
 */
export function uploadFile(sourcePath: string, options: FileUploadOptions): FileUpload {
    validateUploadOptions(options);
    const source = stat(sourcePath);
    if (source === null || source.kind !== "file") {
        throw validationError("source_not_found", "upload source is not a VFS file");
    }
    if (source.size <= SINGLE_PART_LIMIT) return uploadSinglePart(sourcePath, source.size, options);
    return uploadMultiPart(sourcePath, source.size, options);
}

/**
 * Retrieve one of this connection's file uploads by its resolved ID.
 *
 * @param uploadId Resolved ID of the file upload.
 * @returns The file upload with its current status.
 */
export function getFileUpload(uploadId: string): FileUpload {
    return fileUploadFrom(notionGet("/file_uploads/" + pathId(uploadId)).json());
}

/**
 * List file uploads owned by this connection.
 *
 * @param requestedSize Uploads per page, 1 to 100; `null` uses 100.
 * @param startCursor Cursor from a previous page's `nextCursor`; `null` starts at the first upload.
 * @returns One page of file uploads.
 */
export function listFileUploads(requestedSize: number | null, startCursor: string | null): PageResult<FileUpload> {
    const query = new Map<string, string>();
    putQuery(query, "start_cursor", startCursor);
    query.set("page_size", pageSize(requestedSize).toString());
    const page = listFrom(notionGet("/file_uploads", query));
    const results: FileUpload[] = [];
    for (const raw of page.results) results.push(fileUploadFrom(raw));
    return {
        results: results,
        hasMore: page.hasMore,
        nextCursor: page.nextCursor,
        isComplete: page.isComplete,
    };
}

function uploadSinglePart(sourcePath: string, size: number, options: FileUploadOptions): FileUpload {
    const upload = createUpload("single_part", options, 1);
    const path = trustedUploadPath(upload.uploadUrl);
    const bytes = readBytes(sourcePath, 0, size);
    return sendPart(path, upload.id, bytes, options, null);
}

function uploadMultiPart(sourcePath: string, size: number, options: FileUploadOptions): FileUpload {
    const chunkSize = options.chunkSize === null ? DEFAULT_CHUNK_SIZE : options.chunkSize;
    if (chunkSize < MIN_CHUNK_SIZE || chunkSize > MAX_CHUNK_SIZE) {
        throw validationError("invalid_chunk_size", "multipart chunkSize must be between 5 MiB and 20 MiB");
    }
    const partCount = Math.ceil(size / chunkSize);
    if (partCount > 1000) throw validationError("too_many_parts", "multipart uploads cannot exceed 1000 parts");
    const upload = createUpload("multi_part", options, partCount);
    const path = trustedUploadPath(upload.uploadUrl);
    let offset = 0;
    let partNumber = 1;
    while (offset < size) {
        const length = size - offset > chunkSize ? chunkSize : size - offset;
        const bytes = readBytes(sourcePath, offset, length);
        sendPart(path, upload.id, bytes, options, partNumber);
        offset += bytes.length;
        partNumber += 1;
    }
    if (upload.completeUrl.length > 0) trustedUploadPath(upload.completeUrl);
    return fileUploadFrom(notionPost("/file_uploads/" + pathId(upload.id) + "/complete", null).json());
}

function createUpload(mode: string, options: FileUploadOptions, partCount: number): FileUpload {
    const fields: string[] = [
        fieldJson("mode", mode),
        fieldJson("filename", options.filename),
        fieldJson("content_type", options.contentType),
    ];
    if (mode === "multi_part") fields.push(fieldJson("number_of_parts", partCount));
    const upload = fileUploadFrom(notionPost("/file_uploads", objectJson(fields)).json());
    if (upload.uploadUrl.length === 0) throw validationError("missing_upload_url", "Notion did not return an upload URL");
    return upload;
}

function sendPart(
    path: string,
    uploadId: string,
    bytes: Uint8Array,
    options: FileUploadOptions,
    partNumber: number | null,
): FileUpload {
    const boundary = "submilli-notion-" + uploadId.replaceAll("-", "");
    const body = multipartBody(boundary, bytes, options, partNumber);
    const headers = authHeaders();
    headers.set("Content-Type", "multipart/form-data; boundary=" + boundary);
    const response = post(API + path, body, headers);
    requireOk(response);
    return fileUploadFrom(response.json());
}

function multipartBody(
    boundary: string,
    bytes: Uint8Array,
    options: FileUploadOptions,
    partNumber: number | null,
): Uint8Array {
    let prefix = "";
    if (partNumber !== null) {
        prefix += "--" + boundary + "\r\n"
            + "Content-Disposition: form-data; name=\"part_number\"\r\n\r\n"
            + partNumber.toString() + "\r\n";
    }
    prefix += "--" + boundary + "\r\n"
        + "Content-Disposition: form-data; name=\"file\"; filename=\"" + options.filename + "\"\r\n"
        + "Content-Type: " + options.contentType + "\r\n\r\n";
    const suffix = "\r\n--" + boundary + "--\r\n";
    const prefixBytes = new TextEncoder().encode(prefix);
    const suffixBytes = new TextEncoder().encode(suffix);
    const result = Uint8Array.alloc(prefixBytes.length + bytes.length + suffixBytes.length);
    result.set(prefixBytes, 0);
    result.set(bytes, prefixBytes.length);
    result.set(suffixBytes, prefixBytes.length + bytes.length);
    return result;
}

function validateUploadOptions(options: FileUploadOptions): void {
    if (options.filename.length === 0) throw validationError("invalid_filename", "upload filename cannot be empty");
    if (options.filename.includes("\r") || options.filename.includes("\n") || options.filename.includes("\"")) {
        throw validationError("invalid_filename", "upload filename cannot contain quotes or line breaks");
    }
    if (new TextEncoder().encode(options.filename).length > 900) {
        throw validationError("invalid_filename", "upload filename cannot exceed 900 UTF-8 bytes");
    }
    if (options.contentType.length === 0 || options.contentType.includes("\r") || options.contentType.includes("\n")) {
        throw validationError("invalid_content_type", "upload contentType cannot be empty or contain line breaks");
    }
}
