import { get, put, Response } from "submilli:http";
import { encodeComponent } from "submilli:url";
import secrets from "submilli:secrets";
import { check } from "submilli:security";

const API = "https://sentry.io/api/0";
const DEFAULT_LIMIT = 50;
const MAX_LIMIT = 100;

/** Cursor pagination accepted by organization and project list operations. */
export interface PageOptions {
    /** Number of results, from 1 through 100. Defaults to 50. */
    limit?: number;
    /** Opaque cursor returned by a previous page. */
    cursor?: string;
}

/** One page of curated results. */
export interface PageResult<T> {
    /** Results in this page. */
    items: T[];
    /** Opaque cursor for the next page, or an empty string on the last page. */
    nextCursor: string;
    /** Whether Sentry reports that there is no next page. */
    isComplete: boolean;
}

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

/** Supported Sentry issue sort orders. */
export type IssueSort = "date" | "freq" | "inbox" | "new" | "recommended" | "trends" | "user";

/** Filters and cursor pagination for listIssues. */
export interface ListIssuesOptions {
    /** Number of results, from 1 through 100. Defaults to 50. */
    limit?: number;
    /** Opaque cursor returned by a previous page. */
    cursor?: string;
    /** Repeated project ID filters. */
    projects?: string[];
    /** Repeated environment filters. */
    environments?: string[];
    /** Sentry search query; an explicit empty string includes all issues. */
    query?: string;
    /** Relative time period such as 24h. */
    statsPeriod?: string;
    /** ISO range start; must be paired with end. */
    start?: string;
    /** ISO range end; must be paired with start. */
    end?: string;
    /** Statistics bucket period. */
    groupStatsPeriod?: string;
    /** Enable Sentry's short-ID query lookup behavior. */
    shortIdLookup?: boolean;
    /** Result sort order. */
    sort?: IssueSort;
}

/** Filters and cursor pagination for listIssueEvents. */
export interface ListIssueEventsOptions {
    /** Number of results, from 1 through 100. Defaults to 50. */
    limit?: number;
    /** Opaque cursor returned by a previous page. */
    cursor?: string;
    /** Repeated environment filters. */
    environments?: string[];
    /** Sentry event search query. */
    query?: string;
    /** Relative time period such as 24h. */
    statsPeriod?: string;
    /** ISO range start; must be paired with end. */
    start?: string;
    /** ISO range end; must be paired with start. */
    end?: string;
    /** Ask Sentry to include full event bodies in list results. */
    full?: boolean;
    /** Return events in Sentry's deterministic pseudo-random order. */
    sample?: boolean;
}

/** Issue statuses supported by the core triage surface. */
export type IssueStatus = "resolved" | "resolvedInNextRelease" | "unresolved" | "ignored";
/** Issue priorities supported by Sentry. */
export type IssuePriority = "low" | "medium" | "high";
/** Issue substatuses documented by Sentry's update endpoint. */
export type IssueSubstatus =
    "archived_until_escalating" |
    "archived_until_condition_met" |
    "archived_forever" |
    "escalating" |
    "ongoing" |
    "regressed" |
    "new";

/** Partial core triage changes for updateIssue. */
export interface UpdateIssueInput {
    /** New workflow status. */
    status?: IssueStatus;
    /** New substatus, supplied together with a compatible status. */
    substatus?: IssueSubstatus;
    /** User or team assignment accepted by Sentry's assignedTo field. */
    assignedTo?: string;
    /** Explicitly clear the current assignee. */
    clearAssignee?: boolean;
    /** New issue priority. */
    priority?: IssuePriority;
}

/** A Sentry API or local validation error with request and rate-limit metadata. */
export class SentryError extends Error {
    code: string;
    status: number;
    requestId: string;
    retryAfterSeconds: number | null;
    rateLimit: number | null;
    rateLimitRemaining: number | null;
    rateLimitReset: number | null;
    concurrentLimit: number | null;
    concurrentRemaining: number | null;

    constructor(
        code: string,
        message: string,
        status: number,
        requestId: string,
        retryAfterSeconds: number | null,
        rateLimit: number | null,
        rateLimitRemaining: number | null,
        rateLimitReset: number | null,
        concurrentLimit: number | null,
        concurrentRemaining: number | null,
    ) {
        super(message);
        this.name = "SentryError";
        this.code = code;
        this.status = status;
        this.requestId = requestId;
        this.retryAfterSeconds = retryAfterSeconds;
        this.rateLimit = rateLimit;
        this.rateLimitRemaining = rateLimitRemaining;
        this.rateLimitReset = rateLimitReset;
        this.concurrentLimit = concurrentLimit;
        this.concurrentRemaining = concurrentRemaining;
    }
}

interface ApiStatus {
    id?: string | null;
    name?: string | null;
}

interface ApiOrganization {
    id?: string | null;
    slug?: string | null;
    name?: string | null;
    status?: unknown;
    dateCreated?: string | null;
    isEarlyAdopter?: boolean | null;
    require2FA?: boolean | null;
}

interface ApiOrganizationRef {
    slug?: string | null;
}

interface ApiProject {
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

interface ApiActor {
    type?: string | null;
    id?: string | null;
    name?: string | null;
    email?: string | null;
}

interface ApiIssueMetadata {
    type?: string | null;
    value?: string | null;
    title?: string | null;
}

interface ApiIssue {
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

interface ApiShortIdLookup {
    groupId?: string | null;
    group?: ApiIssue | null;
}

interface ApiTag {
    key?: string | null;
    value?: string | null;
}

interface ApiEventUser {
    id?: string | null;
    email?: string | null;
    username?: string | null;
    ip_address?: string | null;
    name?: string | null;
}

interface ApiMechanism {
    type?: string | null;
    handled?: boolean | null;
}

interface ApiFrame {
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

interface ApiStacktrace {
    frames?: ApiFrame[] | null;
}

interface ApiException {
    type?: string | null;
    value?: string | null;
    module?: string | null;
    threadId?: unknown;
    mechanism?: ApiMechanism | null;
    stacktrace?: ApiStacktrace | null;
}

interface ApiExceptionData {
    values?: ApiException[] | null;
}

interface ApiBreadcrumbData {
    url?: string | null;
    method?: string | null;
    status_code?: unknown;
}

interface ApiBreadcrumb {
    type?: string | null;
    category?: string | null;
    message?: string | null;
    level?: string | null;
    timestamp?: string | null;
    data?: ApiBreadcrumbData | null;
}

interface ApiBreadcrumbsData {
    values?: ApiBreadcrumb[] | null;
}

interface ApiRequestData {
    url?: string | null;
    method?: string | null;
    fragment?: string | null;
    headers?: unknown[] | null;
    query?: unknown[] | null;
}

interface ApiEventEntry {
    type?: string | null;
    data?: unknown;
}

interface ApiRelease {
    version?: string | null;
    shortVersion?: string | null;
    dateReleased?: string | null;
    url?: string | null;
}

interface ApiEvent {
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

interface ApiErrorEnvelope {
    detail?: string | null;
    error?: string | null;
    message?: string | null;
}

/** List organizations visible to the token.
 * @param page Optional `limit` (1–100, default 50) and `cursor`; omit it to request the first page of 50.
 * @returns One page of organizations visible to the token, with the cursor for the next page.
 * @capability sentry.io/organizations.list {}
 */
export function listOrganizations(page: PageOptions = {}): PageResult<Organization> {
    const { limit, cursor } = page;
    check("sentry.io/organizations.list", {});
    const response = sentryGet("/organizations/" + buildPageQuery(limit, cursor));
    const items: Organization[] = [];
    for (const item of response.json() as ApiOrganization[]) items.push(organizationFrom(item));
    return pageFrom(response, items);
}

/** List projects in an organization.
 * @param organization Organization slug, as returned by `listOrganizations`.
 * @param page Optional `limit` (1–100, default 50) and `cursor`; omit it to request the first page of 50.
 * @returns One page of the organization's projects, with the cursor for the next page.
 * @capability sentry.io/projects.list { organization: string }
 */
export function listProjects(organization: string, page: PageOptions = {}): PageResult<Project> {
    const slug = requireText(organization, "organization");
    const { limit, cursor } = page;
    check("sentry.io/projects.list", { organization: slug });
    const response = sentryGet("/organizations/" + encodeComponent(slug) + "/projects/" + buildPageQuery(limit, cursor));
    const items: Project[] = [];
    for (const item of response.json() as ApiProject[]) items.push(projectFrom(item, slug));
    return pageFrom(response, items);
}

/** List issues in an organization. Omitting query keeps Sentry's unresolved default; query "" lists all.
 * @param organization Organization slug, as returned by `listOrganizations`.
 * @param options Optional filters, sort order and pagination; omit it for Sentry's default unresolved issues, first 50.
 * @returns One page of issues, with the cursor for the next page; empty `items` when nothing matches.
 * @capability sentry.io/issues.list { organization: string, projects: string[] }
 */
export function listIssues(organization: string, options: ListIssuesOptions = {}): PageResult<Issue> {
    const slug = requireText(organization, "organization");
    const {
        limit,
        cursor,
        projects: requestedProjects,
        environments: requestedEnvironments,
        query,
        statsPeriod,
        start,
        end,
        groupStatsPeriod,
        shortIdLookup,
        sort,
    } = options;
    const projects: string[] = [];
    if (requestedProjects !== undefined) {
        for (const project of requestedProjects) projects.push(project);
    }
    const environments: string[] = [];
    if (requestedEnvironments !== undefined) {
        for (const environment of requestedEnvironments) environments.push(environment);
    }
    const issueOptions: ListIssuesOptions = {
        limit: limit,
        cursor: cursor,
        projects: projects,
        environments: environments,
        query: query,
        statsPeriod: statsPeriod,
        start: start,
        end: end,
        groupStatsPeriod: groupStatsPeriod,
        shortIdLookup: shortIdLookup,
        sort: sort,
    };
    check("sentry.io/issues.list", { organization: slug, projects: projects });
    const response = sentryGet("/organizations/" + encodeComponent(slug) + "/issues/" + buildIssueQuery(issueOptions));
    const items: Issue[] = [];
    for (const item of response.json() as ApiIssue[]) items.push(issueFrom(item));
    return pageFrom(response, items);
}

/** Retrieve an issue by numeric ID or short ID; null when absent.
 * @param organization Organization slug, as returned by `listOrganizations`.
 * @param project Project slug, as returned by `listProjects`.
 * @param issueId Numeric group ID or short ID such as `PROJECT-7E`.
 * @returns The issue, or `null` when it does not exist. Throws `project_mismatch` if it belongs to a different project.
 * @capability sentry.io/issues.get { organization: string, project: string, issue: string }
 */
export function getIssue(organization: string, project: string, issueId: string): Issue | null {
    const slug = requireText(organization, "organization");
    const projectSlug = requireText(project, "project");
    const issue = requireText(issueId, "issueId");
    check("sentry.io/issues.get", { organization: slug, project: projectSlug, issue: issue });
    return loadIssueForProject(slug, projectSlug, issue);
}

/** List events belonging to an issue identified by numeric ID or short ID.
 * @param organization Organization slug, as returned by `listOrganizations`.
 * @param project Project slug, as returned by `listProjects`.
 * @param issueId Numeric group ID or short ID such as `PROJECT-7E`.
 * @param options Optional event filters and pagination; omit it for the first 50 events.
 * @returns One page of event summaries for the issue, with the cursor for the next page.
 * @capability sentry.io/issueEvents.list { organization: string, project: string, issue: string }
 */
export function listIssueEvents(
    organization: string,
    project: string,
    issueId: string,
    options: ListIssueEventsOptions = {},
): PageResult<EventSummary> {
    const slug = requireText(organization, "organization");
    const projectSlug = requireText(project, "project");
    const issue = requireText(issueId, "issueId");
    const { limit, cursor, environments: requestedEnvironments, query, statsPeriod, start, end, full, sample } = options;
    const environments: string[] = [];
    if (requestedEnvironments !== undefined) {
        for (const environment of requestedEnvironments) environments.push(environment);
    }
    const eventOptions: ListIssueEventsOptions = {
        limit: limit,
        cursor: cursor,
        environments: environments,
        query: query,
        statsPeriod: statsPeriod,
        start: start,
        end: end,
        full: full,
        sample: sample,
    };
    check("sentry.io/issueEvents.list", { organization: slug, project: projectSlug, issue: issue });
    const loaded = requireIssueForProject(slug, projectSlug, issue);
    const response = sentryGet(issuePath(slug, loaded.id) + "/events/" + buildEventQuery(eventOptions));
    const items: EventSummary[] = [];
    for (const item of response.json() as ApiEvent[]) items.push(eventSummaryFrom(item));
    return pageFrom(response, items);
}

/** Retrieve one issue event. eventId may be an event ID, latest, oldest, or recommended.
 * @param organization Organization slug, as returned by `listOrganizations`.
 * @param project Project slug, as returned by `listProjects`.
 * @param issueId Numeric group ID or short ID such as `PROJECT-7E`.
 * @param eventId Event ID, or `latest`, `oldest` or `recommended` (the default).
 * @returns Detailed event data, or `null` when the issue or event does not exist.
 * @capability sentry.io/issueEvents.get { organization: string, project: string, issue: string }
 */
export function getIssueEvent(
    organization: string,
    project: string,
    issueId: string,
    eventId: string = "recommended",
): EventDetails | null {
    const slug = requireText(organization, "organization");
    const projectSlug = requireText(project, "project");
    const issue = requireText(issueId, "issueId");
    const event = requireText(eventId, "eventId");
    check("sentry.io/issueEvents.get", { organization: slug, project: projectSlug, issue: issue });
    const loaded = loadIssueForProject(slug, projectSlug, issue);
    if (loaded === null) return null;
    const response = sentryGetNullable(issuePath(slug, loaded.id) + "/events/" + encodeComponent(event) + "/");
    return response === null ? null : eventDetailsFrom(response.json() as ApiEvent);
}

/** Apply a core triage update to an issue.
 * @param organization Organization slug, as returned by `listOrganizations`.
 * @param project Project slug, as returned by `listProjects`.
 * @param issueId Numeric group ID or short ID such as `PROJECT-7E`.
 * @param input Changes to apply; at least one field is required, and `assignedTo` cannot be combined with `clearAssignee`.
 * @returns The issue as updated by Sentry.
 * @capability sentry.io/issues.update { organization: string, project: string, issue: string }
 */
export function updateIssue(organization: string, project: string, issueId: string, input: UpdateIssueInput): Issue {
    const slug = requireText(organization, "organization");
    const projectSlug = requireText(project, "project");
    const issue = requireText(issueId, "issueId");
    const { status, substatus, assignedTo, clearAssignee, priority } = input;
    const changes: UpdateIssueInput = {
        status: status,
        substatus: substatus,
        assignedTo: assignedTo,
        clearAssignee: clearAssignee,
        priority: priority,
    };
    validateUpdate(changes);
    check("sentry.io/issues.update", { organization: slug, project: projectSlug, issue: issue });
    const loaded = requireIssueForProject(slug, projectSlug, issue);
    const response = sentryPut(issuePath(slug, loaded.id) + "/", buildUpdateIssueBody(changes));
    const updated = issueFrom(response.json() as ApiIssue);
    requireMatchingProject(updated, projectSlug);
    return updated;
}

/**
 * Build Sentry's repeated-parameter issue query. Exported for deterministic diagnostics and tests.
 * @param options Issue filters and pagination; omit it for the default page size only.
 * @returns Query string beginning with `?`, with each project and environment as a repeated parameter.
 */
export function buildIssueQuery(options: ListIssuesOptions = {}): string {
    const { limit, projects, environments, cursor, query, statsPeriod, start, end, groupStatsPeriod, sort, shortIdLookup } = options;
    const parts: string[] = [];
    addQuery(parts, "limit", pageLimit(limit).toString());
    addRepeated(parts, "project", projects);
    addRepeated(parts, "environment", environments);
    addOptionalQuery(parts, "cursor", cursor, false);
    addOptionalQuery(parts, "query", query, true);
    addOptionalQuery(parts, "statsPeriod", statsPeriod, false);
    addOptionalQuery(parts, "start", start, false);
    addOptionalQuery(parts, "end", end, false);
    addOptionalQuery(parts, "groupStatsPeriod", groupStatsPeriod, false);
    addOptionalQuery(parts, "sort", sort, false);
    if (shortIdLookup !== undefined) addQuery(parts, "shortIdLookup", shortIdLookup ? "1" : "0");
    validateTimeRange(statsPeriod, start, end);
    return "?" + parts.join("&");
}

/**
 * Build the partial update JSON body. Exported for deterministic diagnostics and tests.
 * @param input Triage changes; validated before serializing.
 * @returns JSON body containing only the supplied fields; `clearAssignee` becomes `"assignedTo":null`.
 */
export function buildUpdateIssueBody(input: UpdateIssueInput): string {
    validateUpdate(input);
    const fields: string[] = [];
    if (input.status !== undefined) fields.push(jsonProperty("status", JSON.stringify(input.status)));
    if (input.substatus !== undefined) fields.push(jsonProperty("substatus", JSON.stringify(input.substatus)));
    if (input.assignedTo !== undefined) fields.push(jsonProperty("assignedTo", JSON.stringify(input.assignedTo)));
    if (input.clearAssignee === true) fields.push(jsonProperty("assignedTo", "null"));
    if (input.priority !== undefined) fields.push(jsonProperty("priority", JSON.stringify(input.priority)));
    return "{" + fields.join(",") + "}";
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
 * Return whether a validated issue reference needs Sentry's short-ID resolver.
 *
 * Sentry builds a short ID as `PROJECT-<counter>` with the counter in base32,
 * not decimal — `RUST-7E` is an ordinary one. Requiring digits rejects most real
 * short IDs, so the suffix test is "uppercase alphanumeric", which still tells a
 * short ID apart from the all-digit group ID this resolver exists to pass
 * through, and from a lowercase slug like `not-an-id`.
 * @param issueId Issue reference, already validated as non-empty text.
 * @returns `true` for a short ID such as `PROJECT-7E`; `false` for a numeric group ID or any other text.
 */
export function isShortIssueId(issueId: string): boolean {
    if (issueId.length === 0) return false;
    const parts = issueId.split("-");
    if (parts.length < 2) return false;
    const suffix = parts[parts.length - 1];
    if (suffix.length === 0) return false;
    for (let i = 0; i < suffix.length; i += 1) {
        const code = suffix.charCodeAt(i);
        const isDigit = code >= 48 && code <= 57;
        const isUpper = code >= 65 && code <= 90;
        if (!isDigit && !isUpper) return false;
    }
    const prefixLength = issueId.length - suffix.length - 1;
    return prefixLength > 0;
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

function buildPageQuery(limit: number | undefined, cursor: string | undefined): string {
    const parts: string[] = [];
    addQuery(parts, "per_page", pageLimit(limit).toString());
    addOptionalQuery(parts, "cursor", cursor, false);
    return "?" + parts.join("&");
}

function buildEventQuery(options: ListIssueEventsOptions): string {
    const { limit, environments, cursor, query, statsPeriod, start, end, full, sample } = options;
    const parts: string[] = [];
    addQuery(parts, "per_page", pageLimit(limit).toString());
    addRepeated(parts, "environment", environments);
    addOptionalQuery(parts, "cursor", cursor, false);
    addOptionalQuery(parts, "query", query, true);
    addOptionalQuery(parts, "statsPeriod", statsPeriod, false);
    addOptionalQuery(parts, "start", start, false);
    addOptionalQuery(parts, "end", end, false);
    if (full !== undefined) addQuery(parts, "full", full ? "1" : "0");
    if (sample !== undefined) addQuery(parts, "sample", sample ? "1" : "0");
    validateTimeRange(statsPeriod, start, end);
    return "?" + parts.join("&");
}

function addRepeated(parts: string[], key: string, values: string[] | undefined): void {
    if (values === undefined) return;
    for (const value of values) addQuery(parts, key, value);
}

function addOptionalQuery(parts: string[], key: string, value: string | undefined, allowEmpty: boolean): void {
    if (value === undefined || (!allowEmpty && value.length === 0)) return;
    addQuery(parts, key, value);
}

function addQuery(parts: string[], key: string, value: string): void {
    parts.push(encodeComponent(key) + "=" + encodeComponent(value));
}

function pageLimit(value: number | undefined): number {
    const limit = value ?? DEFAULT_LIMIT;
    if (limit < 1 || limit > MAX_LIMIT || limit !== Math.floor(limit)) {
        throw validationError("invalid_page_size", "limit must be an integer between 1 and 100");
    }
    return limit;
}

function validateTimeRange(statsPeriod: string | undefined, start: string | undefined, end: string | undefined): void {
    if (statsPeriod !== undefined && (start !== undefined || end !== undefined)) {
        throw validationError("invalid_time_range", "statsPeriod cannot be combined with start or end");
    }
    if ((start === undefined) !== (end === undefined)) {
        throw validationError("invalid_time_range", "start and end must be provided together");
    }
}

function validateUpdate(input: UpdateIssueInput): void {
    const status: string = input.status ?? "";
    const substatus: string = input.substatus ?? "";
    if (input.assignedTo !== undefined && input.clearAssignee === true) {
        throw validationError("invalid_update", "assignedTo and clearAssignee cannot be combined");
    }
    if (input.assignedTo !== undefined && input.assignedTo.length === 0) {
        throw validationError("invalid_update", "assignedTo cannot be empty; use clearAssignee instead");
    }
    if (substatus.length > 0 && status.length === 0) {
        throw validationError("invalid_update", "substatus requires status in the same update");
    }
    if (substatus.length > 0 && (status === "resolved" || status === "resolvedInNextRelease")) {
        throw validationError("invalid_update", "resolved statuses cannot be combined with substatus");
    }
    if (substatus.length > 0 && status === "ignored" && !substatus.startsWith("archived_")) {
        throw validationError("invalid_update", "ignored status requires an archived substatus");
    }
    if (substatus.length > 0 && status === "unresolved" && substatus.startsWith("archived_")) {
        throw validationError("invalid_update", "unresolved status cannot use an archived substatus");
    }
    if (
        input.status === undefined && input.substatus === undefined && input.assignedTo === undefined &&
        input.clearAssignee !== true && input.priority === undefined
    ) {
        throw validationError("empty_update", "updateIssue requires at least one change");
    }
}

function resolveIssueId(organization: string, issueId: string): string | null {
    if (!isShortIssueId(issueId)) {
        for (let i = 0; i < issueId.length; i += 1) {
            const code = issueId.charCodeAt(i);
            if (code < 48 || code > 57) {
                throw validationError("invalid_issue_id", "issueId must be a numeric group ID such as 1234567890, or a short ID such as PROJECT-7E");
            }
        }
        return issueId;
    }
    const response = sentryGetNullable(
        "/organizations/" + encodeComponent(organization) + "/shortids/" + encodeComponent(issueId) + "/",
    );
    if (response === null) return null;
    const data = response.json() as ApiShortIdLookup;
    const groupId = data.groupId;
    if (groupId) return groupId;
    const embeddedId = data.group?.id;
    if (embeddedId) return embeddedId;
    throw validationError("invalid_response", "Sentry short-ID response did not include a group ID");
}

function loadIssueForProject(organization: string, project: string, issueId: string): Issue | null {
    const resolved = resolveIssueId(organization, issueId);
    if (resolved === null) return null;
    const response = sentryGetNullable(issuePath(organization, resolved) + "/");
    if (response === null) return null;
    const issue = issueFrom(response.json() as ApiIssue);
    requireMatchingProject(issue, project);
    return issue;
}

function requireIssueForProject(organization: string, project: string, issueId: string): Issue {
    const issue = loadIssueForProject(organization, project, issueId);
    if (issue === null) {
        throw new SentryError("not_found", "Sentry issue was not found", 404, "", null, null, null, null, null, null);
    }
    return issue;
}

function requireMatchingProject(issue: Issue, expectedProject: string): void {
    if (issue.project.slug !== expectedProject) {
        throw validationError(
            "project_mismatch",
            "Sentry issue belongs to project " + issue.project.slug + ", not " + expectedProject,
        );
    }
}

function issuePath(organization: string, issueId: string): string {
    return "/organizations/" + encodeComponent(organization) + "/issues/" + encodeComponent(issueId);
}

function sentryGet(path: string): Response {
    return requireOk(get(API + path, authHeaders()));
}

function sentryGetNullable(path: string): Response | null {
    const response = get(API + path, authHeaders());
    if (response.status === 404) return null;
    return requireOk(response);
}

function sentryPut(path: string, body: string): Response {
    return requireOk(put(API + path, body, authHeaders()));
}

function authHeaders(): Map<string, string> {
    const token = secrets.get("SENTRY_AUTH_TOKEN");
    if (token === undefined) throw validationError("missing_token", "SENTRY_AUTH_TOKEN is not bound");
    const headers = new Map<string, string>();
    headers.set("Authorization", "Bearer " + token);
    headers.set("Accept", "application/json");
    headers.set("Content-Type", "application/json");
    headers.set("User-Agent", "submilli-sentry/0.1.0");
    return headers;
}

function requireOk(response: Response): Response {
    if (response.ok) return response;
    throw sentryError(response);
}

function sentryError(response: Response): SentryError {
    return new SentryError(
        sentryErrorCode(response.status),
        sentryFailureMessage(response.status, response.statusText, response.body),
        response.status,
        firstHeader(response, "x-sentry-request-id", "x-request-id"),
        numberHeader(response, "retry-after"),
        numberHeader(response, "x-sentry-rate-limit-limit"),
        numberHeader(response, "x-sentry-rate-limit-remaining"),
        numberHeader(response, "x-sentry-rate-limit-reset"),
        numberHeader(response, "x-sentry-rate-limit-concurrentlimit"),
        numberHeader(response, "x-sentry-rate-limit-concurrentremaining"),
    );
}

function validationError(code: string, message: string): SentryError {
    return new SentryError(code, message, 0, "", null, null, null, null, null, null);
}

function firstHeader(response: Response, first: string, second: string): string {
    const value = response.headers.get(first);
    if (value !== undefined) return value;
    return header(response, second);
}

function header(response: Response, name: string): string {
    return response.headers.get(name) ?? "";
}

function numberHeader(response: Response, name: string): number | null {
    const value = response.headers.get(name);
    if (value === undefined || value.length === 0) return null;
    const parsed = Number(value);
    return isNaN(parsed) ? null : parsed;
}

function pageFrom<T>(response: Response, items: T[]): PageResult<T> {
    const next = parseNextCursor(header(response, "link"));
    return { items: items, nextCursor: next, isComplete: next.length === 0 };
}

function organizationFrom(data: ApiOrganization): Organization {
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

function projectFrom(data: ApiProject, fallbackOrganizationSlug: string): Project {
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

function issueFrom(data: ApiIssue): Issue {
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

function eventSummaryFrom(data: ApiEvent): EventSummary {
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

function eventDetailsFrom(data: ApiEvent): EventDetails {
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

function requireText(value: string, name: string): string {
    if (value.length === 0) throw validationError("invalid_input", name + " cannot be empty");
    return value;
}

function jsonProperty(name: string, value: string): string {
    return JSON.stringify(name) + ":" + value;
}

function listed<T>(value: T[] | null | undefined): T[] {
    if (value) return value;
    return [];
}
