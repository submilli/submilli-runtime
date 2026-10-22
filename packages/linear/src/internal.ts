// The variable builders, GraphQL error renderers and their types behind
// src/lib.ts, plus the two public input types they read (lib.ts re-exports
// those). The helpers stay out of the package API; the package's own tests
// import them from here.

// ---------------------------------------------------------------------------
// Pagination + filtering inputs (re-exported by lib.ts).
// ---------------------------------------------------------------------------

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

/**
 * Translate a curated `IssueFilter` into Linear's nested filter variable.
 * @param filter Curated issue filter; omit it for no filtering.
 * @returns Linear filter variable containing only the supplied constraints; empty when `filter` is omitted. Timestamps lose any bracketed zone annotation.
 */
export function buildIssueFilter(filter?: IssueFilter): IssueFilterVar {
    const out: IssueFilterVar = {};
    if (filter === undefined) {
        return out;
    }
    const teamId = filter.teamId;
    const assigneeId = filter.assigneeId;
    const stateType = filter.stateType;
    const completedAtAfter = filter.completedAtAfter;
    const completedAtBefore = filter.completedAtBefore;
    const createdAtAfter = filter.createdAtAfter;
    const updatedAtAfter = filter.updatedAtAfter;
    if (teamId !== undefined) {
        out.team = { id: { eq: teamId } };
    }
    if (assigneeId !== undefined) {
        out.assignee = { id: { eq: assigneeId } };
    }
    if (stateType !== undefined) {
        out.state = { type: { eq: stateType } };
    }
    if (completedAtAfter !== undefined || completedAtBefore !== undefined) {
        const completedAt: DateComparatorVar = {};
        if (completedAtAfter !== undefined) {
            completedAt.gte = stripZoneAnnotation(completedAtAfter);
        }
        if (completedAtBefore !== undefined) {
            completedAt.lt = stripZoneAnnotation(completedAtBefore);
        }
        out.completedAt = completedAt;
    }
    if (createdAtAfter !== undefined) {
        out.createdAt = { gte: stripZoneAnnotation(createdAtAfter) };
    }
    if (updatedAtAfter !== undefined) {
        out.updatedAt = { gte: stripZoneAnnotation(updatedAtAfter) };
    }
    return out;
}

// Linear rejects `after` unless `first` is also present ("after cannot be used
// without at least one of first") and otherwise defaults the page size to 50, so
// we always send an explicit `first` — a caller paginating with just a cursor
// would otherwise send `{ after }` and every page past the first would fail.
const DEFAULT_PAGE_SIZE = 50;

/**
 * Build the `first`/`after` variables for a paginated list call.
 * @param page Optional pagination; omitting it or its `first` uses the default page size of 50.
 * @returns `first`/`after` variables; `after` is present only when a cursor was given.
 */
export function buildPageVars(page?: PageOptions): PageVars {
    return { first: page?.first ?? DEFAULT_PAGE_SIZE, after: page?.after ?? undefined };
}

/**
 * Build the variables for `listIssues` — pagination plus an optional filter.
 * @param filter Optional issue filter; omitting it adds no filter variable.
 * @param page Optional pagination; omitting it or its `first` uses the default page size of 50.
 * @returns Variables for the issues query: `first`, optional `after`, and optional `filter`.
 */
export function buildListIssuesVars(filter?: IssueFilter, page?: PageOptions): ListIssuesVars {
    const pagination = buildPageVars(page);
    return {
        first: pagination.first,
        after: pagination.after,
        filter: filter === undefined ? undefined : buildIssueFilter(filter),
    };
}

// ---------------------------------------------------------------------------
// GraphQL variable and error shapes.
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
/** A Linear GraphQL error. */
export interface GraphQlError {
    /** The generic top-level message (often just "Argument Validation Error"). */
    message: string;
    /** Field path to the error, e.g. `["issues"]`. */
    path?: GraphQlPathSegment[] | null;
    /** Implementation-specific detail; where Linear hides the actionable cause. */
    extensions?: GraphQlErrorExtensions | null;
}

/** The subset of Linear's GraphQL error `extensions` we surface. */
export interface GraphQlErrorExtensions {
    /** Machine code, e.g. "INVALID_INPUT". */
    code?: string | null;
    /** A human-readable detail Linear sometimes includes. */
    userPresentableMessage?: string | null;
    /** Per-argument validation failures. */
    validationErrors?: ValidationErrorDetail[] | null;
}

/** One class-validator failure inside `extensions.validationErrors`. */
export interface ValidationErrorDetail {
    /** The offending argument/field name, e.g. "after". */
    property?: string | null;
}

// `data` is read concretely per operation (`json() as GraphQlResponse<XData>`):
// a runtime cast can't target an unconstrained generic, so the envelope is cast
// with a concrete data type at each call site, then `requireData` does the
// shared error/null unwrap generically (it only reads fields, never parses).
/** A GraphQL response envelope. */
export interface GraphQlResponse<T> {
    /** The operation result; an error response omits it or sends null. */
    data?: T | null;
    /** GraphQL errors; absent or null when the operation succeeded. */
    errors?: GraphQlError[] | null;
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

// ---------------------------------------------------------------------------
// GraphQL error rendering.
// ---------------------------------------------------------------------------

// The `errors`-only slice of the envelope, parseable without a concrete `data`
// type (a failure body's `data` is null or absent).
interface GraphQlErrorEnvelope {
    errors?: GraphQlError[] | null;
}

// Linear sends a GraphQL `errors` body even on 4xx/5xx (auth failures,
// variable-validation 400s), so render it instead of the bare status line —
// the status alone tells a caller nothing actionable.
/**
 * Render a non-2xx response into one actionable message.
 * @param status HTTP status code of the failed response.
 * @param statusText HTTP status text, used in the fallback message.
 * @param body Raw response body; GraphQL `errors` in it are rendered when present.
 * @returns The rendered GraphQL errors, or `Linear request failed: HTTP <status> <statusText>` when the body has none.
 */
export function httpFailureMessage(status: number, statusText: string, body: string): string {
    try {
        const envelope = JSON.parse(body) as GraphQlErrorEnvelope;
        const errors = envelope.errors;
        if (errors && errors.length > 0) {
            return graphqlErrorMessage(errors);
        }
    } catch (e) {
        // Not a GraphQL envelope (proxy HTML, truncated body) — fall through.
    }
    return `Linear request failed: HTTP ${status} ${statusText}`;
}

/**
 * Render Linear's GraphQL `errors` into one actionable message.
 * @param errors GraphQL errors from a response.
 * @returns One message prefixed `Linear GraphQL error:` with each error's message, code, and details joined by `;`.
 */
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
    const code = ext?.code;
    if (code) {
        detail = `${detail} [${code}]`;
    }
    const presentable = ext?.userPresentableMessage;
    if (presentable) {
        detail = `${detail}: ${presentable}`;
    }
    const args = invalidArguments(ext?.validationErrors);
    if (args.length > 0) {
        detail = `${detail} (invalid arguments: ${args.join(", ")})`;
    }
    const path = err.path;
    if (path && path.length > 0) {
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

function invalidArguments(validationErrors: ValidationErrorDetail[] | null | undefined): string[] {
    const properties: string[] = [];
    if (!validationErrors) return properties;
    for (const detail of validationErrors) {
        const property = detail.property;
        if (property) {
            properties.push(property);
        }
    }
    return properties;
}
