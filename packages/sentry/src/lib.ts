import { get, put, Response } from "submilli:http";
import { encodeComponent } from "submilli:url";
import secrets from "submilli:secrets";
import { check } from "submilli:security";
import { SentryError, validationError } from "./errors";
import {
    buildEventQuery,
    buildIssueQuery,
    buildPageQuery,
    buildUpdateIssueBody,
    isShortIssueId,
    ListIssueEventsOptions,
    ListIssuesOptions,
    requireText,
    UpdateIssueInput,
    validateUpdate,
} from "./request";
import {
    ApiEvent,
    ApiIssue,
    ApiOrganization,
    ApiProject,
    ApiShortIdLookup,
    EventDetails,
    eventDetailsFrom,
    EventSummary,
    eventSummaryFrom,
    Issue,
    issueFrom,
    Organization,
    organizationFrom,
    parseNextCursor,
    Project,
    projectFrom,
    sentryErrorCode,
    sentryFailureMessage,
} from "./response";

export { SentryError } from "./errors";
export {
    IssueSort,
    ListIssuesOptions,
    ListIssueEventsOptions,
    IssueStatus,
    IssuePriority,
    IssueSubstatus,
    UpdateIssueInput,
} from "./request";
export {
    Organization,
    Project,
    ProjectSummary,
    Actor,
    IssueMetadata,
    Issue,
    Tag,
    EventUser,
    ExceptionMechanism,
    SourceLine,
    StackFrame,
    EventException,
    Breadcrumb,
    KeyValue,
    EventRequest,
    ReleaseSummary,
    EventSummary,
    EventDetails,
} from "./response";

const API = "https://sentry.io/api/0";

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
