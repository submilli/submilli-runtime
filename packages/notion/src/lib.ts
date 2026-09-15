import discovery from "./discovery";
import pages from "./pages";
import data from "./data";
import views from "./views";
import collaboration from "./collaboration";
import blocks from "./blocks";
import uploads from "./uploads";

// Public interfaces live here so generated package declarations retain their
// fields; types.ts mirrors them for internal feature modules.
/** Kind discriminator for Notion resources returned by search and fetch. */
export type ResourceKind = "page" | "database" | "data_source" | "block" | "view" | "user";

/** Reference to one Notion resource by kind plus ID or URL. */
export interface ResourceRef {
    /** Resource kind: "page", "database", "data_source", "block", "view", or "user". */
    kind: ResourceKind;
    /** Resource ID or Notion URL. */
    ref: string;
}

/** Any resource fetch can return; narrow by shape or by the requested kind. */
export type FetchedResource = NotionPage | NotionDatabase | NotionDataSource | NotionBlock | NotionView | NotionUser;

/** One match from a workspace title search. */
export interface SearchResult {
    /** Kind of the matched resource. */
    kind: ResourceKind;
    /** UUID of the matched resource. */
    id: string;
    /** Notion URL of the matched resource. */
    url: string;
    /** Plain-text title. */
    title: string;
    /** Last edit time (ISO 8601). */
    lastEditedTime: string;
    /** Raw Notion API object, for fields not modeled here. */
    raw: unknown;
}

/** Cursor-pagination options shared by list endpoints. */
export interface PageOptions {
    /** Results per page, max 100 (default 100). */
    pageSize?: number;
    /** Cursor from a previous page's nextCursor. */
    startCursor?: string;
}

/** One page of results with cursor-pagination state. */
export interface PageResult<T> {
    /** Items in this page. */
    results: T[];
    /** True when more pages are available. */
    hasMore: boolean;
    /** Cursor for the next page; empty when exhausted. */
    nextCursor: string;
    /** False when Notion reported the response as partial (incomplete request status). */
    isComplete: boolean;
}

/** Parent container of a page or block. */
export interface NotionParent {
    /** Parent type, e.g. "page_id", "data_source_id", "block_id", or "workspace". */
    type: string;
    /** Parent UUID; empty when the parent is the workspace. */
    id: string;
}

/** A Notion page. */
export interface NotionPage {
    /** Page UUID. */
    id: string;
    /** Notion URL of the page. */
    url: string;
    /** Creation time (ISO 8601). */
    createdTime: string;
    /** Last edit time (ISO 8601). */
    lastEditedTime: string;
    /** True when the page is in the trash. */
    inTrash: boolean;
    /** Containing page, data source, block, or workspace. */
    parent: NotionParent;
    /** Raw Notion property-value objects keyed by property name. */
    properties: Map<string, unknown>;
    /** Raw Notion API page object. */
    raw: unknown;
}

/** A data source belonging to a database. */
export interface DataSourceReference {
    /** Data source UUID. */
    id: string;
    /** Data source display name. */
    name: string;
}

/** A Notion database container; holds one or more data sources. */
export interface NotionDatabase {
    /** Database UUID. */
    id: string;
    /** Notion URL of the database. */
    url: string;
    /** Title as raw Notion rich-text items. */
    title: unknown[];
    /** Description as raw Notion rich-text items. */
    description: unknown[];
    /** Creation time (ISO 8601). */
    createdTime: string;
    /** Last edit time (ISO 8601). */
    lastEditedTime: string;
    /** True when the database is in the trash. */
    inTrash: boolean;
    /** Data sources contained in this database. */
    dataSources: DataSourceReference[];
    /** Raw Notion API database object. */
    raw: unknown;
}

/** A Notion data source: the queryable table of pages inside a database. */
export interface NotionDataSource {
    /** Data source UUID. */
    id: string;
    /** UUID of the owning database. */
    databaseId: string;
    /** Notion URL of the data source. */
    url: string;
    /** Title as raw Notion rich-text items. */
    title: unknown[];
    /** Creation time (ISO 8601). */
    createdTime: string;
    /** Last edit time (ISO 8601). */
    lastEditedTime: string;
    /** True when the data source is in the trash. */
    inTrash: boolean;
    /** Raw Notion property-schema objects keyed by property name. */
    properties: Map<string, unknown>;
    /** Raw Notion API data source object. */
    raw: unknown;
}

/** A Notion content block. */
export interface NotionBlock {
    /** Block UUID. */
    id: string;
    /** Block type, e.g. "paragraph", "heading_1", "toggle". */
    type: string;
    /** Creation time (ISO 8601). */
    createdTime: string;
    /** Last edit time (ISO 8601). */
    lastEditedTime: string;
    /** True when the block has child blocks. */
    hasChildren: boolean;
    /** True when the block is in the trash. */
    inTrash: boolean;
    /** Raw Notion API block object, including the type-specific payload. */
    raw: unknown;
}

/** A Notion user. */
export interface NotionUser {
    /** User UUID. */
    id: string;
    /** User type: "person" or "bot". */
    type: string;
    /** Display name; empty when unavailable. */
    name: string;
    /** Avatar image URL; empty when unset. */
    avatarUrl: string;
    /** Email address of a person user; empty for bots or when hidden. */
    personEmail: string;
    /** Raw Notion API user object. */
    raw: unknown;
}

/** A saved database view. */
export interface NotionView {
    /** View UUID. */
    id: string;
    /** View layout type, e.g. "table", "board", "calendar". */
    type: string;
    /** View display name. */
    name: string;
    /** UUID of the database the view belongs to. */
    parentDatabaseId: string;
    /** Raw Notion API view object. */
    raw: unknown;
}

/** Page content rendered as enhanced Markdown. */
export interface PageMarkdown {
    /** Page UUID. */
    id: string;
    /** Enhanced-Markdown rendering of the page content. */
    markdown: string;
    /** True when the rendering was cut off before the end of the content. */
    truncated: boolean;
    /** IDs of blocks that could not be rendered to Markdown. */
    unknownBlockIds: string[];
}

/** Options for a workspace title search. */
export interface SearchOptions {
    /** Text matched against titles; omit to list everything visible. */
    query?: string;
    /** Restrict results to pages or to data sources. */
    kind?: "page" | "data_source";
    /** Sort direction for the timestamp sort. */
    direction?: "ascending" | "descending";
    /** Timestamp to sort by; only "last_edited_time" is supported. */
    timestamp?: "last_edited_time";
    /** Results per page, max 100 (default 100). */
    pageSize?: number;
    /** Cursor from a previous page's nextCursor. */
    startCursor?: string;
}

/** Title or rich-text property value written as plain text. */
export interface TextProperty {
    /** Property name. */
    name: string;
    /** "title" or "rich_text". */
    type: "title" | "rich_text";
    /** Plain-text content. */
    text: string;
}

/** Number property value. */
export interface NumberProperty {
    /** Property name. */
    name: string;
    /** Always "number". */
    type: "number";
    /** Numeric value; null clears the property. */
    number: number | null;
}

/** Checkbox property value. */
export interface CheckboxProperty {
    /** Property name. */
    name: string;
    /** Always "checkbox". */
    type: "checkbox";
    /** Checked state. */
    checked: boolean;
}

/** Select or status property value addressed by option name. */
export interface NamedProperty {
    /** Property name. */
    name: string;
    /** "select" or "status". */
    type: "select" | "status";
    /** Option name; null clears the selection. */
    value: string | null;
}

/** Multi-select property value addressed by option names. */
export interface NamedListProperty {
    /** Property name. */
    name: string;
    /** Always "multi_select". */
    type: "multi_select";
    /** Option names to set. */
    values: string[];
}

/** Date or date-range property value. */
export interface DateProperty {
    /** Property name. */
    name: string;
    /** Always "date". */
    type: "date";
    /** Start date or date-time (ISO 8601); null clears the property. */
    start: string | null;
    /** End date or date-time (ISO 8601) for ranges. */
    end?: string;
    /** IANA time zone name, e.g. "Europe/Paris". */
    timeZone?: string;
}

/** People or relation property value addressed by UUIDs. */
export interface IdListProperty {
    /** Property name. */
    name: string;
    /** "people" or "relation". */
    type: "people" | "relation";
    /** User UUIDs (people) or page UUIDs (relation) to set. */
    ids: string[];
}

/** URL, email, or phone-number property value. */
export interface StringProperty {
    /** Property name. */
    name: string;
    /** "url", "email", or "phone_number". */
    type: "url" | "email" | "phone_number";
    /** String value; null clears the property. */
    value: string | null;
}

/** An external URL or an uploaded file, used for files properties, icons, covers, and attachments. */
export interface FileReference {
    /** "external" for a URL, "file_upload" for a previously uploaded file. */
    type: "external" | "file_upload";
    /** External file URL; required when type is "external". */
    url?: string;
    /** File upload ID; required when type is "file_upload". */
    uploadId?: string;
    /** Display name for the file. */
    name?: string;
}

/** Files property value. */
export interface FilesProperty {
    /** Property name. */
    name: string;
    /** Always "files". */
    type: "files";
    /** Files to attach. */
    files: FileReference[];
}

/** Discriminated property value: set the value field that matches type. */
export interface PropertyValue {
    /** Property name. */
    name: string;
    /** Property type selecting which value field applies. */
    type: "title" | "rich_text" | "number" | "checkbox" | "select" | "status" | "multi_select" | "date" | "people" | "relation" | "url" | "email" | "phone_number" | "files";
    /** Plain text, for "title" and "rich_text". */
    text?: string;
    /** Numeric value, for "number"; null clears the property. */
    number?: number | null;
    /** Checked state, for "checkbox". */
    checked?: boolean;
    /** Option name, for "select" and "status"; null clears the selection. */
    value?: string | null;
    /** Option names, for "multi_select". */
    values?: string[];
    /** Start date or date-time (ISO 8601), for "date"; null clears the property. */
    start?: string | null;
    /** End date or date-time (ISO 8601), for "date" ranges. */
    end?: string;
    /** IANA time zone name, for "date". */
    timeZone?: string;
    /** User or page UUIDs, for "people" and "relation". */
    ids?: string[];
    /** Files to attach, for "files". */
    files?: FileReference[];
}

/** Property values to write; typed values and raw fallbacks may be mixed, names must be unique. */
export interface PropertyBag {
    /** Typed property values. */
    values?: PropertyValue[];
    /** Raw Notion property-value objects keyed by property name, for types not modeled here. */
    custom?: Map<string, unknown>;
}

/** Where to create or move a page. */
export interface PageParent {
    /** "page_id", "data_source_id", or "workspace". */
    type: "page_id" | "data_source_id" | "workspace";
    /** Parent page or data source ID/URL; omit for "workspace". */
    id?: string;
}

/** Content strategy: create the page empty. */
export interface EmptyPageContent {
    /** Always "none". */
    type: "none";
}

/** Content strategy: create the page from enhanced Markdown. */
export interface MarkdownPageContent {
    /** Always "markdown". */
    type: "markdown";
    /** Enhanced-Markdown body. */
    markdown: string;
}

/** Content strategy: create the page from raw Notion block objects. */
export interface BlocksPageContent {
    /** Always "blocks". */
    type: "blocks";
    /** Raw Notion block objects appended as page children. */
    children: unknown[];
}

/** Content strategy: create the page from a data source template. */
export interface TemplatePageContent {
    /** Always "template". */
    type: "template";
    /** Template choice: the data source default, none, or a specific template by ID. */
    template: "default" | "none" | "template_id";
    /** Template ID or URL; required when template is "template_id". */
    templateId?: string;
    /** IANA time zone used when instantiating template dates. */
    timeZone?: string;
}

/** Page content strategy: set the fields that match type. */
export interface PageContent {
    /** Content strategy selecting which fields apply. */
    type: "none" | "markdown" | "blocks" | "template";
    /** Enhanced-Markdown body, for "markdown". */
    markdown?: string;
    /** Raw Notion block objects, for "blocks". */
    children?: unknown[];
    /** Template choice, for "template". */
    template?: "default" | "none" | "template_id";
    /** Template ID or URL, when template is "template_id". */
    templateId?: string;
    /** IANA time zone used when instantiating template dates. */
    timeZone?: string;
}

/** Input for createPage. */
export interface CreatePageInput {
    /** Where to create the page. */
    parent: PageParent;
    /** Initial property values. */
    properties?: PropertyBag;
    /** Initial content; defaults to an empty page. */
    content?: PageContent;
    /** Page icon. */
    icon?: FileReference;
    /** Page cover image. */
    cover?: FileReference;
}

/** Input for updatePage; omitted fields are left unchanged. */
export interface UpdatePageInput {
    /** Property values to change. */
    properties?: PropertyBag;
    /** New page icon. */
    icon?: FileReference | null;
    /** New page cover image. */
    cover?: FileReference | null;
    /** True to remove the icon. */
    clearIcon?: boolean;
    /** True to remove the cover. */
    clearCover?: boolean;
    /** Data source template (ID or URL) to apply to the page. */
    templateId?: string;
    /** IANA time zone used when instantiating template dates. */
    templateTimeZone?: string;
}

/** One page move for movePages. */
export interface MovePageInput {
    /** ID or URL of the page to move. */
    page: string;
    /** New parent. */
    parent: PageParent;
}

/** Text replacement applied to a page's enhanced Markdown. */
export interface MarkdownUpdate {
    /** Existing text to find; must not be empty. */
    oldText: string;
    /** Replacement text. */
    newText: string;
    /** True to replace every match instead of only the first. */
    replaceAll?: boolean;
}

/** Input for createDatabase. */
export interface CreateDatabaseInput {
    /** Parent page ID or URL. */
    parentPage: string;
    /** Database title (plain text). */
    title: string;
    /** Database description (plain text). */
    description?: string;
    /** True to render the database inline inside the parent page. */
    isInline?: boolean;
    /** Initial schema: raw Notion property-schema objects keyed by property name. */
    properties: Map<string, unknown>;
}

/** Input for updateDataSource; omitted fields are left unchanged. */
export interface UpdateDataSourceInput {
    /** New title (plain text). */
    title?: string;
    /** Schema changes: raw Notion property-schema objects keyed by property name. */
    properties?: Map<string, unknown>;
    /** Move the data source under this database (ID or URL). */
    databaseId?: string;
}

/** Options for queryDataSource. */
export interface QueryDataSourceOptions {
    /** Raw Notion filter object. */
    filter?: unknown;
    /** Raw Notion sort objects. */
    sorts?: unknown[];
    /** Restrict returned page properties to these property IDs. */
    filterProperties?: string[];
    /** True to query items in the trash. */
    inTrash?: boolean;
    /** Restrict results to pages or to nested data sources. */
    resultType?: "page" | "data_source";
    /** Results per page, max 100 (default 100). */
    pageSize?: number;
    /** Cursor from a previous page's nextCursor. */
    startCursor?: string;
}

/** One item returned by queryDataSource. */
export interface DataSourceQueryItem {
    /** Whether the item is a page or a nested data source. */
    kind: "page" | "data_source";
    /** Item UUID. */
    id: string;
    /** Parsed page when kind is "page"; null otherwise. */
    page: NotionPage | null;
    /** Parsed data source when kind is "data_source"; null otherwise. */
    dataSource: NotionDataSource | null;
    /** Raw Notion API object. */
    raw: unknown;
}

/** A page template defined on a data source. */
export interface DataSourceTemplate {
    /** Template UUID. */
    id: string;
    /** Template display name. */
    name: string;
    /** True when this is the data source's default template. */
    isDefault: boolean;
}

/** Options for listDataSourceTemplates. */
export interface ListTemplateOptions {
    /** Filter templates by name. */
    name?: string;
    /** Results per page, max 100 (default 100). */
    pageSize?: number;
    /** Cursor from a previous page's nextCursor. */
    startCursor?: string;
}

/** Input for createView. */
export interface CreateViewInput {
    /** Database (ID or URL) that will own the view. */
    databaseId: string;
    /** Data source (ID or URL) the view queries. */
    dataSourceId: string;
    /** View display name. */
    name: string;
    /** View layout type. */
    type: "table" | "board" | "list" | "calendar" | "timeline" | "gallery" | "form" | "chart" | "map" | "dashboard";
    /** Raw Notion view filter object. */
    filter?: unknown;
    /** Raw Notion view sort objects. */
    sorts?: unknown[];
    /** Raw type-specific view configuration object. */
    configuration?: unknown;
    /** Raw position object controlling placement among sibling views. */
    position?: unknown;
}

/** Input for updateView; omitted fields are left unchanged. */
export interface UpdateViewInput {
    /** New display name. */
    name?: string;
    /** Raw Notion view filter object. */
    filter?: unknown;
    /** Raw Notion view sort objects. */
    sorts?: unknown[];
    /** Raw quick-filter configuration. */
    quickFilters?: unknown;
    /** Raw type-specific view configuration object. */
    configuration?: unknown;
    /** True to remove the saved filter. */
    clearFilter?: boolean;
    /** True to remove the saved sorts. */
    clearSorts?: boolean;
    /** True to remove the saved quick filters. */
    clearQuickFilters?: boolean;
}

/** Cached result set from executing a view's saved query. */
export interface ViewQuery {
    /** Result set ID; pass to continueViewQuery to page further. */
    id: string;
    /** UUID of the queried view. */
    viewId: string;
    /** When the cached result set expires (ISO 8601). */
    expiresAt: string;
    /** Total number of matching items. */
    totalCount: number;
    /** References to the first page of matching resources. */
    results: ResourceRef[];
    /** Cursor for the next page; empty when exhausted. */
    nextCursor: string;
    /** True when more pages are available. */
    hasMore: boolean;
    /** False when Notion reported the response as partial (incomplete request status). */
    isComplete: boolean;
}

/** Where to attach a comment. */
export interface CommentTarget {
    /** Attach to a page, to a block, or to an existing discussion thread. */
    type: "page" | "block" | "discussion";
    /** Page, block, or discussion ID (pages and blocks also accept URLs). */
    id: string;
    /** For "discussion" targets: the page or block (ID or URL) hosting the discussion; required. */
    discussionParentRef?: string;
}

/** A comment on a page or block. */
export interface NotionComment {
    /** Comment UUID. */
    id: string;
    /** UUID of the discussion thread the comment belongs to. */
    discussionId: string;
    /** Creation time (ISO 8601). */
    createdTime: string;
    /** Last edit time (ISO 8601). */
    lastEditedTime: string;
    /** Comment body as Markdown (falls back to concatenated plain text). */
    markdown: string;
    /** Raw Notion API comment object. */
    raw: unknown;
}

/** Input for createComment. */
export interface CreateCommentInput {
    /** Where to attach the comment. */
    target: CommentTarget;
    /** Comment body as Markdown; must not be empty. */
    markdown: string;
    /** Files to attach, at most three. */
    attachments?: FileReference[];
}

/** Options for queryMeetingNotes. */
export interface MeetingNotesOptions {
    /** Raw Notion filter object. */
    filter?: unknown;
    /** Raw Notion sort objects. */
    sorts?: unknown[];
    /** Maximum number of blocks to return, max 50 (default 50). */
    limit?: number;
}

/** Result of queryMeetingNotes. */
export interface MeetingNotesResult {
    /** Matching meeting-note blocks. */
    results: NotionBlock[];
    /** True when more matches exist beyond the limit. */
    hasMore: boolean;
}

/** Input for appendBlockChildren. */
export interface AppendBlockChildrenInput {
    /** Raw Notion block objects, each encoded as a JSON string. */
    childrenJson: string[];
    /** Notion position object as a JSON string, e.g. {"type":"after","block_id":"..."}; omit to append at the end. */
    positionJson?: string;
}

/** Options for uploadFile. */
export interface FileUploadOptions {
    /** File name recorded on the upload; at most 900 UTF-8 bytes, no quotes or line breaks. */
    filename: string;
    /** MIME content type of the file. */
    contentType: string;
    /** Multipart chunk size in bytes, 5-20 MiB (default 10 MiB); files of 20 MiB or less upload in one part. */
    chunkSize?: number;
}

/** A Notion file upload. */
export interface FileUpload {
    /** Upload UUID; usable as FileReference.uploadId once uploaded. */
    id: string;
    /** Upload status, e.g. "pending", "uploaded", "expired". */
    status: string;
    /** File name recorded on the upload. */
    filename: string;
    /** MIME content type. */
    contentType: string;
    /** Size in bytes; 0 until Notion reports it. */
    contentLength: number;
    /** When an unfinished upload expires (ISO 8601); empty when not applicable. */
    expiryTime: string;
    /** API URL file parts are sent to; empty once no longer applicable. */
    uploadUrl: string;
    /** API URL that completes a multipart upload; empty when not applicable. */
    completeUrl: string;
    /** Raw Notion API file upload object. */
    raw: unknown;
}

export { NotionError, BatchNotionError } from "./transport";

/** Extract and validate a Notion ID from an ID, URL, or collection reference. */
export function notionId(ref: string): string {
    return discovery.notionId(ref);
}

/** Retrieve the integration bot user.
 * @capability submilli/notion.getSelf {}
 */
export function getSelf(): NotionUser {
    return discovery.getSelf();
}

/** Search titles visible to the connection.
 * @capability submilli/notion.search {}
 */
export function search(options: SearchOptions | null = null): PageResult<SearchResult> {
    return discovery.search(options);
}

/** Fetch one resource using its explicit kind.
 * @capability submilli/notion.fetch { kind: string, id: string }
 */
export function fetch(resource: ResourceRef): FetchedResource {
    return discovery.fetch(resource);
}

/** Retrieve a page by ID or Notion URL.
 * @capability submilli/notion.fetchPage { pageId: string }
 */
export function fetchPage(ref: string): NotionPage {
    return discovery.fetchPage(ref);
}

/** Retrieve a database container by ID or Notion URL.
 * @capability submilli/notion.fetchDatabase { databaseId: string }
 */
export function fetchDatabase(ref: string): NotionDatabase {
    return discovery.fetchDatabase(ref);
}

/** Retrieve a data source by ID, URL, or collection reference.
 * @capability submilli/notion.fetchDataSource { dataSourceId: string }
 */
export function fetchDataSource(ref: string): NotionDataSource {
    return discovery.fetchDataSource(ref);
}

/** Retrieve a workspace user.
 * @capability submilli/notion.getUser { userId: string }
 */
export function getUser(ref: string): NotionUser {
    return discovery.getUser(ref);
}

/** List users visible to the connection.
 * @capability submilli/notion.listUsers {}
 */
export function listUsers(options: PageOptions | null = null): PageResult<NotionUser> {
    return discovery.listUsers(options);
}

/** Create a page with properties and one content strategy.
 * @capability submilli/notion.createPage { parentId: string }
 */
export function createPage(input: CreatePageInput): NotionPage {
    return pages.createPage(input);
}

/** Create pages sequentially and stop on the first failure.
 * @capability submilli/notion.createPages {}
 */
export function createPages(inputs: CreatePageInput[]): NotionPage[] {
    return pages.createPages(inputs);
}

/** Update page properties, icon, cover, or template.
 * @capability submilli/notion.updatePage { pageId: string }
 */
export function updatePage(ref: string, input: UpdatePageInput): NotionPage {
    return pages.updatePage(ref, input);
}

/** Retrieve a page as enhanced Markdown.
 * @capability submilli/notion.readPageMarkdown { pageId: string }
 */
export function readPageMarkdown(ref: string, includeTranscript: boolean = false): PageMarkdown {
    return pages.readPageMarkdown(ref, includeTranscript);
}

/** Replace matching enhanced Markdown content.
 * @capability submilli/notion.updatePageMarkdown { pageId: string }
 */
export function updatePageMarkdown(ref: string, update: MarkdownUpdate): PageMarkdown {
    return pages.updatePageMarkdown(ref, update);
}

/** Replace all page content with enhanced Markdown.
 * @capability submilli/notion.replacePageMarkdown { pageId: string }
 */
export function replacePageMarkdown(ref: string, markdown: string, allowDeletingContent: boolean = false): PageMarkdown {
    return pages.replacePageMarkdown(ref, markdown, allowDeletingContent);
}

/** Append enhanced Markdown to a page.
 * @capability submilli/notion.appendPageMarkdown { pageId: string }
 */
export function appendPageMarkdown(ref: string, markdown: string): PageMarkdown {
    return pages.appendPageMarkdown(ref, markdown);
}

/** Move a page under another page or data source.
 * @capability submilli/notion.movePage { pageId: string, parentId: string }
 */
export function movePage(ref: string, parent: PageParent): NotionPage {
    return pages.movePage(ref, parent);
}

/** Move pages sequentially and stop on the first failure.
 * @capability submilli/notion.movePages {}
 */
export function movePages(inputs: MovePageInput[]): NotionPage[] {
    return pages.movePages(inputs);
}

/** Retrieve one page property item.
 * @capability submilli/notion.getPageProperty { pageId: string }
 */
export function getPageProperty(pageRef: string, propertyId: string): unknown {
    return pages.getPageProperty(pageRef, propertyId);
}

/** Move a page to trash.
 * @capability submilli/notion.trashPage { pageId: string }
 */
export function trashPage(ref: string): NotionPage {
    return pages.trashPage(ref);
}

/** Restore a page from trash.
 * @capability submilli/notion.restorePage { pageId: string }
 */
export function restorePage(ref: string): NotionPage {
    return pages.restorePage(ref);
}

/** Create a database and its initial data source.
 * @capability submilli/notion.createDatabase { parentId: string }
 */
export function createDatabase(input: CreateDatabaseInput): NotionDatabase {
    return data.createDatabase(input);
}

/** Update a data source title, schema, or parent.
 * @capability submilli/notion.updateDataSource { dataSourceId: string }
 */
export function updateDataSource(ref: string, input: UpdateDataSourceInput): NotionDataSource {
    return data.updateDataSource(ref, input);
}

/** Query pages and nested data sources.
 * @capability submilli/notion.queryDataSource { dataSourceId: string }
 */
export function queryDataSource(
    ref: string,
    options: QueryDataSourceOptions | null = null,
): PageResult<DataSourceQueryItem> {
    return data.queryDataSource(ref, options);
}

/** List page templates available to a data source.
 * @capability submilli/notion.listDataSourceTemplates { dataSourceId: string }
 */
export function listDataSourceTemplates(
    ref: string,
    options: ListTemplateOptions | null = null,
): PageResult<DataSourceTemplate> {
    return data.listDataSourceTemplates(ref, options);
}

/** Move a database to trash.
 * @capability submilli/notion.trashDatabase { databaseId: string }
 */
export function trashDatabase(ref: string): NotionDatabase {
    return data.trashDatabase(ref);
}

/** Restore a database from trash.
 * @capability submilli/notion.restoreDatabase { databaseId: string }
 */
export function restoreDatabase(ref: string): NotionDatabase {
    return data.restoreDatabase(ref);
}

/** Create a configured database view.
 * @capability submilli/notion.createView { databaseId: string, dataSourceId: string }
 */
export function createView(input: CreateViewInput): NotionView {
    return views.createView(input);
}

/** Update a view's saved query or presentation.
 * @capability submilli/notion.updateView { viewId: string }
 */
export function updateView(ref: string, input: UpdateViewInput): NotionView {
    return views.updateView(ref, input);
}

/** List views belonging to a database.
 * @capability submilli/notion.listViews { databaseId: string }
 */
export function listViews(databaseRef: string, options: PageOptions | null = null): PageResult<NotionView> {
    return views.listViews(databaseRef, options);
}

/** Execute a view's saved filters and sorts.
 * @capability submilli/notion.queryView { viewId: string }
 */
export function queryView(ref: string, resultPageSize: number = 100): ViewQuery {
    return views.queryView(ref, resultPageSize);
}

/** Continue a cached view query.
 * @capability submilli/notion.continueViewQuery { viewId: string }
 */
export function continueViewQuery(
    viewRef: string,
    queryRef: string,
    startCursor: string = "",
    resultPageSize: number = 100,
): PageResult<ResourceRef> {
    return views.continueViewQuery(viewRef, queryRef, startCursor, resultPageSize);
}

/** Create a Markdown comment.
 * @capability submilli/notion.createComment { pageId: string }
 */
export function createComment(input: CreateCommentInput): NotionComment {
    return collaboration.createComment(input);
}

/** List open comments for a page or block.
 * @capability submilli/notion.getComments { pageId: string }
 */
export function getComments(ref: string, options: PageOptions | null = null): PageResult<NotionComment> {
    return collaboration.getComments(ref, options);
}

/** Query meeting-note blocks visible to the integration user.
 * @capability submilli/notion.queryMeetingNotes {}
 */
export function queryMeetingNotes(options: MeetingNotesOptions | null = null): MeetingNotesResult {
    return collaboration.queryMeetingNotes(options);
}

/** Retrieve one block.
 * @capability submilli/notion.getBlock { pageId: string }
 */
export function getBlock(ref: string): NotionBlock {
    return blocks.getBlock(ref);
}

/** List direct children of a block or page.
 * @capability submilli/notion.listBlockChildren { pageId: string }
 */
export function listBlockChildren(ref: string, options: PageOptions | null = null): PageResult<NotionBlock> {
    return blocks.listBlockChildren(ref, options);
}

/** Append block children at an optional position.
 * @capability submilli/notion.appendBlockChildren { pageId: string }
 */
export function appendBlockChildren(ref: string, input: AppendBlockChildrenInput): PageResult<NotionBlock> {
    return blocks.appendBlockChildrenJson(
        ref,
        input.childrenJson,
        input.positionJson === null ? "" : input.positionJson,
    );
}

/** Update fields on a block.
 * @capability submilli/notion.updateBlock { pageId: string }
 */
export function updateBlock(ref: string, fields: Map<string, unknown>): NotionBlock {
    return blocks.updateBlock(ref, fields);
}

/** Move a block to trash.
 * @capability submilli/notion.trashBlock { pageId: string }
 */
export function trashBlock(ref: string): NotionBlock {
    return blocks.trashBlock(ref);
}

/** Restore a block from trash.
 * @capability submilli/notion.restoreBlock { pageId: string }
 */
export function restoreBlock(ref: string): NotionBlock {
    return blocks.restoreBlock(ref);
}

/** Upload a VFS file using automatic single- or multipart mode.
 * @capability submilli/notion.uploadFile { path: string }
 */
export function uploadFile(sourcePath: string, options: FileUploadOptions): FileUpload {
    return uploads.uploadFile(sourcePath, options);
}

/** Retrieve one file upload.
 * @capability submilli/notion.getFileUpload { uploadId: string }
 */
export function getFileUpload(ref: string): FileUpload {
    return uploads.getFileUpload(ref);
}

/** List file uploads owned by this connection.
 * @capability submilli/notion.listFileUploads {}
 */
export function listFileUploads(options: PageOptions | null = null): PageResult<FileUpload> {
    return uploads.listFileUploads(options);
}
