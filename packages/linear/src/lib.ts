// A synchronous client for Linear (https://linear.app), talking raw GraphQL
// over `submilli:http`. Linear's official @linear/sdk is class- and
// Promise-based; this is a function-per-operation client returning plain typed
// structs, which fits the Submilli subset and is more efficient than the SDK's
// lazy per-relation fetches.
//
// The API token VALUE is never passed in — it is read from the `LINEAR_API_KEY`
// secret via the secrets capability at call time, so it stays out of the
// agent-visible surface. AUTH GOTCHA: a personal API key goes in a RAW
// `Authorization` header (no `Bearer` prefix; `Bearer` is OAuth-only).

import { post, Response } from "submilli:http";
import secrets from "submilli:secrets";
import { check } from "submilli:security";

const ENDPOINT = "https://api.linear.app/graphql";

// ---------------------------------------------------------------------------
// Public entity types (curated; 1-level relations embedded in the query).
// ---------------------------------------------------------------------------

/** A Linear user (the viewer, an assignee, a comment author, …). */
export interface User {
    /** Stable UUID. */
    id: string;
    /** Display name. */
    name: string;
    /** Email address. */
    email: string;
    /** Whether the account is active. */
    active: boolean;
}

/** A Linear team. */
export interface Team {
    /** Stable UUID. */
    id: string;
    /** Short key, e.g. "ENG". */
    key: string;
    /** Team name. */
    name: string;
}

/** A workflow state an issue can be in. */
export interface WorkflowState {
    /** Stable UUID. */
    id: string;
    /** State name, e.g. "In Progress". */
    name: string;
    /** Category: "backlog" | "unstarted" | "started" | "completed" | "canceled". */
    type: string;
}

/** A Linear project. */
export interface Project {
    /** Stable UUID. */
    id: string;
    /** Project name. */
    name: string;
    /** Project state, e.g. "started". */
    state: string;
}

/** A Linear cycle. */
export interface Cycle {
    /** Stable UUID. */
    id: string;
    /** Cycle number within the team. */
    number: number;
    /** Cycle name. */
    name: string;
    /** Cycle start time, or null when unset. */
    startsAt: string | null;
    /** Cycle end time, or null when unset. */
    endsAt: string | null;
}

/** A Linear issue, with its 1-level relations embedded. */
export interface Issue {
    /** Stable UUID. */
    id: string;
    /** Human identifier, e.g. "ENG-123". */
    identifier: string;
    /** Numeric issue number scoped to the team. */
    number: number;
    /** Issue title. */
    title: string;
    /** Markdown body, or null when unset. */
    description: string | null;
    /** Priority: 0=none, 1=urgent, 2=high, 3=medium, 4=low. */
    priority: number;
    /** Human label for the priority, e.g. "High". */
    priorityLabel: string;
    /** Web URL. */
    url: string;
    /** ISO timestamp when the issue was created. */
    createdAt: string;
    /** ISO timestamp when the issue was last meaningfully updated. */
    updatedAt: string;
    /** ISO timestamp when the issue moved into completed state, or null. */
    completedAt: string | null;
    /** ISO timestamp when the issue moved into canceled state, or null. */
    canceledAt: string | null;
    /** ISO timestamp when the issue moved into started state, or null. */
    startedAt: string | null;
    /** ISO timestamp when the issue left triage, or null. */
    triagedAt: string | null;
    /** ISO timestamp when the issue was archived, or null. */
    archivedAt: string | null;
    /** ISO timestamp when Linear auto-closed the issue, or null. */
    autoClosedAt: string | null;
    /** Due date in YYYY-MM-DD form, or null. */
    dueDate: string | null;
    /** Estimate value, or null when unset. */
    estimate: number | null;
    /** UUIDs of labels currently attached to the issue. */
    labelIds: string[];
    /** Assignee, or null when unassigned. */
    assignee: User | null;
    /** Creator, or null when unavailable. */
    creator: User | null;
    /** Owning team, or null. */
    team: Team | null;
    /** Current workflow state, or null. */
    state: WorkflowState | null;
    /** Project, or null when not assigned to a project. */
    project: Project | null;
    /** Cycle, or null when not assigned to a cycle. */
    cycle: Cycle | null;
}

/** A comment on an issue. */
export interface Comment {
    /** Parent thread, if this is a reply. */
    parent?: EntityReference | null;
    /** Stable UUID. */
    id: string;
    /** Markdown body. */
    body: string;
    /** Author, or null. */
    user: User | null;
}

// ---------------------------------------------------------------------------
// Mutation inputs (optional fields are omitted from the request, so an update
// only touches the fields you set — absent ≠ null).
// ---------------------------------------------------------------------------

/** Input for `createIssue`. Only `teamId` is required. */
export interface IssueCreateInput {
    /** Owning team UUID (required). */
    teamId: string;
    /** Title. */
    title?: string;
    /** Markdown description. */
    description?: string;
    /** Assignee UUID. */
    assigneeId?: string;
    /** Workflow state UUID. */
    stateId?: string;
    /** Priority: 0=none, 1=urgent, 2=high, 3=medium, 4=low. */
    priority?: number;
    /** Label UUIDs. */
    labelIds?: string[];
    /** Project UUID. */
    projectId?: string;
}

/** Input for `updateIssue`. Every field is optional; only set ones are changed. */
export interface IssueUpdateInput {
    /** New title. */
    title?: string;
    /** New markdown description. */
    description?: string;
    /** Reassign to this user UUID. */
    assigneeId?: string;
    /** Move to this workflow state UUID. */
    stateId?: string;
    /** New priority 0-4. */
    priority?: number;
    /** Replace the label set. */
    labelIds?: string[];
    /** Move to this project UUID. */
    projectId?: string;
}

/** Input for `createComment`. */
export interface CommentCreateInput {
    /** Target issue UUID. */
    issueId: string;
    /** Markdown body. */
    body: string;
    /** Parent comment UUID for a threaded reply. */
    parentId?: string;
}

// ---------------------------------------------------------------------------
// Agent session types.
// ---------------------------------------------------------------------------

/** A related entity's stable UUID. */
export interface EntityReference {
    /** Stable UUID. */
    id: string;
}

/** A named link shown on the Linear agent session. */
export interface AgentSessionExternalUrl {
    /** Human-readable label. */
    label: string;
    /** URL to open. */
    url: string;
}

/** One item in the complete session plan. */
export interface AgentPlanStep {
    /** Description of the step. */
    content: string;
    /** Current lifecycle or plan status. */
    status: "pending" | "inProgress" | "completed" | "canceled";
}

/** Session state is driven by activities, not set manually. */
export interface AgentSession {
    /** Stable UUID. */
    id: string;
    /** Current lifecycle or plan status. */
    status: string;
    /** URL to open. */
    url: string | null;
    /** Session title, or null. */
    summary: string | null;
    /** Associated issue, or null. */
    issue: EntityReference | null;
    /** Associated comment, or null. */
    comment: EntityReference | null;
    /** Named external links; on update, replaces the entire list. */
    externalUrls: AgentSessionExternalUrl[];
    /** Complete session plan, or null. */
    plan: AgentPlanStep[] | null;
}

/** A progress message, clarification request, result, or failure. */
export interface AgentTextContent {
    /** Semantic activity type. */
    type: "thought" | "elicitation" | "response" | "error";
    /** Markdown message. */
    body: string;
}

/** A tool action and optional result. */
export interface AgentActionContent {
    /** Semantic activity type. */
    type: "action";
    /** Action being performed. */
    action: string;
    /** Action input, such as a path or search term. */
    parameter: string;
    /** Optional Markdown action result. */
    result?: string;
}

/** Prompt activities are read-only: they originate from the user. */
export interface AgentPromptContent {
    /** Semantic activity type. */
    type: "prompt";
    /** Markdown message. */
    body: string;
}
/** Activity types an agent may emit; user prompts are excluded. */
export type AgentActivityContent = AgentTextContent | AgentActionContent;

/** One choice presented with a select signal. */
export interface AgentSelectOption {
    /** Human-readable label. */
    label?: string;
    /** Value returned when the choice is selected. */
    value: string;
}
/** Metadata for authentication links and selection prompts. */
export interface AgentSignalMetadata {
    /** URL to open. */
    url?: string;
    /** Optional user allowed to complete authentication. */
    userId?: string;
    /** Provider shown in the authentication prompt. */
    providerName?: string;
    /** Choices shown for a select signal. */
    options?: AgentSelectOption[];
}

/** One recorded session activity, including incoming user prompts. */
export interface AgentActivity {
    /** Stable UUID. */
    id: string;
    /** Creation timestamp. */
    createdAt: string;
    /** Whether the next activity replaces this activity. */
    ephemeral: boolean;
    /** Structured activity content. */
    content: AgentActivityContent | AgentPromptContent;
    /** Intent modifier; incoming user prompts may carry stop. */
    signal: string | null;
    /** Additional signal details. */
    signalMetadata: AgentSignalMetadata | null;
}

/** Input for emitting an activity as the installed app. */
export interface AgentActivityCreateInput {
    /** Target session UUID. */
    agentSessionId: string;
    /** Structured activity content. */
    content: AgentActivityContent;
    /** Only thought and action activities may be ephemeral. */
    ephemeral?: boolean;
    /** Optional caller-provided UUID. */
    id?: string;
    /** Authentication or selection prompt modifier. */
    signal?: "auth" | "select";
    /** Additional signal details. */
    signalMetadata?: AgentSignalMetadata;
}

/** Partial update; omitted fields remain unchanged. */
export interface AgentSessionUpdateInput {
    /** Named external links; on update, replaces the entire list. */
    externalUrls?: AgentSessionExternalUrl[];
    /** Links to add without replacing existing links. */
    addedExternalUrls?: AgentSessionExternalUrl[];
    /** URLs to remove. */
    removedExternalUrls?: string[];
    /** Replaces the complete plan. */
    plan?: AgentPlanStep[];
    /** Session title, or null. */
    summary?: string | null;
}

/** Input for proactively starting a session on an issue. */
export interface AgentSessionCreateOnIssueInput {
    /** Target issue UUID. */
    issueId: string;
    /** Named external links; on update, replaces the entire list. */
    externalUrls?: AgentSessionExternalUrl[];
}

/** Input for proactively starting a session on a comment. */
export interface AgentSessionCreateOnCommentInput {
    /** Target comment UUID. */
    commentId: string;
    /** Named external links; on update, replaces the entire list. */
    externalUrls?: AgentSessionExternalUrl[];
}


// ---------------------------------------------------------------------------
// Pagination + filtering.
// ---------------------------------------------------------------------------

/** Relay page-cursor metadata. */
export interface PageInfo {
    /** True when another page exists. */
    hasNextPage: boolean;
    /** Cursor to pass as `after` for the next page; null when there is no next page. */
    endCursor: string | null;
}

/** A page of `T` from a Relay connection. */
export interface Page<T> {
    /** Items on this page. */
    nodes: T[];
    /** Cursor metadata for fetching the next page. */
    pageInfo: PageInfo;
}

/** Pagination options for list calls. All optional. */
export interface PageOptions {
    /** Max items to fetch (Linear default 50, max 250). */
    first?: number;
    /** Opaque cursor from a prior `PageInfo.endCursor`; a null cursor is treated as absent. */
    after?: string | null;
}

/** A curated issue filter; maps to Linear's nested `IssueFilter`. All optional. */
export interface IssueFilter {
    /** Restrict to a team UUID. */
    teamId?: string;
    /** Restrict to an assignee UUID. */
    assigneeId?: string;
    /** Restrict by state category, e.g. "started". */
    stateType?: string;
    /** Restrict to issues completed at or after this ISO timestamp. */
    completedAtAfter?: string;
    /** Restrict to issues completed before this ISO timestamp. */
    completedAtBefore?: string;
    /** Restrict to issues created at or after this ISO timestamp. */
    createdAtAfter?: string;
    /** Restrict to issues updated at or after this ISO timestamp. */
    updatedAtAfter?: string;
}

// ---------------------------------------------------------------------------
// Read operations.
// ---------------------------------------------------------------------------

/**
 * Fetch the authenticated user (whoever owns the API token).
 * @capability linear.app/getViewer {}
 */
export function getViewer(): User {
    check("linear.app/getViewer", {});
    const envelope = graphqlPost(VIEWER_QUERY, {}).json() as GraphQlResponse<ViewerData>;
    return requireData(envelope).viewer;
}

/**
 * Fetch one issue by UUID; null when not found.
 * @capability linear.app/getIssue {}
 */
export function getIssue(id: string): Issue | null {
    check("linear.app/getIssue", {});
    const vars: IdVars = { id: id };
    const envelope = graphqlPost(GET_ISSUE_QUERY, vars).json() as GraphQlResponse<IssueData>;
    return requireData(envelope).issue;
}

/**
 * List issues, optionally filtered and paginated.
 * @capability linear.app/listIssues { teamId: string }
 */
export function listIssues(
    filter: IssueFilter | null = null,
    page: PageOptions | null = null,
): Page<Issue> {
    const teamId = filter !== null ? filter.teamId : null;
    check("linear.app/listIssues", { teamId: teamId });
    const vars = buildListIssuesVars(filter, page);
    const envelope = graphqlPost(LIST_ISSUES_QUERY, vars).json() as GraphQlResponse<ListIssuesData>;
    return requireData(envelope).issues;
}

/**
 * Fetch one team by UUID; null when not found.
 * @capability linear.app/getTeam { teamId: $id }
 */
export function getTeam(id: string): Team | null {
    check("linear.app/getTeam", { teamId: id });
    const vars: IdVars = { id: id };
    const envelope = graphqlPost(GET_TEAM_QUERY, vars).json() as GraphQlResponse<TeamData>;
    return requireData(envelope).team;
}

/**
 * List teams, paginated.
 * @capability linear.app/listTeams {}
 */
export function listTeams(page: PageOptions | null = null): Page<Team> {
    check("linear.app/listTeams", {});
    const envelope = graphqlPost(LIST_TEAMS_QUERY, buildPageVars(page))
        .json() as GraphQlResponse<ListTeamsData>;
    return requireData(envelope).teams;
}

/**
 * List projects, paginated.
 * @capability linear.app/listProjects {}
 */
export function listProjects(page: PageOptions | null = null): Page<Project> {
    check("linear.app/listProjects", {});
    const envelope = graphqlPost(LIST_PROJECTS_QUERY, buildPageVars(page))
        .json() as GraphQlResponse<ListProjectsData>;
    return requireData(envelope).projects;
}

/**
 * List users, paginated.
 * @capability linear.app/listUsers {}
 */
export function listUsers(page: PageOptions | null = null): Page<User> {
    check("linear.app/listUsers", {});
    const envelope = graphqlPost(LIST_USERS_QUERY, buildPageVars(page))
        .json() as GraphQlResponse<ListUsersData>;
    return requireData(envelope).users;
}

// ---------------------------------------------------------------------------
// Write operations.
// ---------------------------------------------------------------------------

/**
 * Create an issue; returns the created issue.
 * @capability linear.app/createIssue { teamId: string }
 */
export function createIssue(input: IssueCreateInput): Issue {
    check("linear.app/createIssue", { teamId: input.teamId });
    const vars: CreateIssueVars = { input: input };
    const envelope = graphqlPost(CREATE_ISSUE_QUERY, vars).json() as GraphQlResponse<IssueCreateData>;
    return requirePayloadIssue(requireData(envelope).issueCreate);
}

/**
 * Update an issue by UUID; returns the updated issue.
 * @capability linear.app/updateIssue {}
 */
export function updateIssue(id: string, input: IssueUpdateInput): Issue {
    check("linear.app/updateIssue", {});
    const vars: UpdateIssueVars = { id: id, input: input };
    const envelope = graphqlPost(UPDATE_ISSUE_QUERY, vars).json() as GraphQlResponse<IssueUpdateData>;
    return requirePayloadIssue(requireData(envelope).issueUpdate);
}

/**
 * Create a comment on an issue; returns the created comment.
 * @capability linear.app/createComment {}
 */
export function createComment(input: CommentCreateInput): Comment {
    check("linear.app/createComment", {});
    const vars: CreateCommentVars = { input: input };
    const envelope = graphqlPost(CREATE_COMMENT_QUERY, vars)
        .json() as GraphQlResponse<CommentCreateData>;
    const payload = requireData(envelope).commentCreate;
    const comment = payload.comment;
    if (!payload.success || comment === null) {
        throw new Error("Linear commentCreate did not succeed");
    }
    return comment;
}

/** Read comments, including parent IDs for threaded replies.
 * @capability linear.app/listComments {}
 */
export function listComments(issueId: string, page: PageOptions | null = null): Page<Comment> {
    check("linear.app/listComments", {});
    const vars = buildEntityPageVars(issueId, page);
    const envelope = graphqlPost(LIST_COMMENTS_QUERY, vars).json() as GraphQlResponse<IssueCommentsData>;
    return requireData(envelope).issue.comments;
}

/** Fetch a Linear agent session by UUID.
 * @capability linear.app/getAgentSession {}
 */
export function getAgentSession(id: string): AgentSession {
    check("linear.app/getAgentSession", {});
    const vars: IdVars = { id: id };
    const envelope = graphqlPost(GET_AGENT_SESSION_QUERY, vars).json() as GraphQlResponse<AgentSessionData>;
    return requireData(envelope).agentSession;
}

/** Read a page of session activities, including user prompts and stop signals.
 * @capability linear.app/listAgentActivities {}
 */
export function listAgentActivities(id: string, page: PageOptions | null = null): Page<AgentActivity> {
    check("linear.app/listAgentActivities", {});
    const vars = buildEntityPageVars(id, page);
    const envelope = graphqlPost(LIST_AGENT_ACTIVITIES_QUERY, vars).json() as GraphQlResponse<AgentActivitiesData>;
    return requireData(envelope).agentSession.activities;
}

/** Emit progress, a question, a final response, or an error.
 * @capability linear.app/createAgentActivity {}
 */
export function createAgentActivity(input: AgentActivityCreateInput): AgentActivity {
    check("linear.app/createAgentActivity", {});
    const vars: CreateAgentActivityVars = { input: input };
    const envelope = graphqlPost(CREATE_AGENT_ACTIVITY_QUERY, vars).json() as GraphQlResponse<AgentActivityCreateData>;
    const payload = requireData(envelope).agentActivityCreate;
    if (!payload.success || payload.agentActivity === null) {
        throw new Error("Linear agentActivityCreate did not succeed");
    }
    return payload.agentActivity;
}

/** Update session links, summary, or the complete plan.
 * @capability linear.app/updateAgentSession {}
 */
export function updateAgentSession(id: string, input: AgentSessionUpdateInput): AgentSession {
    check("linear.app/updateAgentSession", {});
    const vars: UpdateAgentSessionVars = { id: id, input: input };
    const envelope = graphqlPost(UPDATE_AGENT_SESSION_QUERY, vars).json() as GraphQlResponse<AgentSessionUpdateData>;
    return requireAgentSession(requireData(envelope).agentSessionUpdate, "agentSessionUpdate");
}

/** Proactively create a session on an issue using the installed app's token.
 * @capability linear.app/createAgentSessionOnIssue {}
 */
export function createAgentSessionOnIssue(input: AgentSessionCreateOnIssueInput): AgentSession {
    check("linear.app/createAgentSessionOnIssue", {});
    const vars: CreateAgentSessionOnIssueVars = { input: input };
    const envelope = graphqlPost(CREATE_AGENT_SESSION_ON_ISSUE_QUERY, vars).json() as GraphQlResponse<AgentSessionCreateOnIssueData>;
    return requireAgentSession(requireData(envelope).agentSessionCreateOnIssue, "agentSessionCreateOnIssue");
}

/** Proactively create a session on an existing comment using the app's token.
 * @capability linear.app/createAgentSessionOnComment {}
 */
export function createAgentSessionOnComment(input: AgentSessionCreateOnCommentInput): AgentSession {
    check("linear.app/createAgentSessionOnComment", {});
    const vars: CreateAgentSessionOnCommentVars = { input: input };
    const envelope = graphqlPost(CREATE_AGENT_SESSION_ON_COMMENT_QUERY, vars).json() as GraphQlResponse<AgentSessionCreateOnCommentData>;
    return requireAgentSession(requireData(envelope).agentSessionCreateOnComment, "agentSessionCreateOnComment");
}

function requireAgentSession(payload: AgentSessionPayload, operation: string): AgentSession {
    if (!payload.success || payload.agentSession === null) {
        throw new Error(`Linear ${operation} did not succeed`);
    }
    return payload.agentSession;
}

function buildEntityPageVars(id: string, page: PageOptions | null): EntityPageVars {
    const pagination = buildPageVars(page);
    const vars: EntityPageVars = { id: id };
    if (pagination.first !== null) vars.first = pagination.first;
    if (pagination.after !== null) vars.after = pagination.after;
    return vars;
}

// ---------------------------------------------------------------------------
// Pure variable builders (no network/secrets) — unit-tested directly.
// ---------------------------------------------------------------------------

// `Temporal.ZonedDateTime.toString()` appends a bracketed zone annotation
// ("2026-07-15T13:36:00+00:00[UTC]") that Linear's DateTime scalar rejects with
// an HTTP 400. The annotation names the zone the offset already encodes, so
// stripping it preserves the instant — accept any Temporal string form rather
// than making the caller know which toString() is safe.
function stripZoneAnnotation(timestamp: string): string {
    const bracket = timestamp.indexOf("[");
    if (bracket === -1) {
        return timestamp;
    }
    return timestamp.slice(0, bracket);
}

/** Translate a curated `IssueFilter` into Linear's nested filter variable. */
export function buildIssueFilter(filter: IssueFilter | null): IssueFilterVar {
    const out: IssueFilterVar = {};
    if (filter === null) {
        return out;
    }
    const teamId = filter.teamId;
    const assigneeId = filter.assigneeId;
    const stateType = filter.stateType;
    const completedAtAfter = filter.completedAtAfter;
    const completedAtBefore = filter.completedAtBefore;
    const createdAtAfter = filter.createdAtAfter;
    const updatedAtAfter = filter.updatedAtAfter;
    if (teamId !== null) {
        out.team = { id: { eq: teamId } };
    }
    if (assigneeId !== null) {
        out.assignee = { id: { eq: assigneeId } };
    }
    if (stateType !== null) {
        out.state = { type: { eq: stateType } };
    }
    if (completedAtAfter !== null || completedAtBefore !== null) {
        const completedAt: DateComparatorVar = {};
        if (completedAtAfter !== null) {
            completedAt.gte = stripZoneAnnotation(completedAtAfter);
        }
        if (completedAtBefore !== null) {
            completedAt.lt = stripZoneAnnotation(completedAtBefore);
        }
        out.completedAt = completedAt;
    }
    if (createdAtAfter !== null) {
        out.createdAt = { gte: stripZoneAnnotation(createdAtAfter) };
    }
    if (updatedAtAfter !== null) {
        out.updatedAt = { gte: stripZoneAnnotation(updatedAtAfter) };
    }
    return out;
}

// Linear rejects `after` unless `first` is also present ("after cannot be used
// without at least one of first") and otherwise defaults the page size to 50, so
// we always send an explicit `first` — a caller paginating with just a cursor
// would otherwise send `{ after }` and every page past the first would fail.
const DEFAULT_PAGE_SIZE = 50;

/** Build the `first`/`after` variables for a paginated list call. */
export function buildPageVars(page: PageOptions | null): PageVars {
    const out: PageVars = { first: DEFAULT_PAGE_SIZE };
    if (page !== null) {
        const first = page.first;
        const after = page.after;
        if (first !== null) {
            out.first = first;
        }
        if (after !== null) {
            out.after = after;
        }
    }
    return out;
}

/** Build the variables for `listIssues` — pagination plus an optional filter. */
export function buildListIssuesVars(
    filter: IssueFilter | null,
    page: PageOptions | null,
): ListIssuesVars {
    const out: ListIssuesVars = { first: DEFAULT_PAGE_SIZE };
    if (page !== null) {
        const first = page.first;
        const after = page.after;
        if (first !== null) {
            out.first = first;
        }
        if (after !== null) {
            out.after = after;
        }
    }
    if (filter !== null) {
        out.filter = buildIssueFilter(filter);
    }
    return out;
}

// ---------------------------------------------------------------------------
// Internal: GraphQL plumbing.
// ---------------------------------------------------------------------------

/** A GraphQL error-path segment: a field name or a list index. */
export type GraphQlPathSegment = string | number;

// Linear puts the actionable detail behind a generic top-level `message`
// ("Argument Validation Error"): `extensions.code` carries the machine code,
// `extensions.validationErrors[].property` names the offending argument, and
// `path` locates the failing field. We surface all of them so a caller learns
// *which* argument is wrong instead of just that *some* argument is. The dynamic
// per-constraint messages (`constraints.<RuleName>`) can't be modelled in our
// JSON.parse subset (object with dynamic keys), so we lift the property names.
/** A Linear GraphQL error. Exported so the rendering helper's signature is nameable. */
export interface GraphQlError {
    /** The generic top-level message (often just "Argument Validation Error"). */
    message: string;
    /** Field path to the error, e.g. `["issues"]`. */
    path?: GraphQlPathSegment[];
    /** Implementation-specific detail; where Linear hides the actionable cause. */
    extensions?: GraphQlErrorExtensions;
}

/** The subset of Linear's GraphQL error `extensions` we surface. */
export interface GraphQlErrorExtensions {
    /** Machine code, e.g. "INVALID_INPUT". */
    code?: string;
    /** A human-readable detail Linear sometimes includes. */
    userPresentableMessage?: string;
    /** Per-argument validation failures. */
    validationErrors?: ValidationErrorDetail[];
}

/** One class-validator failure inside `extensions.validationErrors`. */
export interface ValidationErrorDetail {
    /** The offending argument/field name, e.g. "after". */
    property?: string;
}

// `data` is read concretely per operation (`json<GraphQlResponse<XData>>()`):
// JSON.parse can't target an unconstrained generic, so the envelope is parsed
// with a concrete data type at each call site, then `requireData` does the
// shared error/null unwrap generically (it only reads fields, never parses).
interface GraphQlResponse<T> {
    data: T | null;
    errors?: GraphQlError[];
}

/** Linear's nested issue-filter variable shape (built by `buildIssueFilter`). */
export interface IssueFilterVar {
    /** Team-id equality clause. */
    team?: { id: { eq: string } };
    /** Assignee-id equality clause. */
    assignee?: { id: { eq: string } };
    /** State-type equality clause. */
    state?: { type: { eq: string } };
    /** Completed-at date comparator. */
    completedAt?: DateComparatorVar;
    /** Created-at date comparator. */
    createdAt?: DateComparatorVar;
    /** Updated-at date comparator. */
    updatedAt?: DateComparatorVar;
}

/** Linear date comparator subset used by curated filters. */
export interface DateComparatorVar {
    /** Greater-than-or-equal bound. */
    gte?: string;
    /** Strict less-than bound. */
    lt?: string;
}

/** Pagination variables (`first`/`after`) for a list call. */
export interface PageVars {
    /** Page size. */
    first?: number;
    /** Cursor. */
    after?: string;
}

/** Variables for `listIssues`: pagination plus an optional filter. */
export interface ListIssuesVars {
    /** Page size. */
    first?: number;
    /** Cursor. */
    after?: string;
    /** Nested issue filter. */
    filter?: IssueFilterVar;
}

interface IdVars {
    id: string;
}

interface ViewerData {
    viewer: User;
}
interface IssueData {
    issue: Issue | null;
}
interface ListIssuesData {
    issues: Page<Issue>;
}
interface TeamData {
    team: Team | null;
}
interface ListTeamsData {
    teams: Page<Team>;
}
interface ListProjectsData {
    projects: Page<Project>;
}
interface ListUsersData {
    users: Page<User>;
}

interface IssuePayload {
    success: boolean;
    issue: Issue | null;
}
interface CommentPayload {
    success: boolean;
    comment: Comment | null;
}
interface IssueCreateData {
    issueCreate: IssuePayload;
}
interface IssueUpdateData {
    issueUpdate: IssuePayload;
}
interface CommentCreateData {
    commentCreate: CommentPayload;
}

interface CreateIssueVars {
    input: IssueCreateInput;
}
interface UpdateIssueVars {
    id: string;
    input: IssueUpdateInput;
}
interface CreateCommentVars {
    input: CommentCreateInput;
}

interface EntityPageVars { id: string; first?: number; after?: string; }
interface IssueComments { comments: Page<Comment>; }
interface IssueCommentsData { issue: IssueComments; }
interface AgentSessionData { agentSession: AgentSession; }
interface SessionActivities { activities: Page<AgentActivity>; }
interface AgentActivitiesData { agentSession: SessionActivities; }
interface AgentSessionPayload { success: boolean; agentSession: AgentSession | null; }
interface AgentActivityPayload { success: boolean; agentActivity: AgentActivity | null; }
interface AgentActivityCreateData { agentActivityCreate: AgentActivityPayload; }
interface AgentSessionUpdateData { agentSessionUpdate: AgentSessionPayload; }
interface AgentSessionCreateOnIssueData { agentSessionCreateOnIssue: AgentSessionPayload; }
interface AgentSessionCreateOnCommentData { agentSessionCreateOnComment: AgentSessionPayload; }
interface CreateAgentActivityVars { input: AgentActivityCreateInput; }
interface UpdateAgentSessionVars { id: string; input: AgentSessionUpdateInput; }
interface CreateAgentSessionOnIssueVars { input: AgentSessionCreateOnIssueInput; }
interface CreateAgentSessionOnCommentVars { input: AgentSessionCreateOnCommentInput; }

const COMMENT_FIELDS = "id body user { id name email active } parent { id }";
const AGENT_SESSION_FIELDS = "id status url summary issue { id } comment { id } externalUrls plan";
const AGENT_ACTIVITY_FIELDS = "id createdAt ephemeral signal signalMetadata content { " +
    "... on AgentActivityThoughtContent { type body } " +
    "... on AgentActivityActionContent { type action parameter result } " +
    "... on AgentActivityElicitationContent { type body } " +
    "... on AgentActivityResponseContent { type body } " +
    "... on AgentActivityErrorContent { type body } " +
    "... on AgentActivityPromptContent { type body } }";
const LIST_COMMENTS_QUERY = `query($id: String!, $first: Int, $after: String) { issue(id: $id) { comments(first: $first, after: $after) { nodes { ${COMMENT_FIELDS} } pageInfo { hasNextPage endCursor } } } }`;
const GET_AGENT_SESSION_QUERY = `query($id: String!) { agentSession(id: $id) { ${AGENT_SESSION_FIELDS} } }`;
const LIST_AGENT_ACTIVITIES_QUERY = `query($id: String!, $first: Int, $after: String) { agentSession(id: $id) { activities(first: $first, after: $after) { nodes { ${AGENT_ACTIVITY_FIELDS} } pageInfo { hasNextPage endCursor } } } }`;
const CREATE_AGENT_ACTIVITY_QUERY = `mutation($input: AgentActivityCreateInput!) { agentActivityCreate(input: $input) { success agentActivity { ${AGENT_ACTIVITY_FIELDS} } } }`;
const UPDATE_AGENT_SESSION_QUERY = `mutation($id: String!, $input: AgentSessionUpdateInput!) { agentSessionUpdate(id: $id, input: $input) { success agentSession { ${AGENT_SESSION_FIELDS} } } }`;
const CREATE_AGENT_SESSION_ON_ISSUE_QUERY = `mutation($input: AgentSessionCreateOnIssue!) { agentSessionCreateOnIssue(input: $input) { success agentSession { ${AGENT_SESSION_FIELDS} } } }`;
const CREATE_AGENT_SESSION_ON_COMMENT_QUERY = `mutation($input: AgentSessionCreateOnComment!) { agentSessionCreateOnComment(input: $input) { success agentSession { ${AGENT_SESSION_FIELDS} } } }`;

const ISSUE_FIELDS =
    "id identifier number title description priority priorityLabel url " +
    "createdAt updatedAt completedAt canceledAt startedAt triagedAt archivedAt autoClosedAt " +
    "dueDate estimate labelIds " +
    "assignee { id name email active } " +
    "creator { id name email active } " +
    "team { id key name } " +
    "state { id name type } " +
    "project { id name state } " +
    "cycle { id number name startsAt endsAt }";

const VIEWER_QUERY = "query { viewer { id name email active } }";
const GET_ISSUE_QUERY = `query($id: String!) { issue(id: $id) { ${ISSUE_FIELDS} } }`;
const LIST_ISSUES_QUERY = `query($first: Int, $after: String, $filter: IssueFilter) { issues(first: $first, after: $after, filter: $filter) { nodes { ${ISSUE_FIELDS} } pageInfo { hasNextPage endCursor } } }`;
const GET_TEAM_QUERY = "query($id: String!) { team(id: $id) { id key name } }";
const LIST_TEAMS_QUERY = "query($first: Int, $after: String) { teams(first: $first, after: $after) { nodes { id key name } pageInfo { hasNextPage endCursor } } }";
const LIST_PROJECTS_QUERY = "query($first: Int, $after: String) { projects(first: $first, after: $after) { nodes { id name state } pageInfo { hasNextPage endCursor } } }";
const LIST_USERS_QUERY = "query($first: Int, $after: String) { users(first: $first, after: $after) { nodes { id name email active } pageInfo { hasNextPage endCursor } } }";

const CREATE_ISSUE_QUERY = `mutation($input: IssueCreateInput!) { issueCreate(input: $input) { success issue { ${ISSUE_FIELDS} } } }`;
const UPDATE_ISSUE_QUERY = `mutation($id: String!, $input: IssueUpdateInput!) { issueUpdate(id: $id, input: $input) { success issue { ${ISSUE_FIELDS} } } }`;
const CREATE_COMMENT_QUERY = `mutation($input: CommentCreateInput!) { commentCreate(input: $input) { success comment { ${COMMENT_FIELDS} } } }`;

// Personal keys use a raw header; OAuth access tokens use Bearer. The
// literal secret name keeps the `secrets.get` capability statically filterable.
function authorize(headers: Map<string, string>): void {
    const token = secrets.get("LINEAR_API_KEY");
    if (token !== null) {
        if (token.startsWith("lin_api_") || token.startsWith("Bearer ")) {
            headers.set("Authorization", token);
        } else {
            headers.set("Authorization", "Bearer " + token);
        }
    }
}

// POST one GraphQL operation, returning the raw response. The endpoint is a
// constant referenced here so `submilli build` pins the http.post host. Generic
// over the variables type `V` — only the value is JSON-encoded, never parsed.
function graphqlPost<V>(query: string, variables: V): Response {
    const headers = new Map<string, string>();
    authorize(headers);
    const response = post(ENDPOINT, { query: query, variables: variables }, headers);
    if (!response.ok) {
        throw new Error(httpFailureMessage(response.status, response.statusText, response.body));
    }
    return response;
}

// The `errors`-only slice of the envelope, parseable without a concrete `data`
// type (a failure body's `data` is null or absent).
interface GraphQlErrorEnvelope {
    errors?: GraphQlError[];
}

// Linear sends a GraphQL `errors` body even on 4xx/5xx (auth failures,
// variable-validation 400s), so render it instead of the bare status line —
// the status alone tells a caller nothing actionable.
/** Render a non-2xx response into one actionable message. Exported for unit tests. */
export function httpFailureMessage(status: number, statusText: string, body: string): string {
    try {
        const envelope = JSON.parse(body) as GraphQlErrorEnvelope;
        const errors = envelope.errors;
        if (errors !== null && errors.length > 0) {
            return graphqlErrorMessage(errors);
        }
    } catch (e) {
        // Not a GraphQL envelope (proxy HTML, truncated body) — fall through.
    }
    return `Linear request failed: HTTP ${status} ${statusText}`;
}

// Surface GraphQL `errors` as a thrown Error, then unwrap the non-null `data`.
// Generic but parse-free, so `T` may be a type parameter here (unlike json<T>).
function requireData<T>(envelope: GraphQlResponse<T>): T {
    const errors = envelope.errors;
    if (errors !== null && errors.length > 0) {
        throw new Error(graphqlErrorMessage(errors));
    }
    const data = envelope.data;
    if (data === null) {
        throw new Error("Linear GraphQL response had no data");
    }
    return data;
}

/** Render Linear's GraphQL `errors` into one actionable message. Exported for unit tests. */
export function graphqlErrorMessage(errors: GraphQlError[]): string {
    const parts: string[] = [];
    for (const err of errors) {
        parts.push(describeGraphqlError(err));
    }
    return `Linear GraphQL error: ${parts.join("; ")}`;
}

function describeGraphqlError(err: GraphQlError): string {
    let detail = err.message;
    const ext = err.extensions;
    if (ext !== null) {
        const code = ext.code;
        if (code !== null) {
            detail = `${detail} [${code}]`;
        }
        const presentable = ext.userPresentableMessage;
        if (presentable !== null) {
            detail = `${detail}: ${presentable}`;
        }
        const args = invalidArguments(ext.validationErrors);
        if (args.length > 0) {
            detail = `${detail} (invalid arguments: ${args.join(", ")})`;
        }
    }
    const path = err.path;
    if (path !== null && path.length > 0) {
        const segments: string[] = [];
        for (const segment of path) {
            if (typeof segment === "string") {
                segments.push(segment);
            } else {
                segments.push(`${segment}`);
            }
        }
        detail = `${detail} at ${segments.join(".")}`;
    }
    return detail;
}

function invalidArguments(validationErrors: ValidationErrorDetail[] | null): string[] {
    const properties: string[] = [];
    if (validationErrors !== null) {
        for (const detail of validationErrors) {
            const property = detail.property;
            if (property !== null) {
                properties.push(property);
            }
        }
    }
    return properties;
}

// Unwrap an issue-mutation payload, surfacing a non-success or missing issue.
function requirePayloadIssue(payload: IssuePayload): Issue {
    const issue = payload.issue;
    if (!payload.success || issue === null) {
        throw new Error("Linear issue mutation did not succeed");
    }
    return issue;
}
