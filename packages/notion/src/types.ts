export type ResourceKind = "page" | "database" | "data_source" | "block" | "view" | "user";

export interface ResourceRef {
    kind: ResourceKind;
    ref: string;
}

export type FetchedResource = NotionPage | NotionDatabase | NotionDataSource | NotionBlock | NotionView | NotionUser;

export interface SearchResult {
    kind: ResourceKind;
    id: string;
    url: string;
    title: string;
    lastEditedTime: string;
    raw: unknown;
}

export interface PageOptions {
    pageSize?: number;
    startCursor?: string;
}

export interface PageResult<T> {
    results: T[];
    hasMore: boolean;
    nextCursor: string;
    isComplete: boolean;
}

export interface NotionParent {
    type: string;
    id: string;
}

export interface NotionPage {
    id: string;
    url: string;
    createdTime: string;
    lastEditedTime: string;
    inTrash: boolean;
    parent: NotionParent;
    properties: Map<string, unknown>;
    raw: unknown;
}

export interface DataSourceReference {
    id: string;
    name: string;
}

export interface NotionDatabase {
    id: string;
    url: string;
    title: unknown[];
    description: unknown[];
    createdTime: string;
    lastEditedTime: string;
    inTrash: boolean;
    dataSources: DataSourceReference[];
    raw: unknown;
}

export interface NotionDataSource {
    id: string;
    databaseId: string;
    url: string;
    title: unknown[];
    createdTime: string;
    lastEditedTime: string;
    inTrash: boolean;
    properties: Map<string, unknown>;
    raw: unknown;
}

export interface NotionBlock {
    id: string;
    type: string;
    createdTime: string;
    lastEditedTime: string;
    hasChildren: boolean;
    inTrash: boolean;
    raw: unknown;
}

export interface NotionUser {
    id: string;
    type: string;
    name: string;
    avatarUrl: string;
    personEmail: string;
    raw: unknown;
}

export interface NotionView {
    id: string;
    type: string;
    name: string;
    parentDatabaseId: string;
    raw: unknown;
}

export interface PageMarkdown {
    id: string;
    markdown: string;
    truncated: boolean;
    unknownBlockIds: string[];
}

export interface SearchOptions {
    query?: string;
    kind?: "page" | "data_source";
    direction?: "ascending" | "descending";
    timestamp?: "last_edited_time";
    pageSize?: number;
    startCursor?: string;
}

export interface TextProperty {
    name: string;
    type: "title" | "rich_text";
    text: string;
}

export interface NumberProperty {
    name: string;
    type: "number";
    number: number | null;
}

export interface CheckboxProperty {
    name: string;
    type: "checkbox";
    checked: boolean;
}

export interface NamedProperty {
    name: string;
    type: "select" | "status";
    value: string | null;
}

export interface NamedListProperty {
    name: string;
    type: "multi_select";
    values: string[];
}

export interface DateProperty {
    name: string;
    type: "date";
    start: string | null;
    end?: string;
    timeZone?: string;
}

export interface IdListProperty {
    name: string;
    type: "people" | "relation";
    ids: string[];
}

export interface StringProperty {
    name: string;
    type: "url" | "email" | "phone_number";
    value: string | null;
}

export interface FileReference {
    type: "external" | "file_upload";
    url?: string;
    uploadId?: string;
    name?: string;
}

export interface FilesProperty {
    name: string;
    type: "files";
    files: FileReference[];
}

export interface PropertyValue {
    name: string;
    type: "title" | "rich_text" | "number" | "checkbox" | "select" | "status" | "multi_select" | "date" | "people" | "relation" | "url" | "email" | "phone_number" | "files";
    text?: string;
    number?: number | null;
    checked?: boolean;
    value?: string | null;
    values?: string[];
    start?: string | null;
    end?: string;
    timeZone?: string;
    ids?: string[];
    files?: FileReference[];
}

export interface PropertyBag {
    values?: PropertyValue[];
    custom?: Map<string, unknown>;
}

export interface PageParent {
    type: "page_id" | "data_source_id" | "workspace";
    id?: string;
}

export interface EmptyPageContent {
    type: "none";
}

export interface MarkdownPageContent {
    type: "markdown";
    markdown: string;
}

export interface BlocksPageContent {
    type: "blocks";
    children: unknown[];
}

export interface TemplatePageContent {
    type: "template";
    template: "default" | "none" | "template_id";
    templateId?: string;
    timeZone?: string;
}

export interface PageContent {
    type: "none" | "markdown" | "blocks" | "template";
    markdown?: string;
    children?: unknown[];
    template?: "default" | "none" | "template_id";
    templateId?: string;
    timeZone?: string;
}

export interface CreatePageInput {
    parent: PageParent;
    properties?: PropertyBag;
    content?: PageContent;
    icon?: FileReference;
    cover?: FileReference;
}

export interface UpdatePageInput {
    properties?: PropertyBag;
    icon?: FileReference | null;
    cover?: FileReference | null;
    clearIcon?: boolean;
    clearCover?: boolean;
    templateId?: string;
    templateTimeZone?: string;
}

export interface MovePageInput {
    page: string;
    parent: PageParent;
}

export interface MarkdownUpdate {
    oldText: string;
    newText: string;
    replaceAll?: boolean;
}

export interface CreateDatabaseInput {
    parentPage: string;
    title: string;
    description?: string;
    isInline?: boolean;
    properties: Map<string, unknown>;
}

export interface UpdateDataSourceInput {
    title?: string;
    properties?: Map<string, unknown>;
    databaseId?: string;
}

export interface QueryDataSourceOptions {
    filter?: unknown;
    sorts?: unknown[];
    filterProperties?: string[];
    inTrash?: boolean;
    resultType?: "page" | "data_source";
    pageSize?: number;
    startCursor?: string;
}

export interface DataSourceQueryItem {
    kind: "page" | "data_source";
    id: string;
    page: NotionPage | null;
    dataSource: NotionDataSource | null;
    raw: unknown;
}

export interface DataSourceTemplate {
    id: string;
    name: string;
    isDefault: boolean;
}

export interface ListTemplateOptions {
    name?: string;
    pageSize?: number;
    startCursor?: string;
}

export interface CreateViewInput {
    databaseId: string;
    dataSourceId: string;
    name: string;
    type: "table" | "board" | "list" | "calendar" | "timeline" | "gallery" | "form" | "chart" | "map" | "dashboard";
    filter?: unknown;
    sorts?: unknown[];
    configuration?: unknown;
    position?: unknown;
}

export interface UpdateViewInput {
    name?: string;
    filter?: unknown;
    sorts?: unknown[];
    quickFilters?: unknown;
    configuration?: unknown;
    clearFilter?: boolean;
    clearSorts?: boolean;
    clearQuickFilters?: boolean;
}

export interface ViewQuery {
    id: string;
    viewId: string;
    expiresAt: string;
    totalCount: number;
    results: ResourceRef[];
    nextCursor: string;
    hasMore: boolean;
    isComplete: boolean;
}

export interface CommentTarget {
    type: "page" | "block" | "discussion";
    id: string;
    discussionParentRef?: string;
}

export interface NotionComment {
    id: string;
    discussionId: string;
    createdTime: string;
    lastEditedTime: string;
    markdown: string;
    raw: unknown;
}

export interface CreateCommentInput {
    target: CommentTarget;
    markdown: string;
    attachments?: FileReference[];
}

export interface MeetingNotesOptions {
    filter?: unknown;
    sorts?: unknown[];
    limit?: number;
}

export interface MeetingNotesResult {
    results: NotionBlock[];
    hasMore: boolean;
}

export interface AppendBlockChildrenInput {
    childrenJson: string[];
    positionJson?: string;
}

export interface FileUploadOptions {
    filename: string;
    contentType: string;
    chunkSize?: number;
}

export interface FileUpload {
    id: string;
    status: string;
    filename: string;
    contentType: string;
    contentLength: number;
    expiryTime: string;
    uploadUrl: string;
    completeUrl: string;
    raw: unknown;
}
