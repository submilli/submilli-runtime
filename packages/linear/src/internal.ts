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
    /** Restrict to issues that carry this label UUID. */
    labelId?: string;
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
    const labelId = filter.labelId;
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
    if (labelId !== undefined) {
        out.labels = { some: { id: { eq: labelId } } };
    }
    return out;
}

// Common words that carry no meaning when comparing two short texts.
const STOP_WORDS = new Set<string>([
    "the", "and", "for", "not", "but", "you", "your", "are", "was", "were", "this", "that", "with",
    "from", "have", "has", "had", "does", "did", "doesn", "didn", "can", "cannot", "could", "would",
    "should", "will", "when", "what", "how", "why", "who", "which", "there", "their", "they", "them",
    "then", "than", "into", "onto", "out", "about", "after", "before", "also", "just", "only", "very",
    "its", "it's", "any", "all", "some", "too", "feedback", "bug", "idea", "praise", "question", "other",
]);

// A letter or digit: ASCII letters and digits, plus non-ASCII characters outside the general
// punctuation block, so accented and non-Latin words count as words.
function isWordChar(code: number): boolean {
    if (code >= 48 && code <= 57) return true;
    if (code >= 97 && code <= 122) return true;
    if (code <= 0xbf) return false; // ASCII and Latin-1 controls, spaces and symbols, including the no-break space
    if (code === 0xd7 || code === 0xf7) return false;
    if (code === 0x1680 || code === 0x180e || code === 0xfeff) return false;
    if (code >= 0x2000 && code <= 0x206f) return false; // general punctuation and Unicode spaces
    if (code >= 0x3000 && code <= 0x303f) return false; // ideographic space and CJK punctuation
    return true;
}

/**
 * Split a text into its distinct significant words for similarity comparison.
 * @param text Any text, such as an issue title.
 * @returns Lowercase words of at least three letters or digits, in first-seen order, without stop words or duplicates.
 */
export function significantWords(text: string): string[] {
    const lower = text.toLowerCase();
    const words: string[] = [];
    const seen = new Set<string>();
    let current = "";
    for (let i = 0; i <= lower.length; i++) {
        const code = i < lower.length ? lower.charCodeAt(i) : 32;
        if (isWordChar(code)) {
            current = current + lower.charAt(i);
            continue;
        }
        if (current.length >= 3 && !STOP_WORDS.has(current) && !seen.has(current)) {
            seen.add(current);
            words.push(current);
        }
        current = "";
    }
    return words;
}

/**
 * Similarity of two word lists: the Dice coefficient, twice the shared words over the total.
 * @param a Distinct words of the first text.
 * @param b Distinct words of the second text.
 * @returns A score from 0 (no shared words, or either list empty) to 1 (the same words).
 */
export function wordSimilarity(a: string[], b: string[]): number {
    if (a.length === 0 || b.length === 0) return 0;
    const inB = new Set<string>();
    for (const word of b) inB.add(word);
    let shared = 0;
    for (const word of a) {
        if (inB.has(word)) shared = shared + 1;
    }
    return (2 * shared) / (a.length + b.length);
}

/** Linear's issue-filter variable for a duplicate search: a team, and a label or a project. */
export interface SimilarIssuesFilterVar {
    /** Team-id equality clause. */
    team: { id: { eq: string } };
    /** Has-label clause, when only a label is given. */
    labels?: { some: { id: { eq: string } } };
    /** Project-id equality clause, when only a project is given. */
    project?: { id: { eq: string } };
    /** Either clause, when both a label and a project are given. */
    or?: SimilarIssuesScopeClause[];
}

/** One alternative of a duplicate-search scope: a label or a project. */
export interface SimilarIssuesScopeClause {
    /** Has-label clause. */
    labels?: { some: { id: { eq: string } } };
    /** Project-id equality clause. */
    project?: { id: { eq: string } };
}

/**
 * Build the filter for a duplicate search: issues in the team that carry the label or belong to
 * the project (either one when only one is given; the whole team when neither is).
 * @param teamId Team UUID.
 * @param labelId Optional label UUID.
 * @param projectId Optional project UUID.
 * @returns Linear filter variable.
 */
export function buildSimilarIssuesFilter(teamId: string, labelId?: string, projectId?: string): SimilarIssuesFilterVar {
    const out: SimilarIssuesFilterVar = { team: { id: { eq: teamId } } };
    if (labelId !== undefined && projectId !== undefined) {
        out.or = [{ labels: { some: { id: { eq: labelId } } } }, { project: { id: { eq: projectId } } }];
    } else if (labelId !== undefined) {
        out.labels = { some: { id: { eq: labelId } } };
    } else if (projectId !== undefined) {
        out.project = { id: { eq: projectId } };
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

// `data` is read concretely per operation (`json<GraphQlResponse<XData>>()`):
// each call site checks the envelope with its concrete data type, then
// `requireData` does the shared error/null unwrap generically (it only reads
// fields, never parses).
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
    /** Has-label clause: at least one label with this id. */
    labels?: { some: { id: { eq: string } } };
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
