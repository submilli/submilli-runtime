// Response side of the Sentry client: the result model, the raw API shapes it
// is decoded from, Link-header pagination, and HTTP error codes and messages.
// Internal: `src/lib.ts` re-exports the result types, and programs reach the
// helpers only through its operations.

/** A Sentry organization visible to the authenticated token. */
export interface Organization {
    /** Stable organization ID. */
    id: string;
    /** URL-safe organization slug used by package operations. */
    slug: string;
    /** Display name. */
    name: string;
    /** Organization status identifier. */
    status: string;
    /** ISO creation timestamp. */
    dateCreated: string;
    /** Whether the organization receives early features. */
    isEarlyAdopter: boolean;
    /** Whether the organization requires two-factor authentication. */
    require2FA: boolean;
}

/** A Sentry project. */
export interface Project {
    /** Stable project ID. */
    id: string;
    /** URL-safe project slug. */
    slug: string;
    /** Display name. */
    name: string;
    /** Primary platform, or null when unset. */
    platform: string | null;
    /** Project status identifier. */
    status: string;
    /** ISO creation timestamp. */
    dateCreated: string;
    /** Whether the token can access the project. */
    hasAccess: boolean;
    /** Whether the project is bookmarked for the token owner. */
    isBookmarked: boolean;
    /** Parent organization slug. */
    organizationSlug: string;
}

/** Project fields embedded in an issue. */
export interface ProjectSummary {
    /** Stable project ID. */
    id: string;
    /** URL-safe project slug. */
    slug: string;
    /** Display name. */
    name: string;
    /** Primary platform, or null when unset. */
    platform: string | null;
}

/** A user or team assigned to an issue. */
export interface Actor {
    /** Actor kind, normally user or team. */
    type: string;
    /** Stable actor ID. */
    id: string;
    /** Display name. */
    name: string;
    /** Email when Sentry exposes one. */
    email: string;
}

/** Type-specific issue summary fields supplied by Sentry. */
export interface IssueMetadata {
    /** Exception or issue type. */
    type: string;
    /** Primary exception or issue value. */
    value: string;
    /** Sentry-computed metadata title. */
    title: string;
}

/** A curated Sentry issue. */
export interface Issue {
    /** Numeric Sentry group ID, represented as a string. */
    id: string;
    /** Human issue reference such as API-42. */
    shortId: string;
    /** Issue title. */
    title: string;
    /** Sentry-computed culprit. */
    culprit: string;
    /** Severity level. */
    level: string;
    /** Workflow status. */
    status: string;
    /** Workflow substatus. */
    substatus: string;
    /** Issue priority. */
    priority: string;
    /** Browser URL for the issue. */
    permalink: string;
    /** Event count as Sentry's decimal string. */
    count: string;
    /** Count of affected users. */
    userCount: number;
    /** Count of issue comments. */
    numComments: number;
    /** ISO timestamp of the first event. */
    firstSeen: string;
    /** ISO timestamp of the latest event. */
    lastSeen: string;
    /** Whether the latest occurrence was unhandled. */
    isUnhandled: boolean;
    /** Project containing the issue. */
    project: ProjectSummary;
    /** Current assignee, or null when unassigned. */
    assignedTo: Actor | null;
    /** Type-specific issue summary. */
    metadata: IssueMetadata;
}

/** One indexed event tag. */
export interface Tag {
    /** Tag key. */
    key: string;
    /** Tag value. */
    value: string;
}

/** User context attached to an event. */
export interface EventUser {
    /** Application user ID. */
    id: string;
    /** User email. */
    email: string;
    /** Application username. */
    username: string;
    /** Captured IP address. */
    ipAddress: string;
    /** Display name. */
    name: string;
}

/** Exception capture mechanism. */
export interface ExceptionMechanism {
    /** SDK-specific mechanism type. */
    type: string;
    /** Whether the application handled the exception, or null when unknown. */
    handled: boolean | null;
}

/** One source line around a stack frame. */
export interface SourceLine {
    /** One-based source line number when known. */
    lineNumber: number;
    /** Source text. */
    text: string;
}

/** A curated exception stack frame. */
export interface StackFrame {
    /** Source filename. */
    filename: string;
    /** Function name. */
    functionName: string;
    /** Runtime module name. */
    module: string;
    /** Package name. */
    packageName: string;
    /** Runtime platform. */
    platform: string;
    /** Native instruction offset when supplied. */
    instructionOffset: string;
    /** Source line number, or null. */
    lineNumber: number | null;
    /** Source column number, or null. */
    columnNumber: number | null;
    /** Whether the frame belongs to application code, or null. */
    inApp: boolean | null;
    /** Sentry source link when available. */
    sourceLink: string;
    /** Source context around the frame. */
    context: SourceLine[];
}

/** One exception captured by an event. */
export interface EventException {
    /** Exception type. */
    type: string;
    /** Exception message or value. */
    value: string;
    /** Exception module. */
    module: string;
    /** Thread ID normalized to a string. */
    threadId: string;
    /** Capture mechanism, or null when absent. */
    mechanism: ExceptionMechanism | null;
    /** Stack frames in Sentry response order. */
    frames: StackFrame[];
}

/** One event breadcrumb with common HTTP metadata lifted from data. */
export interface Breadcrumb {
    /** Breadcrumb type. */
    type: string;
    /** Breadcrumb category. */
    category: string;
    /** Human-readable breadcrumb message. */
    message: string;
    /** Breadcrumb severity. */
    level: string;
    /** ISO timestamp. */
    timestamp: string;
    /** Related URL when present. */
    url: string;
    /** Related HTTP method when present. */
    method: string;
    /** Related status code normalized to a string. */
    statusCode: string;
}

/** A string key/value pair used for request metadata. */
export interface KeyValue {
    /** Pair key. */
    key: string;
    /** Pair value. */
    value: string;
}

/** Curated HTTP request metadata attached to an event. */
export interface EventRequest {
    /** Request URL. */
    url: string;
    /** HTTP method. */
    method: string;
    /** URL fragment. */
    fragment: string;
    /** Request headers. */
    headers: KeyValue[];
    /** Parsed query pairs. */
    query: KeyValue[];
}

/** Release fields embedded in an event. */
export interface ReleaseSummary {
    /** Full release version. */
    version: string;
    /** Sentry-computed short version. */
    shortVersion: string;
    /** ISO release timestamp. */
    dateReleased: string;
    /** Browser URL for the release. */
    url: string;
}

/** Event fields returned by list operations. */
export interface EventSummary {
    /** Internal event row ID. */
    id: string;
    /** Public event ID. */
    eventId: string;
    /** Parent issue group ID. */
    issueId: string;
    /** Parent project ID. */
    projectId: string;
    /** Event title. */
    title: string;
    /** Captured message. */
    message: string;
    /** Sentry-computed culprit. */
    culprit: string;
    /** Runtime platform. */
    platform: string;
    /** ISO event timestamp. */
    dateCreated: string;
    /** Indexed tags. */
    tags: Tag[];
}

/** Detailed curated event data returned by getIssueEvent. */
export interface EventDetails {
    /** Common event fields. */
    summary: EventSummary;
    /** ISO server receipt timestamp. */
    dateReceived: string;
    /** Captured exceptions. */
    exceptions: EventException[];
    /** Captured breadcrumbs. */
    breadcrumbs: Breadcrumb[];
    /** HTTP request context, or null. */
    request: EventRequest | null;
    /** Application user context, or null. */
    user: EventUser | null;
    /** Release context, or null. */
    release: ReleaseSummary | null;
}

export interface ApiStatus {
    id?: string | null;
    name?: string | null;
}

export interface ApiOrganization {
    id?: string | null;
    slug?: string | null;
    name?: string | null;
    status?: unknown;
    dateCreated?: string | null;
    isEarlyAdopter?: boolean | null;
    require2FA?: boolean | null;
}

export interface ApiOrganizationRef {
    slug?: string | null;
}

export interface ApiProject {
    id?: string | null;
    slug?: string | null;
    name?: string | null;
    platform?: string | null;
    status?: unknown;
    dateCreated?: string | null;
    hasAccess?: boolean | null;
    isBookmarked?: boolean | null;
    organization?: ApiOrganizationRef | null;
}

export interface ApiActor {
    type?: string | null;
    id?: string | null;
    name?: string | null;
    email?: string | null;
}

export interface ApiIssueMetadata {
    type?: string | null;
    value?: string | null;
    title?: string | null;
}

export interface ApiIssue {
    id?: string | null;
    shortId?: string | null;
    title?: string | null;
    culprit?: string | null;
    level?: string | null;
    status?: string | null;
    substatus?: string | null;
    priority?: string | null;
    permalink?: string | null;
    count?: string | null;
    userCount?: number | null;
    numComments?: number | null;
    firstSeen?: string | null;
    lastSeen?: string | null;
    isUnhandled?: boolean | null;
    project?: ApiProject | null;
    assignedTo?: ApiActor | null;
    metadata?: ApiIssueMetadata | null;
}

export interface ApiShortIdLookup {
    groupId?: string | null;
    group?: ApiIssue | null;
}

export interface ApiTag {
    key?: string | null;
    value?: string | null;
}

export interface ApiEventUser {
    id?: string | null;
    email?: string | null;
    username?: string | null;
    ip_address?: string | null;
    name?: string | null;
}

export interface ApiMechanism {
    type?: string | null;
    handled?: boolean | null;
}

export interface ApiFrame {
    filename?: string | null;
    function?: string | null;
    module?: string | null;
    package?: string | null;
    platform?: string | null;
    instructionOffset?: string | null;
    lineNo?: number | null;
    colNo?: number | null;
    inApp?: boolean | null;
    sourceLink?: string | null;
    context?: unknown[] | null;
    preContext?: string[] | null;
    contextLine?: string | null;
    postContext?: string[] | null;
}

export interface ApiStacktrace {
    frames?: ApiFrame[] | null;
}

export interface ApiException {
    type?: string | null;
    value?: string | null;
    module?: string | null;
    threadId?: unknown;
    mechanism?: ApiMechanism | null;
    stacktrace?: ApiStacktrace | null;
}

export interface ApiExceptionData {
    values?: ApiException[] | null;
}

export interface ApiBreadcrumbData {
    url?: string | null;
    method?: string | null;
    status_code?: unknown;
}

export interface ApiBreadcrumb {
    type?: string | null;
    category?: string | null;
    message?: string | null;
    level?: string | null;
    timestamp?: string | null;
    data?: ApiBreadcrumbData | null;
}

export interface ApiBreadcrumbsData {
    values?: ApiBreadcrumb[] | null;
}

export interface ApiRequestData {
    url?: string | null;
    method?: string | null;
    fragment?: string | null;
    headers?: unknown[] | null;
    query?: unknown[] | null;
}

export interface ApiEventEntry {
    type?: string | null;
    data?: unknown;
}

export interface ApiRelease {
    version?: string | null;
    shortVersion?: string | null;
    dateReleased?: string | null;
    url?: string | null;
}

export interface ApiEvent {
    id?: string | null;
    eventID?: string | null;
    groupID?: string | null;
    projectID?: string | null;
    title?: string | null;
    message?: string | null;
    culprit?: string | null;
    platform?: string | null;
    dateCreated?: string | null;
    dateReceived?: string | null;
    tags?: ApiTag[] | null;
    entries?: ApiEventEntry[] | null;
    user?: ApiEventUser | null;
    release?: ApiRelease | null;
}

export interface ApiErrorEnvelope {
    detail?: string | null;
    error?: string | null;
    message?: string | null;
}

/**
 * Parse Sentry's Link header and return the opaque next cursor, or "" on the last page.
 * @param link Value of Sentry's `Link` response header.
 * @returns Cursor for the `rel="next"` link, or an empty string when there is no further page.
 */
export function parseNextCursor(link: string): string {
    for (const rawPart of link.split(",")) {
        const part = rawPart.trim();
        if (part.indexOf("rel=\"next\"") < 0) continue;
        if (part.indexOf("results=\"false\"") >= 0) return "";
        const marker = "cursor=\"";
        const start = part.indexOf(marker);
        if (start < 0) return "";
        const valueStart = start + marker.length;
        const end = part.indexOf("\"", valueStart);
        return end < 0 ? "" : part.slice(valueStart, end);
    }
    return "";
}

/**
 * Normalize one JSON event response into the curated event model.
 * @param body Raw JSON of one Sentry event.
 * @returns Curated event details.
 */
export function normalizeEventJson(body: string): EventDetails {
    return eventDetailsFrom(JSON.parse(body) as ApiEvent);
}

/**
 * Map an HTTP status to the stable SentryError code used by this package.
 * @param status HTTP status code of the failed response.
 * @returns Stable code such as `bad_request`, `unauthorized`, `forbidden`, `not_found`, `conflict`, `rate_limited`, or `http_error` for any other status.
 */
export function sentryErrorCode(status: number): string {
    if (status === 400) return "bad_request";
    if (status === 401) return "unauthorized";
    if (status === 403) return "forbidden";
    if (status === 404) return "not_found";
    if (status === 409) return "conflict";
    if (status === 429) return "rate_limited";
    return "http_error";
}

/**
 * Lift a Sentry error detail when possible, falling back to the HTTP status line.
 * @param status HTTP status code of the failed response.
 * @param statusText HTTP status text, used in the fallback message.
 * @param body Raw response body; its `detail`, `error` or `message` field is used when it is a JSON object.
 * @returns Sentry's error detail when present, otherwise `Sentry request failed: HTTP <status> <statusText>`.
 */
export function sentryFailureMessage(status: number, statusText: string, body: string): string {
    const fallback = "Sentry request failed: HTTP " + status.toString() + " " + statusText;
    if (body.startsWith("{")) {
        try {
            const { detail, error, message } = JSON.parse(body) as ApiErrorEnvelope;
            // An empty string is no more useful than the status line.
            if (detail) return detail;
            if (error) return error;
            if (message) return message;
        } catch (_cause) {
            return fallback;
        }
    }
    return fallback;
}

export function organizationFrom(data: ApiOrganization): Organization {
    return {
        id: data.id ?? "",
        slug: data.slug ?? "",
        name: data.name ?? "",
        status: statusFrom(data.status),
        dateCreated: data.dateCreated ?? "",
        isEarlyAdopter: data.isEarlyAdopter === true,
        require2FA: data.require2FA === true,
    };
}

export function projectFrom(data: ApiProject, fallbackOrganizationSlug: string): Project {
    const organization = data.organization;
    return {
        id: data.id ?? "",
        slug: data.slug ?? "",
        name: data.name ?? "",
        platform: data.platform ?? null,
        status: statusFrom(data.status),
        dateCreated: data.dateCreated ?? "",
        hasAccess: data.hasAccess === true,
        isBookmarked: data.isBookmarked === true,
        organizationSlug: organization ? organization.slug ?? "" : fallbackOrganizationSlug,
    };
}

function projectSummaryFrom(data: ApiProject | null | undefined): ProjectSummary {
    if (!data) return { id: "", slug: "", name: "", platform: null };
    return { id: data.id ?? "", slug: data.slug ?? "", name: data.name ?? "", platform: data.platform ?? null };
}

function actorFrom(data: ApiActor | null | undefined): Actor | null {
    if (!data) return null;
    return { type: data.type ?? "", id: data.id ?? "", name: data.name ?? "", email: data.email ?? "" };
}

function metadataFrom(data: ApiIssueMetadata | null | undefined): IssueMetadata {
    if (!data) return { type: "", value: "", title: "" };
    return { type: data.type ?? "", value: data.value ?? "", title: data.title ?? "" };
}

export function issueFrom(data: ApiIssue): Issue {
    return {
        id: data.id ?? "",
        shortId: data.shortId ?? "",
        title: data.title ?? "",
        culprit: data.culprit ?? "",
        level: data.level ?? "",
        status: data.status ?? "",
        substatus: data.substatus ?? "",
        priority: data.priority ?? "",
        permalink: data.permalink ?? "",
        count: data.count ?? "",
        userCount: data.userCount ?? 0,
        numComments: data.numComments ?? 0,
        firstSeen: data.firstSeen ?? "",
        lastSeen: data.lastSeen ?? "",
        isUnhandled: data.isUnhandled === true,
        project: projectSummaryFrom(data.project),
        assignedTo: actorFrom(data.assignedTo),
        metadata: metadataFrom(data.metadata),
    };
}

export function eventSummaryFrom(data: ApiEvent): EventSummary {
    const tags: Tag[] = [];
    for (const tag of listed(data.tags)) tags.push({ key: tag.key ?? "", value: tag.value ?? "" });
    return {
        id: data.id ?? "",
        eventId: data.eventID ?? data.id ?? "",
        issueId: data.groupID ?? "",
        projectId: data.projectID ?? "",
        title: data.title ?? "",
        message: data.message ?? "",
        culprit: data.culprit ?? "",
        platform: data.platform ?? "",
        dateCreated: data.dateCreated ?? "",
        tags: tags,
    };
}

export function eventDetailsFrom(data: ApiEvent): EventDetails {
    let request: EventRequest | null = null;
    const exceptions: EventException[] = [];
    const breadcrumbs: Breadcrumb[] = [];
    for (const entry of listed(data.entries)) {
        // `data` is unknown until the entry type says which shape to cast it to.
        const entryData = entry.data;
        if (entryData === null || entryData === undefined) continue;
        if (entry.type === "exception") {
            for (const item of listed((entryData as ApiExceptionData).values)) exceptions.push(exceptionFrom(item));
        } else if (entry.type === "breadcrumbs") {
            for (const item of listed((entryData as ApiBreadcrumbsData).values)) breadcrumbs.push(breadcrumbFrom(item));
        } else if (entry.type === "request") {
            request = requestFrom(entryData as ApiRequestData);
        }
    }
    return {
        summary: eventSummaryFrom(data),
        dateReceived: data.dateReceived ?? "",
        exceptions: exceptions,
        breadcrumbs: breadcrumbs,
        request: request,
        user: userFrom(data.user),
        release: releaseFrom(data.release),
    };
}

function exceptionFrom(data: ApiException): EventException {
    const frames: StackFrame[] = [];
    for (const frame of listed(data.stacktrace?.frames)) frames.push(frameFrom(frame));
    let mechanism: ExceptionMechanism | null = null;
    const captured = data.mechanism;
    if (captured) mechanism = { type: captured.type ?? "", handled: captured.handled ?? null };
    return {
        type: data.type ?? "",
        value: data.value ?? "",
        module: data.module ?? "",
        threadId: scalarString(data.threadId),
        mechanism: mechanism,
        frames: frames,
    };
}

function frameFrom(data: ApiFrame): StackFrame {
    return {
        filename: data.filename ?? "",
        functionName: data.function ?? "",
        module: data.module ?? "",
        packageName: data.package ?? "",
        platform: data.platform ?? "",
        instructionOffset: data.instructionOffset ?? "",
        lineNumber: data.lineNo ?? null,
        columnNumber: data.colNo ?? null,
        inApp: data.inApp ?? null,
        sourceLink: data.sourceLink ?? "",
        context: sourceContextFrom(data),
    };
}

function sourceContextFrom(data: ApiFrame): SourceLine[] {
    const lines: SourceLine[] = [];
    const context = data.context;
    if (context) {
        for (const raw of context) {
            if (raw === null || raw === undefined) continue;
            const pair = raw as unknown[];
            if (pair.length >= 2) lines.push({ lineNumber: scalarNumber(pair[0]), text: scalarString(pair[1]) });
        }
        return lines;
    }
    const lineNumber = data.lineNo ?? 0;
    const before = listed(data.preContext);
    for (let i = 0; i < before.length; i += 1) {
        lines.push({ lineNumber: lineNumber - before.length + i, text: before[i] });
    }
    const contextLine = data.contextLine;
    if (contextLine !== null && contextLine !== undefined) lines.push({ lineNumber: lineNumber, text: contextLine });
    const after = listed(data.postContext);
    for (let i = 0; i < after.length; i += 1) {
        lines.push({ lineNumber: lineNumber + i + 1, text: after[i] });
    }
    return lines;
}

function breadcrumbFrom(data: ApiBreadcrumb): Breadcrumb {
    const details = data.data;
    return {
        type: data.type ?? "",
        category: data.category ?? "",
        message: data.message ?? "",
        level: data.level ?? "",
        timestamp: data.timestamp ?? "",
        url: details?.url ?? "",
        method: details?.method ?? "",
        statusCode: scalarString(details?.status_code),
    };
}

function requestFrom(data: ApiRequestData): EventRequest {
    return {
        url: data.url ?? "",
        method: data.method ?? "",
        fragment: data.fragment ?? "",
        headers: keyValuesFrom(data.headers),
        query: keyValuesFrom(data.query),
    };
}

function keyValuesFrom(values: unknown[] | null | undefined): KeyValue[] {
    const items: KeyValue[] = [];
    for (const raw of listed(values)) {
        if (raw === null || raw === undefined) continue;
        const pair = raw as unknown[];
        if (pair.length >= 2) items.push({ key: scalarString(pair[0]), value: scalarString(pair[1]) });
    }
    return items;
}

function userFrom(data: ApiEventUser | null | undefined): EventUser | null {
    if (!data) return null;
    return {
        id: data.id ?? "",
        email: data.email ?? "",
        username: data.username ?? "",
        ipAddress: data.ip_address ?? "",
        name: data.name ?? "",
    };
}

function releaseFrom(data: ApiRelease | null | undefined): ReleaseSummary | null {
    if (!data) return null;
    return {
        version: data.version ?? "",
        shortVersion: data.shortVersion ?? "",
        dateReleased: data.dateReleased ?? "",
        url: data.url ?? "",
    };
}

function statusFrom(value: unknown): string {
    if (typeof value === "string") return value;
    if (value === null || value === undefined) return "";
    const status = value as ApiStatus;
    return status.id ?? status.name ?? "";
}

function scalarString(value: unknown): string {
    if (typeof value === "string") return value;
    if (typeof value === "number") return value.toString();
    if (typeof value === "boolean") return value ? "true" : "false";
    return "";
}

function scalarNumber(value: unknown): number {
    if (typeof value === "number") return value;
    if (typeof value === "string") {
        const parsed = Number(value);
        return isNaN(parsed) ? 0 : parsed;
    }
    return 0;
}

function listed<T>(value: T[] | null | undefined): T[] {
    if (value) return value;
    return [];
}
