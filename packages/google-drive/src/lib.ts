import {
    get,
    post,
    patch,
    put,
    delete,
    download,
    DownloadOptions,
    DownloadResult,
    Response,
} from "submilli:http";
import { readBytes, stat } from "submilli:fs";
import { encodeComponent, encodeQuery, parse } from "submilli:url";
import secrets from "submilli:secrets";
import { check } from "submilli:security";

const GOOGLE_API = "https://www.googleapis.com/";
const API = GOOGLE_API + "drive/v3";
const UPLOAD_API = GOOGLE_API + "upload/drive/v3";
const CHUNK_SIZE = 8388608;
// Exactly one `@` between two runs of printable ASCII, without the characters RFC 5322 reserves
// for address syntax, `()<>[]:;\,"`, so one value names one account.
const BARE_ADDRESS = /^[!#-'*+\-.\/0-9=?A-Z^_`a-z{|}~]+@[!#-'*+\-.\/0-9=?A-Z^_`a-z{|}~]+$/;
const DOMAIN_NAME = /^[A-Za-z0-9.-]+$/;
// The dots that end a fully qualified domain. The name is the same without them.
const TRAILING_DOTS = /\.+$/;
// One Drive file or folder ID, or an alias such as `root`.
const DRIVE_ID = /^[A-Za-z0-9_-]+$/;
const FILE_FIELDS = "id,name,mimeType,size,createdTime,modifiedTime,webViewLink,webContentLink,parents,trashed,starred,driveId,description";

/** A Google Drive API or file-transfer error with stable fields. */
export class DriveError extends Error {
    code: string;
    status: number;

    constructor(code: string, message: string, status: number) {
        super(message);
        this.name = "DriveError";
        this.code = code;
        this.status = status;
    }
}

/** Token pagination shared by Drive list methods. */
export interface PageOptions {
    /** Maximum items per page. */
    limit?: number;
    /** Continuation token from a previous page's `nextPageToken`. */
    pageToken?: string;
}

/** One page of values and an explicit continuation token. */
export interface Page<T> {
    /** Items on this page. */
    items: T[];
    /** Token for the next page; empty string when there are no more pages. */
    nextPageToken: string;
}

/** Curated Google Drive file metadata. */
export interface DriveFile {
    /** Stable Drive file id. */
    id: string;
    /** File name. */
    name: string;
    /** MIME type; native Google files use `application/vnd.google-apps.*`. */
    mimeType: string;
    /** Size in bytes; 0 for native Google files and folders. */
    size: number;
    /** Creation time, RFC 3339; empty string when absent. */
    createdTime: string;
    /** Last modification time, RFC 3339; empty string when absent. */
    modifiedTime: string;
    /** Browser URL for viewing the file in the Drive UI; empty string when absent. */
    webViewLink: string;
    /** Direct download URL for binary content; empty for native Google files. */
    webContentLink: string;
    /** Ids of parent folders; empty array when absent. */
    parents: string[];
    /** Whether the file is in the trash. */
    trashed: boolean;
    /** Whether the user starred the file. */
    starred: boolean;
    /** Id of the containing Shared Drive; empty string for My Drive files. */
    driveId: string;
    /** User-visible description; empty string when absent. */
    description: string;
}

interface ApiFile {
    id: string;
    name: string;
    mimeType: string;
    size?: string;
    createdTime?: string;
    modifiedTime?: string;
    webViewLink?: string;
    webContentLink?: string;
    parents?: string[];
    trashed?: boolean;
    starred?: boolean;
    driveId?: string;
    description?: string;
}

interface FileListResponse {
    files?: ApiFile[];
    nextPageToken?: string;
}

interface UploadDestinationMetadata {
    mimeType: string;
    driveId?: string;
}

/** Structured and raw Drive file-search options. */
export interface SearchFilesOptions {
    /** Match files whose name contains this substring. */
    nameContains?: string;
    /** Match files with exactly this MIME type. */
    mimeType?: string;
    /** Match files whose parent folder has this id. */
    parentId?: string;
    /** Match by trashed state; when unset, trashed files are excluded. */
    trashed?: boolean;
    /** Match by starred state. */
    starred?: boolean;
    /** Extra Drive query-language clause, AND-ed with the structured filters. */
    rawQuery?: string;
    /** Restrict the search to this Shared Drive. */
    driveId?: string;
    /** Maximum files per page, 1-1000; default 20. */
    limit?: number;
    /** Continuation token from a previous page's `nextPageToken`. */
    pageToken?: string;
    /** Drive sort order, e.g. "modifiedTime desc" or "name". */
    orderBy?: string;
}

/** Recent-file listing options. */
export interface RecentFilesOptions {
    /** Restrict the listing to this Shared Drive. */
    driveId?: string;
    /** Maximum files per page, 1-100; default 20. */
    limit?: number;
    /** Continuation token from a previous page's `nextPageToken`. */
    pageToken?: string;
}

/** Download and export options. */
export interface FileDownloadOptions {
    /** MIME type to export a native Google file as; required for `application/vnd.google-apps.*` files. */
    exportMimeType?: string;
    /** Replace the destination VFS file if it already exists. */
    overwrite?: boolean;
    /** Abort the download once this many bytes have been received. */
    maxBytes?: number;
}

/** Resumable upload metadata and Shared Drive placement. */
export interface FileUploadOptions {
    /** Name for the uploaded file. */
    name: string;
    /** MIME type of the uploaded content. */
    mimeType: string;
    /** Id of the parent folder; uploads to My Drive root when unset. */
    parentId?: string;
    /** Set when uploading into a Shared Drive. */
    driveId?: string;
}

interface FileUploadMetadata {
    name: string;
    parents?: string[];
}

interface FileCreateBody {
    name: string;
    mimeType: string;
    parents?: string[];
}

interface FileCopyBody {
    name?: string;
    parents?: string[];
}

/** Options for copying a Drive file. */
export interface FileCopyOptions {
    /** Name for the copy; defaults to the source file's name. */
    name?: string;
    /** Id of the folder to place the copy in. */
    parentId?: string;
}

/** A Drive permission. */
export interface Permission {
    /** Permission id, used to remove the permission. */
    id: string;
    /** Principal kind: "user", "group", "domain", or "anyone". */
    type: string;
    /** Granted role, e.g. "owner", "writer", "commenter", "reader". */
    role: string;
    /** Email of the user or group principal; empty string otherwise. */
    emailAddress: string;
    /** Domain of a domain principal; empty string otherwise. */
    domain: string;
    /** Human-readable principal name; empty string when absent. */
    displayName: string;
    /** For domain/anyone permissions, whether the file appears in search results. */
    allowFileDiscovery: boolean;
    /** When the permission expires, RFC 3339; empty string if it does not expire. */
    expirationTime: string;
}

interface ApiPermission {
    id: string;
    type: string;
    role: string;
    emailAddress?: string;
    domain?: string;
    displayName?: string;
    allowFileDiscovery?: boolean;
    expirationTime?: string;
}

interface PermissionListResponse {
    permissions?: ApiPermission[];
}

/** Principal and role for a new Drive permission. */
export interface ShareFileInput {
    /** Principal kind: "user", "group", "domain", or "anyone". */
    type: string;
    /** Role to grant: "reader", "commenter", or "writer". */
    role: string;
    /** Email of the user or group principal; required for type "user"/"group". */
    emailAddress?: string;
    /** Domain to share with; required for type "domain". */
    domain?: string;
    /** For domain/anyone permissions, let the file appear in search results. */
    allowFileDiscovery?: boolean;
    /** Email a user or group about the share; defaults to true. Ignored for domain and anyone. */
    sendNotificationEmail?: boolean;
}

interface PermissionCreateBody {
    type: string;
    role: string;
    emailAddress?: string;
    domain?: string;
    allowFileDiscovery?: boolean;
}

interface GoogleErrorEnvelope {
    error?: GoogleErrorBody;
}

interface GoogleErrorBody {
    message?: string;
    status?: string;
    errors?: GoogleErrorDetail[];
}

interface GoogleErrorDetail {
    reason?: string;
}

/**
 * Fetch one Drive file, returning null when absent.
  * @param fileId Drive file ID.
  * @returns The file's metadata, or `null` when it does not exist.
 * @capability submilli/google-drive.getFile { fileId: string }
 */
export function getFile(fileId: string): DriveFile | null {
    check("submilli/google-drive.getFile", { fileId: fileId });
    return fetchFile(fileId);
}

/**
 * Search Drive files with structured filters plus an optional raw Drive query.
 * Shared Drive searches set `driveId` and use `corpora=drive`.
  * @param options Optional name, MIME type, parent folder, trashed/starred filters, raw Drive query, Shared Drive, page size (1-1000, default 20), continuation token, and sort order; `null` lists non-trashed files with the defaults.
  * @returns One page of matching files; `items` is empty when nothing matches and `nextPageToken` is empty on the last page.
 * @capability submilli/google-drive.searchFiles {}
 */
export function searchFiles(options: SearchFilesOptions | null = null): Page<DriveFile> {
    const limit = options === null ? null : options.limit;
    const pageToken = options === null ? null : options.pageToken;
    const orderBy = options === null ? null : options.orderBy;
    const nameContains = options === null ? null : options.nameContains;
    const mimeType = options === null ? null : options.mimeType;
    const parentId = options === null ? null : options.parentId;
    const starred = options === null ? null : options.starred;
    const trashed = options === null ? null : options.trashed;
    const rawQuery = options === null ? null : options.rawQuery;
    const driveId = options === null ? null : options.driveId;
    const hasOptions = options !== null;
    check("submilli/google-drive.searchFiles", {});
    const query = new Map<string, string>();
    query.set("fields", "nextPageToken,files(" + FILE_FIELDS + ")");
    query.set("pageSize", bounded(limit, 20, 1, 1000).toString());
    const clauses: string[] = [];
    let includeTrashed = false;
    putQuery(query, "pageToken", pageToken);
    putQuery(query, "orderBy", orderBy);
    if (nameContains !== null) clauses.push("name contains '" + escapeQuery(nameContains) + "'");
    if (mimeType !== null) clauses.push("mimeType = '" + escapeQuery(mimeType) + "'");
    if (parentId !== null) clauses.push("'" + escapeQuery(parentId) + "' in parents");
    if (starred !== null) clauses.push("starred = " + (starred ? "true" : "false"));
    if (trashed !== null) {
        clauses.push("trashed = " + (trashed ? "true" : "false"));
        includeTrashed = true;
    }
    if (rawQuery !== null) {
        if (rawQuery.length > 0) clauses.push("(" + rawQuery + ")");
    }
    if (hasOptions) applySharedDrive(query, driveId);
    if (!includeTrashed) clauses.push("trashed = false");
    if (clauses.length > 0) query.set("q", clauses.join(" and "));
    const data = driveGet("/files", query).json() as FileListResponse;
    return filePage(data);
}

/**
 * List recently modified, non-trashed files.
  * @param options Optional Shared Drive, page size (1-100, default 20), and continuation token; `null` uses the defaults.
  * @returns One page of files ordered by last modification, newest first; `nextPageToken` is empty on the last page.
 * @capability submilli/google-drive.listRecentFiles {}
 */
export function listRecentFiles(options: RecentFilesOptions | null = null): Page<DriveFile> {
    const limit = options === null ? null : options.limit;
    const pageToken = options === null ? null : options.pageToken;
    const driveId = options === null ? null : options.driveId;
    check("submilli/google-drive.listRecentFiles", {});
    const search: SearchFilesOptions = {
        limit: bounded(limit, 20, 1, 100),
        orderBy: "modifiedTime desc",
    };
    if (pageToken !== null) search.pageToken = pageToken;
    if (driveId !== null) search.driveId = driveId;
    return searchFiles(search);
}

/**
 * Read a text file or export a Google Doc as UTF-8 text.
  * @param fileId Drive file ID of a text file (`text/*`, JSON, or XML) or a Google Doc.
  * @returns The file's text content; a Google Doc is exported as plain text. Throws `DriveError` `unsupported_mime_type` for other file types.
 * @capability submilli/google-drive.readText { fileId: string }
 */
export function readText(fileId: string): string {
    check("submilli/google-drive.readText", { fileId: fileId });
    const file = fetchFile(fileId);
    if (file === null) throw new DriveError("not_found", "Drive file was not found", 404);
    let path = "/files/" + encodeComponent(fileId);
    const query = new Map<string, string>();
    if (file.mimeType === "application/vnd.google-apps.document") {
        path += "/export";
        query.set("mimeType", "text/plain");
    } else if (file.mimeType.startsWith("text/") || file.mimeType === "application/json" || file.mimeType === "application/xml") {
        query.set("alt", "media");
    } else {
        throw new DriveError("unsupported_mime_type", "readText supports text files and Google Docs, not " + file.mimeType, 0);
    }
    return driveGet(path, query).body;
}

/**
 * Stream a Drive file or native-document export into the VFS.
  * @param fileId Drive file ID to download.
  * @param path Destination path in the session VFS.
  * @param options Optional export MIME type (required for native Google files), overwrite flag, and byte limit; `null` downloads binary content without overwriting.
  * @returns The download result for the file saved at `path`; throws `DriveError` `download_failed` on a non-2xx response.
 * @capability submilli/google-drive.downloadFile { fileId: string, path: string }
 */
export function downloadFile(fileId: string, path: string, options: FileDownloadOptions | null = null): DownloadResult {
    const exportMimeType = options === null ? null : options.exportMimeType;
    const overwrite = options === null ? null : options.overwrite;
    const maxBytes = options === null ? null : options.maxBytes;
    check("submilli/google-drive.downloadFile", { fileId: fileId, path: path });
    const file = fetchFile(fileId);
    if (file === null) throw new DriveError("not_found", "Drive file was not found", 404);
    let endpoint = "/files/" + encodeComponent(fileId);
    const query = new Map<string, string>();
    if (file.mimeType.startsWith("application/vnd.google-apps.")) {
        if (exportMimeType === null) {
            throw new DriveError("export_mime_type_required", "native Google files require exportMimeType", 0);
        }
        endpoint += "/export";
        query.set("mimeType", exportMimeType);
    } else {
        query.set("alt", "media");
    }
    const encoded = encodeQuery(query);
    const downloadOptions: DownloadOptions = { headers: authHeaders() };
    if (overwrite !== null) downloadOptions.overwrite = overwrite;
    if (maxBytes !== null) downloadOptions.maxBytes = maxBytes;
    const result = download(API + endpoint + "?" + encoded, path, downloadOptions);
    if (result.status < 200 || result.status >= 300) {
        throw new DriveError("download_failed", "Drive download failed with HTTP " + result.status.toString(), result.status);
    }
    return result;
}

/**
 * Upload a VFS file using Google's resumable protocol and fixed 8 MiB chunks.
 * `parentId` in the check is the destination folder as the caller named it, or "" when the caller
 * names none and the file goes to My Drive root. The alias `root` names that folder too, so a rule
 * on `parentId` lists the folders it allows.
  * @param sourcePath Path of the VFS file to upload.
  * @param options Name and MIME type for the uploaded file, with optional parent folder ID and Shared Drive ID.
  * @returns The metadata of the created Drive file.
 * @capability submilli/google-drive.uploadFile { path: string, parentId: string, driveId: string }
 */
export function uploadFile(sourcePath: string, options: FileUploadOptions): DriveFile {
    const { name, mimeType, parentId: requestedParentId, driveId } = options;
    const parentId = destinationFolderId(requestedParentId);
    const destinationDrive = uploadDestinationDrive(parentId, driveId);
    check("submilli/google-drive.uploadFile", { path: sourcePath, parentId: parentId, driveId: destinationDrive });
    const source = stat(sourcePath);
    if (source === null) throw new DriveError("source_not_found", "upload source is not a VFS file", 0);
    if (source.kind !== "file") throw new DriveError("source_not_found", "upload source is not a VFS file", 0);
    const sourceSize = source.size;
    const metadata: FileUploadMetadata = { name: name };
    if (parentId.length > 0) metadata.parents = [parentId];
    const query = new Map<string, string>();
    query.set("uploadType", "resumable");
    query.set("fields", FILE_FIELDS);
    query.set("supportsAllDrives", "true");
    const headers = authHeaders();
    headers.set("X-Upload-Content-Type", mimeType);
    headers.set("X-Upload-Content-Length", sourceSize.toString());
    const sessionResponse = post(UPLOAD_API + "/files?" + encodeQuery(query), metadata, headers);
    requireOk(sessionResponse);
    const location = sessionResponse.headers.get("location");
    if (location === null) throw new DriveError("missing_upload_location", "Drive did not return a resumable upload location", sessionResponse.status);
    const uploadLocation = validateUploadLocation(location);
    if (sourceSize === 0) {
        const emptyHeaders = authHeaders();
        emptyHeaders.set("Content-Type", mimeType);
        emptyHeaders.set("Content-Length", "0");
        const response = put(GOOGLE_API + uploadLocation, new Uint8Array([]), emptyHeaders);
        requireOk(response);
        return fileFrom(response.json() as ApiFile);
    }
    let offset = 0;
    while (offset < sourceSize) {
        const length = sourceSize - offset > CHUNK_SIZE ? CHUNK_SIZE : sourceSize - offset;
        const chunk = readBytes(sourcePath, offset, length);
        const end = offset + chunk.length - 1;
        const chunkHeaders = authHeaders();
        chunkHeaders.set("Content-Type", mimeType);
        chunkHeaders.set("Content-Length", chunk.length.toString());
        chunkHeaders.set("Content-Range", "bytes " + offset.toString() + "-" + end.toString() + "/" + sourceSize.toString());
        const response = put(GOOGLE_API + uploadLocation, chunk, chunkHeaders);
        if (response.status === 308) {
            offset += chunk.length;
        } else {
            requireOk(response);
            return fileFrom(response.json() as ApiFile);
        }
    }
    throw new DriveError("upload_incomplete", "Drive did not finalize the resumable upload", 0);
}

/**
 * Create a folder, optionally under a parent.
  * @param name Folder name.
  * @param parentId ID of the parent folder; the empty string creates the folder in My Drive root.
  * @returns The created folder's metadata.
 * @capability submilli/google-drive.createFolder { parentId: string }
 */
export function createFolder(name: string, parentId: string = ""): DriveFile {
    const folderId = destinationFolderId(parentId);
    check("submilli/google-drive.createFolder", { parentId: folderId });
    const body: FileCreateBody = {
        name: name,
        mimeType: "application/vnd.google-apps.folder",
    };
    if (folderId.length > 0) body.parents = [folderId];
    const query = new Map<string, string>();
    query.set("fields", FILE_FIELDS);
    query.set("supportsAllDrives", "true");
    const response = post(API + "/files?" + encodeQuery(query), body, authHeaders());
    requireOk(response);
    return fileFrom(response.json() as ApiFile);
}

/**
 * Copy a file with an optional new name or parent. `parentId` in the check is the destination
 * folder, or "" when the copy stays beside the source.
  * @param fileId ID of the file to copy.
  * @param options Optional name for the copy and destination folder ID; `null` copies with the source's name beside the source.
  * @returns The metadata of the new copy.
 * @capability submilli/google-drive.copyFile { fileId: string, parentId: string }
 */
export function copyFile(fileId: string, options: FileCopyOptions | null = null): DriveFile {
    const name = options === null ? null : options.name;
    const parentId = destinationFolderId(options === null ? null : options.parentId);
    check("submilli/google-drive.copyFile", { fileId: fileId, parentId: parentId });
    const body: FileCopyBody = {};
    if (name !== null) body.name = name;
    if (parentId.length > 0) body.parents = [parentId];
    const query = mutationQuery();
    const response = post(API + "/files/" + encodeComponent(fileId) + "/copy?" + encodeQuery(query), body, authHeaders());
    requireOk(response);
    return fileFrom(response.json() as ApiFile);
}

/**
 * Rename a file.
  * @param fileId ID of the file to rename.
  * @param name New file name.
  * @returns The file's metadata after the rename.
 * @capability submilli/google-drive.renameFile { fileId: string }
 */
export function renameFile(fileId: string, name: string): DriveFile {
    check("submilli/google-drive.renameFile", { fileId: fileId });
    return patchFile(fileId, { name: name });
}

/**
 * Move a file to one parent, removing its current parents.
  * @param fileId ID of the file to move.
  * @param parentId ID of the destination folder; must be non-empty.
  * @returns The file's metadata after the move.
 * @capability submilli/google-drive.moveFile { fileId: string, parentId: string }
 */
export function moveFile(fileId: string, parentId: string): DriveFile {
    const folderId = destinationFolderId(parentId);
    if (folderId.length === 0) throw new DriveError("invalid_parent", "moveFile requires a parent folder ID", 0);
    check("submilli/google-drive.moveFile", { fileId: fileId, parentId: folderId });
    const current = fetchFile(fileId);
    if (current === null) throw new DriveError("not_found", "Drive file was not found", 404);
    const query = mutationQuery();
    query.set("addParents", folderId);
    if (current.parents.length > 0) query.set("removeParents", current.parents.join(","));
    const response = patch(API + "/files/" + encodeComponent(fileId) + "?" + encodeQuery(query), {}, authHeaders());
    requireOk(response);
    return fileFrom(response.json() as ApiFile);
}

/**
 * Move a file to trash. This package intentionally has no permanent-delete API.
  * @param fileId ID of the file to trash.
  * @returns The file's metadata, with `trashed` set to true.
 * @capability submilli/google-drive.trashFile { fileId: string }
 */
export function trashFile(fileId: string): DriveFile {
    check("submilli/google-drive.trashFile", { fileId: fileId });
    return patchFile(fileId, { trashed: true });
}

/**
 * Restore a trashed file.
  * @param fileId ID of the file to restore from the trash.
  * @returns The file's metadata, with `trashed` set to false.
 * @capability submilli/google-drive.restoreFile { fileId: string }
 */
export function restoreFile(fileId: string): DriveFile {
    check("submilli/google-drive.restoreFile", { fileId: fileId });
    return patchFile(fileId, { trashed: false });
}

/**
 * List permissions on a Drive file.
  * @param fileId ID of the file whose permissions are listed.
  * @returns The file's permissions; an empty array when none are returned.
 * @capability submilli/google-drive.listPermissions { fileId: string }
 */
export function listPermissions(fileId: string): Permission[] {
    check("submilli/google-drive.listPermissions", { fileId: fileId });
    const query = new Map<string, string>();
    query.set("fields", "permissions(id,type,role,emailAddress,domain,displayName,allowFileDiscovery,expirationTime)");
    query.set("supportsAllDrives", "true");
    const data = driveGet("/files/" + encodeComponent(fileId) + "/permissions", query).json() as PermissionListResponse;
    const permissions: Permission[] = [];
    if (data.permissions !== null) for (const item of data.permissions) permissions.push(permissionFrom(item));
    return permissions;
}

/**
 * Share a file with exactly one user, group, domain, or anyone principal. `principal` is the
 * lowercased email address for "user" and "group", the lowercased domain for "domain", and
 * "anyone" for "anyone". An input that sets `emailAddress`, `domain`, or `allowFileDiscovery`
 * for a type that does not use it is refused. `sendNotificationEmail` is true for a user or
 * group unless set to false; for the other types, which Drive does not email, it is ignored and
 * false. `allowFileDiscovery` is false when unset.
  * @param fileId ID of the file to share.
  * @param input Principal type, role (`reader`, `commenter`, or `writer`), the matching email address or domain, and optional discovery and notification flags.
  * @returns The created permission, including its `id` for `removePermission`.
 * @capability submilli/google-drive.shareFile { fileId: string, principal: string, type: string, role: string, sendNotificationEmail: boolean, allowFileDiscovery: boolean }
 */
export function shareFile(fileId: string, input: ShareFileInput): Permission {
    const { type: principalType, role, emailAddress, domain, allowFileDiscovery, sendNotificationEmail } = input;
    const principal = sharePrincipal(principalType, emailAddress, domain);
    const byEmail = takesEmailAddress(principalType);
    if (role !== "reader" && role !== "commenter" && role !== "writer") {
        throw new DriveError("invalid_permission_role", "permission role must be reader, commenter, or writer", 0);
    }
    if (byEmail && allowFileDiscovery !== null) {
        throw new DriveError("invalid_permission_principal", principalType + " permission does not take allowFileDiscovery", 0);
    }
    const discoverable = allowFileDiscovery === true;
    // Drive emails only a user or a group, so no other share is reported or sent as notifying.
    const notify = byEmail && sendNotificationEmail !== false;
    check("submilli/google-drive.shareFile", {
        fileId: fileId, principal: principal, type: principalType, role: role,
        sendNotificationEmail: notify, allowFileDiscovery: discoverable,
    });
    const body: PermissionCreateBody = { type: principalType, role: role };
    const query = new Map<string, string>();
    query.set("fields", "id,type,role,emailAddress,domain,displayName,allowFileDiscovery,expirationTime");
    query.set("supportsAllDrives", "true");
    if (byEmail) {
        body.emailAddress = principal;
        query.set("sendNotificationEmail", notify ? "true" : "false");
    } else {
        body.allowFileDiscovery = discoverable;
    }
    if (principalType === "domain") body.domain = principal;
    const response = post(API + "/files/" + encodeComponent(fileId) + "/permissions?" + encodeQuery(query), body, authHeaders());
    requireOk(response);
    return permissionFrom(response.json() as ApiPermission);
}

// The one principal a permission of this type names, in the one spelling policy reads. A field
// the type does not use is refused rather than dropped: sent beside the checked principal, it
// would name a second one.
function sharePrincipal(principalType: string, emailAddress: string | null, domain: string | null): string {
    if (takesEmailAddress(principalType)) {
        if (domain !== null) throw invalidPrincipal(principalType + " permission does not take a domain");
        if (emailAddress === null || !BARE_ADDRESS.test(emailAddress)) {
            throw invalidPrincipal(principalType + " permission requires emailAddress, one bare address such as dana@example.com");
        }
        const address = oneSpelling(emailAddress);
        if (address.endsWith("@")) throw invalidPrincipal(principalType + " permission requires emailAddress with a domain");
        return address;
    }
    if (principalType === "domain") {
        if (emailAddress !== null) throw invalidPrincipal("domain permission does not take emailAddress");
        if (domain === null || !DOMAIN_NAME.test(domain)) throw invalidPrincipal("domain permission requires domain, such as example.com");
        const name = oneSpelling(domain);
        if (name.length === 0) throw invalidPrincipal("domain permission requires domain, such as example.com");
        return name;
    }
    if (principalType === "anyone") {
        if (emailAddress !== null || domain !== null) throw invalidPrincipal("anyone permission takes neither emailAddress nor domain");
        return "anyone";
    }
    throw new DriveError("invalid_permission_type", "permission type must be user, group, domain, or anyone", 0);
}

function takesEmailAddress(principalType: string): boolean {
    return principalType === "user" || principalType === "group";
}

// A name is the same whatever its case and whether or not a dot ends its domain, so a rule naming
// one spelling would let another past.
function oneSpelling(name: string): string {
    return name.toLowerCase().replace(TRAILING_DOTS, "");
}

function invalidPrincipal(message: string): DriveError {
    return new DriveError("invalid_permission_principal", message, 0);
}

/**
 * Remove a permission from a Drive file.
  * @param fileId ID of the file whose permission is removed.
  * @param permissionId Permission ID from `Permission.id`; a permission that is already absent is not an error.
 * @capability submilli/google-drive.removePermission { fileId: string }
 */
export function removePermission(fileId: string, permissionId: string): void {
    check("submilli/google-drive.removePermission", { fileId: fileId });
    const query = new Map<string, string>();
    query.set("supportsAllDrives", "true");
    const response = delete(
        API + "/files/" + encodeComponent(fileId) + "/permissions/" + encodeComponent(permissionId) + "?" + encodeQuery(query),
        authHeaders(),
    );
    if (response.status !== 404) requireOk(response);
}

// The folder a file is placed in: one Drive ID, or "" when the caller names none. Drive reads
// `addParents` as a comma-separated list, so anything but one ID could name a folder the check
// never saw.
function destinationFolderId(parentId: string | null): string {
    if (parentId === null || parentId.length === 0) return "";
    if (!DRIVE_ID.test(parentId)) throw new DriveError("invalid_parent", "parent must be one Drive folder ID", 0);
    return parentId;
}

function uploadDestinationDrive(parentId: string, requestedDriveId: string | null): string | null {
    const query = new Map<string, string>();
    query.set("fields", "mimeType,driveId");
    query.set("supportsAllDrives", "true");
    const folderId = parentId.length === 0 ? "root" : parentId;
    const response = driveRawGet("/files/" + encodeComponent(folderId), query);
    if (response.status === 404) throw new DriveError("invalid_parent", "upload destination folder was not found", 404);
    requireOk(response);
    const folder = response.json() as UploadDestinationMetadata;
    if (folder.mimeType !== "application/vnd.google-apps.folder") {
        throw new DriveError("invalid_parent", "upload parent must be a folder", 0);
    }
    const driveId = folder.driveId ?? null;
    if (requestedDriveId !== null && requestedDriveId !== driveId) {
        throw new DriveError("invalid_drive", "driveId must match the destination folder's shared drive", 0);
    }
    return driveId;
}

function fetchFile(fileId: string): DriveFile | null {
    const query = new Map<string, string>();
    query.set("fields", FILE_FIELDS);
    query.set("supportsAllDrives", "true");
    const response = driveRawGet("/files/" + encodeComponent(fileId), query);
    if (response.status === 404) return null;
    requireOk(response);
    return fileFrom(response.json() as ApiFile);
}

function patchFile(fileId: string, body: {}): DriveFile {
    const query = mutationQuery();
    const response = patch(API + "/files/" + encodeComponent(fileId) + "?" + encodeQuery(query), body, authHeaders());
    requireOk(response);
    return fileFrom(response.json() as ApiFile);
}

function mutationQuery(): Map<string, string> {
    const query = new Map<string, string>();
    query.set("fields", FILE_FIELDS);
    query.set("supportsAllDrives", "true");
    return query;
}

function filePage(data: FileListResponse): Page<DriveFile> {
    const items: DriveFile[] = [];
    if (data.files !== null) for (const file of data.files) items.push(fileFrom(file));
    return { items: items, nextPageToken: str(data.nextPageToken) };
}

function fileFrom(file: ApiFile): DriveFile {
    return {
        id: file.id,
        name: file.name,
        mimeType: file.mimeType,
        size: file.size !== null ? Number(file.size) : 0,
        createdTime: str(file.createdTime),
        modifiedTime: str(file.modifiedTime),
        webViewLink: str(file.webViewLink),
        webContentLink: str(file.webContentLink),
        parents: file.parents !== null ? file.parents : [],
        trashed: file.trashed === true,
        starred: file.starred === true,
        driveId: str(file.driveId),
        description: str(file.description),
    };
}

function permissionFrom(item: ApiPermission): Permission {
    return {
        id: item.id,
        type: item.type,
        role: item.role,
        emailAddress: str(item.emailAddress),
        domain: str(item.domain),
        displayName: str(item.displayName),
        allowFileDiscovery: item.allowFileDiscovery === true,
        expirationTime: str(item.expirationTime),
    };
}

function applySharedDrive(query: Map<string, string>, driveId: string | null): void {
    query.set("includeItemsFromAllDrives", "true");
    query.set("supportsAllDrives", "true");
    if (driveId !== null) {
        query.set("corpora", "drive");
        query.set("driveId", driveId);
    }
}

function driveGet(path: string, query: Map<string, string>): Response {
    return requireOk(driveRawGet(path, query));
}

function driveRawGet(path: string, query: Map<string, string>): Response {
    const encoded = encodeQuery(query);
    return get(API + path + (encoded.length > 0 ? "?" + encoded : ""), authHeaders());
}

function authHeaders(): Map<string, string> {
    const token = secrets.get("GOOGLE_ACCESS_TOKEN");
    if (token === null) throw new DriveError("missing_token", "GOOGLE_ACCESS_TOKEN is not bound", 0);
    const headers = new Map<string, string>();
    headers.set("Authorization", "Bearer " + token);
    return headers;
}

function requireOk(response: Response): Response {
    if (response.ok) return response;
    let code = response.status === 401 ? "unauthorized" : "http_error";
    let message = "Google Drive request failed: HTTP " + response.status.toString() + " " + response.statusText;
    if (response.body.startsWith("{")) {
        const envelope = response.json() as GoogleErrorEnvelope;
        const body = envelope.error;
        if (body !== null) {
            const errors = body.errors;
            const bodyStatus = body.status;
            const bodyMessage = body.message;
            if (errors !== null) {
                if (errors.length > 0) {
                    const reason = errors[0].reason;
                    if (reason !== null) code = reason;
                }
            } else {
                if (bodyStatus !== null) code = bodyStatus;
            }
            if (bodyMessage !== null) message = bodyMessage;
        }
    }
    throw new DriveError(code, message, response.status);
}

function validateUploadLocation(location: string): string {
    const url = parse(location);
    if (url.protocol !== "https" || url.host !== "www.googleapis.com" || url.port !== null || !location.startsWith(GOOGLE_API)) {
        throw new DriveError("invalid_upload_location", "Drive returned an untrusted resumable upload location", 0);
    }
    return location.slice(GOOGLE_API.length);
}

function escapeQuery(value: string): string {
    return value.replaceAll("\\", "\\\\").replaceAll("'", "\\'");
}

function putQuery(query: Map<string, string>, name: string, value: string | null): void {
    if (value !== null) query.set(name, value);
}

function bounded(value: number | null, fallback: number, min: number, max: number): number {
    const actual = value === null ? fallback : value;
    if (actual < min) return min;
    if (actual > max) return max;
    return actual;
}

function str(value: string | null): string {
    return value === null ? "" : value;
}
