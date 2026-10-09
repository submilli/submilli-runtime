import { get, post, put, patch, delete, Response } from "submilli:http";
import { encodeComponent, encodeQuery } from "submilli:url";
import secrets from "submilli:secrets";
import { check } from "submilli:security";

const GITHUB_API = "https://api.github.com/";
const GRAPHQL_API = GITHUB_API + "graphql";
const API_VERSION = "2026-03-10";
const DEFAULT_LIMIT = 30;
const MAX_LIMIT = 100;
const MAX_FILE_BYTES = 10485760;
// The characters GitHub allows in an owner login and in a repository name. A name outside them
// could be read as search syntax or as a second path segment.
const OWNER_NAME = /^[A-Za-z0-9_-]+$/;
const REPOSITORY_NAME = /^[A-Za-z0-9._-]+$/;
// Scope names are refused anywhere outside phrases; relying on GitHub's undocumented
// punctuation/format-character tokenization would leave the repository boundary ambiguous.
const SEARCH_SCOPE = /repo:|org:|user:|owner:/;
const SEARCH_KIND_PREFIX = /(^|[^a-z0-9_])(is|type):$/;
const SEARCH_KIND = /(^|[^a-z0-9_])(is|type):(issue|pr|pull-request)($|[^a-z0-9_-])/;
const SEARCH_OR = /(^|[^a-z0-9_-])or($|[^a-z0-9_-])/;

/** Identifies a repository by owner and name. */
export interface RepositoryRef {
    /** Repository owner login (user or organization). */
    owner: string;
    /** Repository name. */
    name: string;
}

/** Pagination controls for list endpoints. */
export interface PageOptions {
    /** Page size, 1-100 (default 30). */
    limit?: number;
    /** Token from a previous page's `nextPageToken`; omit for the first page. */
    pageToken?: string;
}

/** One page of results from a list endpoint. */
export interface PageResult<T> {
    /** Items on this page. */
    items: T[];
    /** Token for the next page, or "" when this is the last page. */
    nextPageToken: string;
    /** True when there are no further pages. */
    isComplete: boolean;
}

/** One page of results from a search endpoint. */
export interface SearchPageResult<T> {
    /** Items on this page. */
    items: T[];
    /** Token for the next page, or "" when this is the last page. */
    nextPageToken: string;
    /** True when there are no further pages. */
    isComplete: boolean;
    /** Total matches across all pages. */
    totalCount: number;
    /** True when GitHub timed out and returned only partial matches. */
    incompleteResults: boolean;
}

/** Available reaction counts. A missing summary or counter is null, never an assumed zero. */
export interface ReactionSummary {
    /** All reaction types combined. */
    totalCount: number | null;
    /** Thumbs-up count. */
    thumbsUp: number | null;
    /** Thumbs-down count. */
    thumbsDown: number | null;
    /** Laugh count. */
    laugh: number | null;
    /** Hooray count. */
    hooray: number | null;
    /** Confused count. */
    confused: number | null;
    /** Heart count. */
    heart: number | null;
    /** Rocket count. */
    rocket: number | null;
    /** Eyes count. */
    eyes: number | null;
}

/** Issue search metadata together with its repository identity. */
export interface IssueSearchHit {
    /** Repository containing the issue. */
    repository: RepositoryRef;
    /** Issue metadata from the search response. */
    issue: Issue;
}

/** Search metadata without mergeability, diff statistics, head or base details. */
export interface PullRequestSummary extends Issue {
    /** Draft state, or null when search omits it. */
    draft: boolean | null;
}
/** Pull-request search metadata together with its repository identity. */
export interface PullRequestSearchHit {
    /** Repository containing the pull request. */
    repository: RepositoryRef;
    /** Metadata returned by search, without detail requests. */
    pullRequest: PullRequestSummary;
}

/** A research page can be exhausted while GitHub still caps the matching results. */
export interface ResearchPageResult<T> extends SearchPageResult<T> {
    /** More than 1,000 matches; narrow the query to retrieve beyond that ceiling. */
    isCapped: boolean;
}

/** Structured author and inclusive UTC calendar-day creation window. */
export interface ResearchSearchOptions extends SearchOptions {
    /** One login or app/login; folded to lowercase. */
    author?: string;
    /** Inclusive first UTC day, YYYY-MM-DD. */
    createdSince?: string;
    /** Inclusive last UTC day, YYYY-MM-DD. */
    createdUntil?: string;
}

/** A GitHub user account. */
export interface User {
    /** Login (username). */
    login: string;
    /** Numeric account id. */
    id: number;
    /** Web URL of the profile. */
    htmlUrl: string;
    /** Avatar image URL. */
    avatarUrl: string;
    /** Account type, e.g. "User", "Organization", or "Bot". */
    type: string;
}

/** An organization team. */
export interface Team {
    /** Numeric team id. */
    id: number;
    /** Team display name. */
    name: string;
    /** URL-safe team slug, as used in API paths. */
    slug: string;
    /** Team description, or null when unset. */
    description: string | null;
    /** Team visibility, "closed" or "secret". */
    privacy: string;
    /** Web URL of the team page. */
    htmlUrl: string;
    /** Login of the owning organization. */
    organization: string;
}

/** A GitHub repository. */
export interface Repository {
    /** Numeric repository id. */
    id: number;
    /** Repository name. */
    name: string;
    /** "owner/name" form. */
    fullName: string;
    /** Repository description, or null when unset. */
    description: string | null;
    /** Web URL of the repository. */
    htmlUrl: string;
    /** True for private repositories. */
    private: boolean;
    /** True when the repository is a fork. */
    fork: boolean;
    /** True when the repository is archived (read-only). */
    archived: boolean;
    /** Default branch name, e.g. "main". */
    defaultBranch: string;
    /** Primary language, or null when undetected. */
    language: string | null;
    /** Repository topics. */
    topics: string[];
    /** Star count. */
    stargazersCount: number;
    /** Fork count. */
    forksCount: number;
    /** Count of open issues, including open pull requests. */
    openIssuesCount: number;
    /** ISO 8601 creation time. */
    createdAt: string;
    /** ISO 8601 time of the last metadata update. */
    updatedAt: string;
    /** ISO 8601 time of the last push, or null when never pushed. */
    pushedAt: string | null;
}

/** A repository branch. */
export interface Branch {
    /** Branch name. */
    name: string;
    /** Head commit SHA. */
    sha: string;
    /** True when branch protection rules apply. */
    protected: boolean;
}

/** A repository file fetched as raw bytes. */
export interface RepositoryFile {
    /** Path within the repository. */
    path: string;
    /** Git blob SHA. */
    sha: string;
    /** File size in bytes as reported by GitHub. */
    size: number;
    /** Web URL of the file, or null when unavailable. */
    htmlUrl: string | null;
    /** Decoded file content. */
    bytes: Uint8Array;
}

/** A repository file decoded as UTF-8 text. */
export interface RepositoryTextFile {
    /** Path within the repository. */
    path: string;
    /** Git blob SHA. */
    sha: string;
    /** File size in bytes as reported by GitHub. */
    size: number;
    /** Web URL of the file, or null when unavailable. */
    htmlUrl: string | null;
    /** File content decoded as UTF-8. */
    text: string;
}

/** One entry in a repository directory listing. */
export interface DirectoryEntry {
    /** Entry kind: "file", "dir", "symlink", or "submodule". */
    type: string;
    /** Entry name (last path segment). */
    name: string;
    /** Full path within the repository. */
    path: string;
    /** Git object SHA. */
    sha: string;
    /** Size in bytes (0 for directories). */
    size: number;
    /** Web URL of the entry, or null when unavailable. */
    htmlUrl: string | null;
}

/** One entry in a Git tree. */
export interface TreeEntry {
    /** Path relative to the tree root. */
    path: string;
    /** Git file mode, e.g. "100644". */
    mode: string;
    /** Git object kind: "blob", "tree", or "commit". */
    type: string;
    /** Git object SHA. */
    sha: string;
    /** Blob size in bytes, or null for non-blob entries. */
    size: number | null;
}

/** A Git tree listing. */
export interface RepositoryTree {
    /** SHA of the tree itself. */
    sha: string;
    /** True when GitHub truncated the entry list. */
    truncated: boolean;
    /** Tree entries. */
    entries: TreeEntry[];
}

/** Aggregate line counts for a commit. */
export interface CommitStats {
    /** Lines added. */
    additions: number;
    /** Lines deleted. */
    deletions: number;
    /** Additions plus deletions. */
    total: number;
}

/** One file changed by a commit. */
export interface CommitFile {
    /** Path of the file after the change. */
    filename: string;
    /** Change kind, e.g. "added", "modified", "removed", "renamed". */
    status: string;
    /** Lines added in this file. */
    additions: number;
    /** Lines deleted in this file. */
    deletions: number;
    /** Additions plus deletions in this file. */
    changes: number;
    /** Prior path for renames, or null. */
    previousFilename: string | null;
    /** Unified diff for the file; null unless "patch" detail was requested or when GitHub omits it (binary or oversized files). */
    patch: string | null;
}

/** A repository commit. */
export interface Commit {
    /** Commit SHA. */
    sha: string;
    /** Web URL of the commit. */
    htmlUrl: string;
    /** Full commit message. */
    message: string;
    /** GitHub account of the author, or null when not linked to one. */
    author: User | null;
    /** GitHub account of the committer, or null when not linked to one. */
    committer: User | null;
    /** Author name from the Git metadata. */
    authorName: string;
    /** Author email from the Git metadata. */
    authorEmail: string;
    /** ISO 8601 author date. */
    authoredAt: string;
    /** Committer name from the Git metadata. */
    committerName: string;
    /** Committer email from the Git metadata. */
    committerEmail: string;
    /** ISO 8601 commit date. */
    committedAt: string;
    /** Aggregate line counts; null with "none" detail or when GitHub omits them. */
    stats: CommitStats | null;
    /** Changed files; empty with "none" detail. */
    files: CommitFile[];
}

/** A repository release. */
export interface Release {
    /** Numeric release id. */
    id: number;
    /** Git tag the release points at. */
    tagName: string;
    /** Release title, or null when unset. */
    name: string | null;
    /** Release notes (Markdown), or null when unset. */
    body: string | null;
    /** Web URL of the release. */
    htmlUrl: string;
    /** True for unpublished drafts. */
    draft: boolean;
    /** True when marked as a prerelease. */
    prerelease: boolean;
    /** ISO 8601 creation time. */
    createdAt: string;
    /** ISO 8601 publish time, or null for drafts. */
    publishedAt: string | null;
    /** User who created the release. */
    author: User;
}

/** An issue and pull request label. */
export interface Label {
    /** Numeric label id. */
    id: number;
    /** Label name. */
    name: string;
    /** 6-digit hex color, without the leading "#". */
    color: string;
    /** Label description, or null when unset. */
    description: string | null;
    /** True for GitHub's default labels. */
    default: boolean;
}

/** An issue milestone. */
export interface Milestone {
    /** Numeric milestone id. */
    id: number;
    /** Milestone number within the repository. */
    number: number;
    /** Milestone title. */
    title: string;
    /** Milestone description, or null when unset. */
    description: string | null;
    /** "open" or "closed". */
    state: string;
    /** ISO 8601 due date, or null when unset. */
    dueOn: string | null;
}

/** A comment on an issue or pull request. */
export interface Comment {
    /** Numeric comment id. */
    id: number;
    /** Comment body (Markdown). */
    body: string;
    /** Web URL of the comment. */
    htmlUrl: string;
    /** Comment author. */
    user: User;
    /** Author's relation to the repository, e.g. "OWNER", "MEMBER", "CONTRIBUTOR", "NONE". */
    authorAssociation: string;
    /** ISO 8601 creation time. */
    createdAt: string;
    /** ISO 8601 time of the last edit. */
    updatedAt: string;
    /** Reaction summary, or null when the endpoint does not provide it. */
    reactions?: ReactionSummary | null;
}

/** A repository issue. */
export interface Issue {
    /** Numeric issue id (global database id, not the issue number). */
    id: number;
    /** Issue number within the repository. */
    number: number;
    /** Issue title. */
    title: string;
    /** Issue body (Markdown), or null when unset. */
    body: string | null;
    /** "open" or "closed" (always lowercase). */
    state: string;
    /** Reason for the current state ("completed", "not_planned", "reopened"), or null. */
    stateReason: string | null;
    /** True when the conversation is locked. */
    locked: boolean;
    /** Web URL of the issue. */
    htmlUrl: string;
    /** Issue author. */
    user: User;
    /** Labels currently applied. */
    labels: Label[];
    /** Assigned users. */
    assignees: User[];
    /** Milestone, or null when unset. */
    milestone: Milestone | null;
    /** Comment count. */
    comments: number;
    /** ISO 8601 creation time. */
    createdAt: string;
    /** ISO 8601 time of the last update. */
    updatedAt: string;
    /** ISO 8601 close time, or null while open. */
    closedAt: string | null;
    /** Reaction summary, or null when the endpoint does not provide it. */
    reactions?: ReactionSummary | null;
}

/** One side (head or base) of a pull request. */
export interface PullRequestRef {
    /** "owner:branch" form. */
    label: string;
    /** Branch name. */
    ref: string;
    /** Commit SHA of this side. */
    sha: string;
    /** Owner of the repository holding the branch. */
    user: User;
    /** Repository holding the branch, or null when it is no longer available (e.g. a deleted fork). */
    repository: Repository | null;
}

/** A pull request. */
export interface PullRequest {
    /** Numeric pull request id (global database id, not the number). */
    id: number;
    /** Pull request number within the repository. */
    number: number;
    /** Pull request title. */
    title: string;
    /** Pull request body (Markdown), or null when unset. */
    body: string | null;
    /** "open" or "closed"; merged pull requests are "closed" with `merged` true. */
    state: string;
    /** True for draft pull requests. */
    draft: boolean;
    /** True when the pull request has been merged. */
    merged: boolean;
    /** Whether it can be merged cleanly; null while GitHub is still computing it. */
    mergeable: boolean | null;
    /** Merge readiness, e.g. "clean", "dirty", "blocked", "unknown". */
    mergeableState: string;
    /** Web URL of the pull request. */
    htmlUrl: string;
    /** Pull request author. */
    user: User;
    /** Labels currently applied. */
    labels: Label[];
    /** Assigned users. */
    assignees: User[];
    /** Users whose review has been requested. */
    requestedReviewers: User[];
    /** Source side (the branch carrying the changes). */
    head: PullRequestRef;
    /** Target side (the branch to merge into). */
    base: PullRequestRef;
    /** Lines added across the pull request. */
    additions: number;
    /** Lines deleted across the pull request. */
    deletions: number;
    /** Number of files changed. */
    changedFiles: number;
    /** Number of commits. */
    commits: number;
    /** Number of general (issue-style) comments. */
    comments: number;
    /** Number of inline review comments. */
    reviewComments: number;
    /** ISO 8601 creation time. */
    createdAt: string;
    /** ISO 8601 time of the last update. */
    updatedAt: string;
    /** ISO 8601 close time, or null while open. */
    closedAt: string | null;
    /** ISO 8601 merge time, or null when not merged. */
    mergedAt: string | null;
    /** Reaction summary, or null when the endpoint does not provide it. */
    reactions?: ReactionSummary | null;
}

/** One file changed by a pull request. */
export interface PullRequestFile {
    /** Blob SHA of the file's new content. */
    sha: string;
    /** Path of the file after the change. */
    filename: string;
    /** Change kind, e.g. "added", "modified", "removed", "renamed". */
    status: string;
    /** Lines added in this file. */
    additions: number;
    /** Lines deleted in this file. */
    deletions: number;
    /** Additions plus deletions in this file. */
    changes: number;
    /** Prior path for renames, or null. */
    previousFilename: string | null;
    /** Unified diff for the file, or null when GitHub omits it (binary or oversized files). */
    patch: string | null;
    /** Web URL of the file blob at the head commit. */
    blobUrl: string;
}

/** A pull request review. */
export interface PullRequestReview {
    /** Numeric review id. */
    id: number;
    /** Review state, e.g. "APPROVED", "CHANGES_REQUESTED", "COMMENTED", "PENDING". */
    state: string;
    /** Review summary text (Markdown). */
    body: string;
    /** Web URL of the review. */
    htmlUrl: string;
    /** Reviewer. */
    user: User;
    /** Commit SHA the review applies to. */
    commitId: string;
    /** ISO 8601 submission time, or null for pending reviews. */
    submittedAt: string | null;
    /** Reviewer's relation to the repository, e.g. "OWNER", "MEMBER", "CONTRIBUTOR". */
    authorAssociation: string;
}

/** Outcome of merging a pull request. */
export interface MergeResult {
    /** True when the merge succeeded. */
    merged: boolean;
    /** Human-readable result message from GitHub. */
    message: string;
    /** SHA of the resulting merge commit. */
    sha: string;
}

/** Rate-limit state for one API resource. */
export interface RateLimitResource {
    /** Requests allowed per window. */
    limit: number;
    /** Requests remaining in the current window. */
    remaining: number;
    /** Requests already used in the current window. */
    used: number;
    /** When the window resets: Unix epoch seconds as a string, or "" when unknown. */
    resetAt: string;
}

/** Rate-limit state across GitHub API resources. */
export interface RateLimit {
    /** REST API requests. */
    core: RateLimitResource;
    /** Search API requests. */
    search: RateLimitResource;
    /** GraphQL API requests. */
    graphql: RateLimitResource;
}

/** One repository search result. */
export interface RepositorySearchHit {
    /** Relevance score assigned by GitHub. */
    score: number;
    /** The matched repository. */
    repository: Repository;
}

/** One code search result. */
export interface CodeSearchHit {
    /** File name (last path segment). */
    name: string;
    /** File path within the repository. */
    path: string;
    /** Blob SHA of the matched file. */
    sha: string;
    /** Web URL of the matched file. */
    htmlUrl: string;
    /** Repository containing the file. */
    repository: Repository;
}

/** One user search result. */
export interface UserSearchHit {
    /** Relevance score assigned by GitHub. */
    score: number;
    /** The matched user. */
    user: User;
}

/** Options for reading a repository file. */
export interface FileReadOptions {
    /** Branch, tag, or commit SHA to read from; the default branch when omitted. */
    ref?: string;
    /** Reject files larger than this many bytes; 1 to 10485760, which is also the default. */
    maxBytes?: number;
}

/** Options for listing a repository directory. */
export interface DirectoryOptions {
    /** Branch, tag, or commit SHA to list from; the default branch when omitted. */
    ref?: string;
}

/** Per-commit detail level: "none" omits stats and files, "stats" adds stats and the file list without diffs, "patch" also includes per-file diffs. */
export type CommitDetail = "none" | "stats" | "patch";

/** Filters and pagination for listing commits. */
export interface ListCommitsOptions {
    /** Branch name or commit SHA to start listing from; the default branch when omitted. */
    sha?: string;
    /** Only commits touching this file or directory path. */
    path?: string;
    /** Only commits by this GitHub login or email address. */
    author?: string;
    /** Only commits after this ISO 8601 timestamp; bracketed Temporal strings are normalized to UTC. */
    since?: string;
    /** Only commits before this ISO 8601 timestamp; bracketed Temporal strings are normalized to UTC. */
    until?: string;
    /** Page size, 1-100 (default 30). */
    limit?: number;
    /** Token from a previous page's `nextPageToken`; omit for the first page. */
    pageToken?: string;
}

/** One file operation inside a commit. */
export interface CommitFileChange {
    /** "write" creates or replaces the file; "delete" removes it. */
    type: "write" | "delete";
    /** Repository-relative file path (no leading slash, no "." or ".." segments). */
    path: string;
    /** New content, UTF-8 text or raw bytes; required for "write", forbidden for "delete". */
    content?: string | Uint8Array;
}

/** Input for committing a batch of file changes to a branch. */
export interface CommitFilesInput {
    /** Target branch name (short name, without "refs/heads/"). */
    branch: string;
    /** Current head commit SHA of the branch; the commit is rejected with "branch_moved" if the branch has advanced. */
    expectedHeadSha: string;
    /** Commit message. */
    message: string;
    /** File writes and deletions; aggregate written content must stay within 10485760 bytes. */
    changes: CommitFileChange[];
}

/** Input for creating a branch. */
export interface CreateBranchInput {
    /** New branch name (short name, without "refs/heads/"). */
    name: string;
    /** Commit SHA the branch starts at (40- or 64-character hex). */
    fromSha: string;
}

/** Filters and pagination for listing issues. */
export interface ListIssuesOptions {
    /** Filter by state; both states when omitted. */
    state?: "OPEN" | "CLOSED";
    /** Only issues carrying these label names. */
    labels?: string[];
    /** Sort field (default UPDATED_AT). */
    orderBy?: "CREATED_AT" | "UPDATED_AT" | "COMMENTS";
    /** Sort direction (default DESC). */
    direction?: "ASC" | "DESC";
    /** Page size, 1-100 (default 30). */
    limit?: number;
    /** Cursor from a previous page's `nextPageToken`; omit for the first page. */
    pageToken?: string;
}

/** Sorting and pagination for search endpoints. */
export interface SearchOptions {
    /** Sort field, endpoint-specific (e.g. "stars", "created", "updated"); best-match relevance when omitted. */
    sort?: string;
    /** Sort order, "asc" or "desc". */
    order?: "asc" | "desc";
    /** Page size, 1-100 (default 30). */
    limit?: number;
    /** Token from a previous page's `nextPageToken`; omit for the first page. */
    pageToken?: string;
}

/** Input for creating an issue. */
export interface CreateIssueInput {
    /** Issue title. */
    title: string;
    /** Issue body (Markdown). */
    body?: string;
    /** Logins of users to assign. */
    assignees?: string[];
    /** Label names to apply. */
    labels?: string[];
    /** Milestone number to associate. */
    milestone?: number;
}

/** Input for updating an issue; only supplied fields change, and at least one must be set. */
export interface UpdateIssueInput {
    /** New title. */
    title?: string;
    /** New body (Markdown). */
    body?: string;
    /** True to remove the body. */
    clearBody?: boolean;
    /** Replacement assignee logins. */
    assignees?: string[];
    /** Replacement label names. */
    labels?: string[];
    /** Milestone number to set. */
    milestone?: number;
    /** True to remove the milestone. */
    clearMilestone?: boolean;
    /** New state. */
    state?: "open" | "closed";
    /** Reason recorded with the state change. */
    stateReason?: "completed" | "not_planned" | "reopened";
}

/** Filters and pagination for listing pull requests. */
export interface ListPullRequestsOptions {
    /** Filter by state (default "open"). */
    state?: "open" | "closed" | "all";
    /** Filter by head branch in "user:branch" form. */
    head?: string;
    /** Filter by base branch name. */
    base?: string;
    /** Sort field (default "created"). */
    sort?: "created" | "updated" | "popularity" | "long-running";
    /** Sort direction. */
    direction?: "asc" | "desc";
    /** Page size, 1-100 (default 30). */
    limit?: number;
    /** Token from a previous page's `nextPageToken`; omit for the first page. */
    pageToken?: string;
}

/** Input for creating a pull request. */
export interface CreatePullRequestInput {
    /** Pull request title. */
    title: string;
    /** Branch carrying the changes; use "owner:branch" for cross-fork pull requests. */
    head: string;
    /** Branch to merge into. */
    base: string;
    /** Pull request body (Markdown). */
    body?: string;
    /** True to open as a draft. */
    draft?: boolean;
    /** Allow maintainers of the base repository to push to the head branch. */
    maintainerCanModify?: boolean;
}

/** Input for updating a pull request; only supplied fields change, and at least one must be set. */
export interface UpdatePullRequestInput {
    /** New title. */
    title?: string;
    /** New body (Markdown). */
    body?: string;
    /** True to remove the body. */
    clearBody?: boolean;
    /** New base branch name. */
    base?: string;
    /** New state: "closed" closes, "open" reopens. */
    state?: "open" | "closed";
    /** Allow maintainers of the base repository to push to the head branch. */
    maintainerCanModify?: boolean;
}

/** Options for merging a pull request. */
export interface MergePullRequestInput {
    /** Title for the merge commit; GitHub's default when omitted. */
    commitTitle?: string;
    /** Extra body text for the merge commit. */
    commitMessage?: string;
    /** Merge strategy; the repository's default when omitted. */
    method?: "merge" | "squash" | "rebase";
    /** Only merge if the head commit still matches this SHA. */
    expectedHeadSha?: string;
}

/** One inline comment attached to a pull request review. */
export interface ReviewCommentInput {
    /** Repository-relative path of the file being commented on. */
    path: string;
    /** Comment text (Markdown). */
    body: string;
    /** Line number the comment applies to (the last line of a multi-line comment). */
    line: number;
    /** Diff side: "LEFT" for the old version, "RIGHT" for the new. */
    side: "LEFT" | "RIGHT";
    /** First line of a multi-line comment; must not exceed `line`. */
    startLine?: number;
    /** Diff side of `startLine`; required when `startLine` is set. */
    startSide?: "LEFT" | "RIGHT";
}

/** Input for creating a pull request review. */
export interface CreateReviewInput {
    /** Review summary text (Markdown); required for REQUEST_CHANGES. */
    body?: string;
    /** Review action to take. */
    event: "APPROVE" | "REQUEST_CHANGES" | "COMMENT";
    /** Commit SHA the review applies to; the head commit when omitted. */
    commitId?: string;
    /** Inline comments to attach. */
    comments?: ReviewCommentInput[];
}

/** A GitHub API or package validation error with retry and rate-limit metadata. */
export class GitHubError extends Error {
    code: string;
    status: number;
    requestId: string;
    documentationUrl: string;
    retryAfterSeconds: number | null;
    rateLimitRemaining: number | null;
    rateLimitReset: string | null;
    errors: string[];

    constructor(
        code: string,
        message: string,
        status: number,
        requestId: string,
        documentationUrl: string,
        retryAfterSeconds: number | null,
        rateLimitRemaining: number | null,
        rateLimitReset: string | null,
        errors: string[],
    ) {
        super(message);
        this.name = "GitHubError";
        this.code = code;
        this.status = status;
        this.requestId = requestId;
        this.documentationUrl = documentationUrl;
        this.retryAfterSeconds = retryAfterSeconds;
        this.rateLimitRemaining = rateLimitRemaining;
        this.rateLimitReset = rateLimitReset;
        this.errors = errors;
    }
}

interface ApiUser {
    login?: string | null;
    id?: number | null;
    html_url?: string | null;
    avatar_url?: string | null;
    type?: string | null;
}

interface ApiOwner {
    login?: string | null;
}

interface ApiRepository {
    id?: number | null;
    name?: string | null;
    full_name?: string | null;
    description?: string | null;
    html_url?: string | null;
    private?: boolean | null;
    fork?: boolean | null;
    archived?: boolean | null;
    default_branch?: string | null;
    language?: string | null;
    topics?: string[] | null;
    stargazers_count?: number | null;
    forks_count?: number | null;
    open_issues_count?: number | null;
    created_at?: string | null;
    updated_at?: string | null;
    pushed_at?: string | null;
}

interface ApiBranchCommit {
    sha?: string | null;
}

interface ApiBranch {
    name?: string | null;
    commit?: ApiBranchCommit | null;
    protected?: boolean | null;
}

interface ApiContent {
    type?: string | null;
    name?: string | null;
    path?: string | null;
    sha?: string | null;
    size?: number | null;
    html_url?: string | null;
    encoding?: string | null;
    content?: string | null;
}

interface ApiTreeEntry {
    path?: string | null;
    mode?: string | null;
    type?: string | null;
    sha?: string | null;
    size?: number | null;
}

interface ApiTree {
    sha?: string | null;
    truncated?: boolean | null;
    tree?: ApiTreeEntry[] | null;
}

interface ApiGitActor {
    name?: string | null;
    email?: string | null;
    date?: string | null;
}

interface ApiCommitBody {
    message?: string | null;
    author?: ApiGitActor | null;
    committer?: ApiGitActor | null;
    tree?: ApiBranchCommit | null;
}

interface ApiCommitStats {
    additions?: number | null;
    deletions?: number | null;
    total?: number | null;
}

interface ApiCommitFile {
    filename?: string | null;
    status?: string | null;
    additions?: number | null;
    deletions?: number | null;
    changes?: number | null;
    previous_filename?: string | null;
    patch?: string | null;
}

interface ApiCommit {
    sha?: string | null;
    html_url?: string | null;
    commit?: ApiCommitBody | null;
    author?: ApiUser | null;
    committer?: ApiUser | null;
    stats?: ApiCommitStats | null;
    files?: ApiCommitFile[] | null;
}

interface ApiRelease {
    id?: number | null;
    tag_name?: string | null;
    name?: string | null;
    body?: string | null;
    html_url?: string | null;
    draft?: boolean | null;
    prerelease?: boolean | null;
    created_at?: string | null;
    published_at?: string | null;
    author?: ApiUser | null;
}

interface ApiLabel {
    id?: number | null;
    name?: string | null;
    color?: string | null;
    description?: string | null;
    default?: boolean | null;
}

interface ApiMilestone {
    id?: number | null;
    number?: number | null;
    title?: string | null;
    description?: string | null;
    state?: string | null;
    due_on?: string | null;
}

interface ApiReactionSummary {
    total_count?: number | null;
    "+1"?: number | null;
    "-1"?: number | null;
    laugh?: number | null;
    hooray?: number | null;
    confused?: number | null;
    heart?: number | null;
    rocket?: number | null;
    eyes?: number | null;
}

interface ApiComment {
    id?: number | null;
    body?: string | null;
    html_url?: string | null;
    user?: ApiUser | null;
    author_association?: string | null;
    created_at?: string | null;
    updated_at?: string | null;
    reactions?: ApiReactionSummary | null;
}

interface ApiIssue {
    id?: number | null;
    number?: number | null;
    title?: string | null;
    body?: string | null;
    state?: string | null;
    state_reason?: string | null;
    locked?: boolean | null;
    html_url?: string | null;
    user?: ApiUser | null;
    labels?: ApiLabel[] | null;
    assignees?: ApiUser[] | null;
    milestone?: ApiMilestone | null;
    comments?: number | null;
    created_at?: string | null;
    updated_at?: string | null;
    closed_at?: string | null;
    pull_request?: unknown;
    reactions?: ApiReactionSummary | null;
    repository_url?: string | null;
    draft?: boolean | null;
}

interface ApiPullRef {
    label?: string | null;
    ref?: string | null;
    sha?: string | null;
    user?: ApiUser | null;
    repo?: ApiRepository | null;
}

interface ApiPullRequest {
    id?: number | null;
    number?: number | null;
    title?: string | null;
    body?: string | null;
    state?: string | null;
    draft?: boolean | null;
    merged?: boolean | null;
    mergeable?: boolean | null;
    mergeable_state?: string | null;
    html_url?: string | null;
    user?: ApiUser | null;
    labels?: ApiLabel[] | null;
    assignees?: ApiUser[] | null;
    requested_reviewers?: ApiUser[] | null;
    head?: ApiPullRef | null;
    base?: ApiPullRef | null;
    additions?: number | null;
    deletions?: number | null;
    changed_files?: number | null;
    commits?: number | null;
    comments?: number | null;
    review_comments?: number | null;
    created_at?: string | null;
    updated_at?: string | null;
    closed_at?: string | null;
    merged_at?: string | null;
    reactions?: ApiReactionSummary | null;
}

interface ApiPullFile {
    sha?: string | null;
    filename?: string | null;
    status?: string | null;
    additions?: number | null;
    deletions?: number | null;
    changes?: number | null;
    previous_filename?: string | null;
    patch?: string | null;
    blob_url?: string | null;
}

interface ApiReview {
    id?: number | null;
    state?: string | null;
    body?: string | null;
    html_url?: string | null;
    user?: ApiUser | null;
    commit_id?: string | null;
    submitted_at?: string | null;
    author_association?: string | null;
}

interface ApiMergeResult {
    merged?: boolean | null;
    message?: string | null;
    sha?: string | null;
}

interface ApiTeam {
    id?: number | null;
    name?: string | null;
    slug?: string | null;
    description?: string | null;
    privacy?: string | null;
    html_url?: string | null;
    organization?: ApiOwner | null;
}

interface ApiRateResource {
    limit?: number | null;
    remaining?: number | null;
    used?: number | null;
    reset?: number | null;
}

interface ApiRateResources {
    core?: ApiRateResource | null;
    search?: ApiRateResource | null;
    graphql?: ApiRateResource | null;
}

interface ApiRateLimit {
    resources?: ApiRateResources | null;
}

interface ApiSearchRepositoryItem {
    id?: number | null;
    name?: string | null;
    full_name?: string | null;
    description?: string | null;
    html_url?: string | null;
    private?: boolean | null;
    fork?: boolean | null;
    archived?: boolean | null;
    default_branch?: string | null;
    language?: string | null;
    topics?: string[] | null;
    stargazers_count?: number | null;
    forks_count?: number | null;
    open_issues_count?: number | null;
    created_at?: string | null;
    updated_at?: string | null;
    pushed_at?: string | null;
    score?: number | null;
}

interface ApiCodeSearchItem {
    name?: string | null;
    path?: string | null;
    sha?: string | null;
    html_url?: string | null;
    repository?: ApiRepository | null;
}

interface ApiUserSearchItem {
    login?: string | null;
    id?: number | null;
    html_url?: string | null;
    avatar_url?: string | null;
    type?: string | null;
    score?: number | null;
}

interface ApiSearchRepositories {
    total_count?: number | null;
    incomplete_results?: boolean | null;
    items?: ApiSearchRepositoryItem[] | null;
}

interface ApiSearchCode {
    total_count?: number | null;
    incomplete_results?: boolean | null;
    items?: ApiCodeSearchItem[] | null;
}

interface ApiSearchUsers {
    total_count?: number | null;
    incomplete_results?: boolean | null;
    items?: ApiUserSearchItem[] | null;
}

interface ApiSearchIssues {
    total_count?: number | null;
    incomplete_results?: boolean | null;
    items?: ApiIssue[] | null;
}

interface ApiError {
    message?: string | null;
    documentation_url?: string | null;
    status?: string | null;
    errors?: unknown[] | null;
}

interface ApiErrorDetail {
    resource?: string | null;
    field?: string | null;
    code?: string | null;
    message?: string | null;
}

interface GraphQlIssueVariables {
    owner: string;
    name: string;
    first: number;
    after: string | null;
    states: string[] | null;
    labels: string[] | null;
    orderField: string;
    direction: string;
}

interface GraphQlIssueConnection {
    nodes?: ApiGraphQlIssue[] | null;
    pageInfo?: GraphQlPageInfo | null;
}

interface ApiGraphQlCount {
    totalCount?: number | null;
}

interface ApiGraphQlLabels {
    nodes?: ApiLabel[] | null;
}

interface ApiGraphQlUsers {
    nodes?: ApiUser[] | null;
}

interface ApiGraphQlIssue {
    id?: number | null;
    number?: number | null;
    title?: string | null;
    body?: string | null;
    state?: string | null;
    state_reason?: string | null;
    locked?: boolean | null;
    html_url?: string | null;
    user?: ApiUser | null;
    labels?: ApiGraphQlLabels | null;
    assignees?: ApiGraphQlUsers | null;
    milestone?: ApiMilestone | null;
    comments?: ApiGraphQlCount | null;
    created_at?: string | null;
    updated_at?: string | null;
    closed_at?: string | null;
}

interface GraphQlRepository {
    issues?: GraphQlIssueConnection | null;
}

interface GraphQlIssueData {
    repository?: GraphQlRepository | null;
}

interface GraphQlPageInfo {
    hasNextPage?: boolean | null;
    endCursor?: string | null;
}

interface GraphQlErrorItem {
    message?: string | null;
}

// `data` is read only when there are no errors: with errors it may be null or partial.
interface GraphQlEnvelope {
    data?: unknown;
    errors?: GraphQlErrorItem[] | null;
}

interface GitBlobBody {
    content: string;
    encoding: string;
}

interface ApiCreatedSha {
    sha?: string | null;
}

interface ApiGitCommit {
    tree?: ApiBranchCommit | null;
}

interface ApiGitRefObject {
    sha?: string | null;
}

interface ApiGitRef {
    object?: ApiGitRefObject | null;
}

interface GitTreeItemBody {
    path: string;
    mode: string;
    type: string;
    sha: string | null;
}

interface GitTreeBody {
    base_tree: string;
    tree: GitTreeItemBody[];
}

interface GitCommitBody {
    message: string;
    tree: string;
    parents: string[];
}

interface GitRefBody {
    ref: string;
    sha: string;
}

interface GitUpdateRefBody {
    sha: string;
    force: boolean;
}

// Every field the capabilities declare is sent, null when the operation does not name it: a check
// omits an undefined field, and a filter such as `base == null` does not match a missing one.
interface RepositoryCapabilityContext {
    owner: string;
    repo: string;
    branch: string | null;
    path: string | null;
    ref: string | null;
    treeSha: string | null;
    head: string | null;
    base: string | null;
}

interface RepositoryNumberCapabilityContext {
    owner: string;
    repo: string;
    number: number;
    base: string | null;
}

const ISSUE_QUERY = "query($owner:String!,$name:String!,$first:Int!,$after:String,$states:[IssueState!],$labels:[String!],$orderField:IssueOrderField!,$direction:OrderDirection!){repository(owner:$owner,name:$name){issues(first:$first,after:$after,states:$states,labels:$labels,orderBy:{field:$orderField,direction:$direction}){nodes{id:databaseId number title body state state_reason:stateReason locked html_url:url comments{totalCount} created_at:createdAt updated_at:updatedAt closed_at:closedAt user:author{login html_url:url avatar_url:avatarUrl type:__typename}labels(first:100){nodes{name color description}}assignees(first:100){nodes{login id:databaseId html_url:url avatar_url:avatarUrl type:__typename}}milestone{number title description state due_on:dueOn}}pageInfo{hasNextPage endCursor}}}}";

/** Retrieve the authenticated GitHub user.
 * @returns The authenticated user.
 * @capability github.com/viewer.get {}
 */
export function getViewer(): User {
    check("github.com/viewer.get", {});
    return userFrom(githubGet("/user").json() as ApiUser);
}

/** List teams visible to the authenticated user.
 * @param options Page size and `pageToken`; omit it for the API defaults (first page, 30 items).
 * @returns One page of teams the authenticated user belongs to; `nextPageToken` is "" on the last page.
 * @capability github.com/teams.list {}
 */
export function listTeams(options: PageOptions = {}): PageResult<Team> {
    check("github.com/teams.list", {});
    const response = githubGet("/user/teams", pageQuery(options));
    const data = response.json() as ApiTeam[];
    const items: Team[] = [];
    for (const item of data) items.push(teamFrom(item));
    return pageFrom(response, items);
}

/** List members of an organization team.
 * @param org Organization login that owns the team.
 * @param teamSlug Team slug (the URL form of the team name).
 * @param options Page size and `pageToken`; omit it for the API defaults (first page, 30 items).
 * @returns One page of team members; `nextPageToken` is "" on the last page.
 * @capability github.com/teamMembers.list { org: string, teamSlug: string }
 */
export function listTeamMembers(org: string, teamSlug: string, options: PageOptions = {}): PageResult<User> {
    check("github.com/teamMembers.list", { org: org, teamSlug: teamSlug });
    const path = "/orgs/" + segment(org, "organization") + "/teams/" + segment(teamSlug, "team slug") + "/members";
    const response = githubGet(path, pageQuery(options));
    const data = response.json() as ApiUser[];
    const items: User[] = [];
    for (const item of data) items.push(userFrom(item));
    return pageFrom(response, items);
}

/** Search repositories visible to the token.
 * @param query Non-empty GitHub search query for repositories, which may use qualifiers such as `language:rust`.
 * @param options Sort field, order, page size and `pageToken`; omit it for best-match order, the first page and 30 items.
 * @returns One page of repository hits, each with a relevance `score`, plus `totalCount` and `incompleteResults`; `nextPageToken` is "" on the last page.
 * @capability github.com/repositories.search {}
 */
export function searchRepositories(query: string, options: SearchOptions = {}): SearchPageResult<RepositorySearchHit> {
    check("github.com/repositories.search", {});
    requireText(query, "query");
    const response = githubGet("/search/repositories", searchQuery(query, options));
    const data = response.json() as ApiSearchRepositories;
    const items: RepositorySearchHit[] = [];
    for (const item of array(data.items)) {
        items.push({ score: num(item.score), repository: repositoryFrom(item) });
    }
    return searchPageFrom(response, items, data.total_count, data.incomplete_results);
}

/** Search code within one repository.
 * @param repository Owner and name of the repository.
 * @param query Non-empty GitHub search query for code, which may use qualifiers such as `language:rust`. The search is scoped to this repository automatically; `repo:`, `org:`, `user:`, `is:issue`, `is:pr` and `OR` are rejected.
 * @param options Sort field, order, page size and `pageToken`; omit it for best-match order, the first page and 30 items.
 * @returns One page of code hits, each with file name, path, SHA and repository, plus `totalCount` and `incompleteResults`; `nextPageToken` is "" on the last page.
 * @capability github.com/code.search { owner: string, repo: string, branch: string, path: string, ref: string, treeSha: string, head: string, base: string }
 */
export function searchCode(repository: RepositoryRef, query: string, options: SearchOptions = {}): SearchPageResult<CodeSearchHit> {
    const { owner: requestedOwner, name: requestedName } = repository;
    const { owner, name } = canonicalRepository(requestedOwner, requestedName);
    const context = checkedRepository(owner, name);
    check("github.com/code.search", context);
    const scoped = scopedSearch(query, owner, name, "");
    const response = githubGet("/search/code", searchQuery(scoped, options));
    const data = response.json() as ApiSearchCode;
    const items: CodeSearchHit[] = [];
    for (const item of array(data.items)) {
        items.push({
            name: str(item.name),
            path: str(item.path),
            sha: str(item.sha),
            htmlUrl: str(item.html_url),
            repository: repositoryFrom(required(item.repository, "search result repository")),
        });
    }
    return searchPageFrom(response, items, data.total_count, data.incomplete_results);
}

/** Search GitHub users.
 * @param query Non-empty GitHub search query for users, which may use qualifiers such as `followers:>10`.
 * @param options Sort field, order, page size and `pageToken`; omit it for best-match order, the first page and 30 items.
 * @returns One page of user hits, each with a relevance `score`, plus `totalCount` and `incompleteResults`; `nextPageToken` is "" on the last page.
 * @capability github.com/users.search {}
 */
export function searchUsers(query: string, options: SearchOptions = {}): SearchPageResult<UserSearchHit> {
    check("github.com/users.search", {});
    requireText(query, "query");
    const response = githubGet("/search/users", searchQuery(query, options));
    const data = response.json() as ApiSearchUsers;
    const items: UserSearchHit[] = [];
    for (const item of array(data.items)) items.push({ score: num(item.score), user: userFrom(item) });
    return searchPageFrom(response, items, data.total_count, data.incomplete_results);
}

/** Retrieve current REST, search, and GraphQL rate limits.
 * @returns Current limit, remaining calls and reset time for the REST core, search and GraphQL resources.
 * @capability github.com/rateLimit.get {}
 */
export function getRateLimit(): RateLimit {
    check("github.com/rateLimit.get", {});
    const data = githubGet("/rate_limit").json() as ApiRateLimit;
    const resources = required(data.resources, "rate limit resources");
    return {
        core: rateResourceFrom(resources.core),
        search: rateResourceFrom(resources.search),
        graphql: rateResourceFrom(resources.graphql),
    };
}

/** Retrieve repository metadata, or null when it does not exist.
 * @param repository Owner and name of the repository.
 * @returns The repository metadata, or `null` when it does not exist or is not visible to the token.
 * @capability github.com/repositories.get { owner: string, repo: string, branch: string, path: string, ref: string, treeSha: string, head: string, base: string }
 */
export function getRepository(repository: RepositoryRef): Repository | null {
    const { owner: requestedOwner, name: requestedName } = repository;
    const { owner, name } = canonicalRepository(requestedOwner, requestedName);
    const context = checkedRepository(owner, name);
    check("github.com/repositories.get", context);
    const response = githubGetNullable(repositoryPath(owner, name));
    return response === null ? null : repositoryFrom(response.json() as ApiRepository);
}

/** Retrieve a branch, or null when it does not exist.
 * @param repository Owner and name of the repository.
 * @param branch Branch name.
 * @returns The branch with its head commit SHA, or `null` when it does not exist.
 * @capability github.com/branches.get { owner: string, repo: string, branch: string, path: string, ref: string, treeSha: string, head: string, base: string }
 */
export function getBranch(repository: RepositoryRef, branch: string): Branch | null {
    const { owner: requestedOwner, name: requestedName } = repository;
    const { owner, name } = canonicalRepository(requestedOwner, requestedName);
    const context = repositoryContext(owner, name, "branch", branch);
    check("github.com/branches.get", context);
    const response = githubGetNullable(repositoryPath(owner, name) + "/branches/" + segment(branch, "branch"));
    return response === null ? null : branchFrom(response.json() as ApiBranch);
}

/** List repository branches.
 * @param repository Owner and name of the repository.
 * @param options Page size and `pageToken`; omit it for the API defaults (first page, 30 items).
 * @returns One page of branches; `nextPageToken` is "" on the last page.
 * @capability github.com/branches.list { owner: string, repo: string, branch: string, path: string, ref: string, treeSha: string, head: string, base: string }
 */
export function listBranches(repository: RepositoryRef, options: PageOptions = {}): PageResult<Branch> {
    const { owner: requestedOwner, name: requestedName } = repository;
    const { owner, name } = canonicalRepository(requestedOwner, requestedName);
    const context = checkedRepository(owner, name);
    check("github.com/branches.list", context);
    const response = githubGet(repositoryPath(owner, name) + "/branches", pageQuery(options));
    const data = response.json() as ApiBranch[];
    const items: Branch[] = [];
    for (const item of data) items.push(branchFrom(item));
    return pageFrom(response, items);
}

/** List repository commits.
 * @param repository Owner and name of the repository.
 * @param options Filters (`sha`, `path`, `author`, `since`, `until`) and pagination; omit it to list the default branch's recent commits, 30 per page.
 * @returns One page of commits without stats or file lists; `nextPageToken` is "" on the last page.
 * @capability github.com/commits.list { owner: string, repo: string, branch: string, path: string, ref: string, treeSha: string, head: string, base: string }
 */
export function listCommits(repository: RepositoryRef, options: ListCommitsOptions = {}): PageResult<Commit> {
    const { owner: requestedOwner, name: requestedName } = repository;
    const { owner, name } = canonicalRepository(requestedOwner, requestedName);
    const { sha, path, author, since, until, limit, pageToken } = options;
    const context = checkedRepository(owner, name);
    context.ref = namedRef(sha);
    context.path = namedRef(path);
    check("github.com/commits.list", context);
    const query = pageQuery({ limit, pageToken });
    putQuery(query, "sha", sha);
    putQuery(query, "path", path);
    putQuery(query, "author", author);
    putQuery(query, "since", since === undefined ? undefined : toRfc3339(since, "since"));
    putQuery(query, "until", until === undefined ? undefined : toRfc3339(until, "until"));
    const response = githubGet(repositoryPath(owner, name) + "/commits", query);
    const data = response.json() as ApiCommit[];
    const items: Commit[] = [];
    for (const item of data) items.push(commitFrom(item, "none"));
    return pageFrom(response, items);
}

/** Retrieve one commit with configurable detail.
 * @param repository Owner and name of the repository.
 * @param ref Commit SHA, branch or tag name.
 * @param detail `"none"`, `"stats"` or `"patch"`; `"stats"` when omitted.
 * @returns The commit at the requested detail level, or `null` when the ref does not exist.
 * @capability github.com/commits.get { owner: string, repo: string, ref: string, branch: string, path: string, treeSha: string, head: string, base: string }
 */
export function getCommit(repository: RepositoryRef, ref: string, detail: CommitDetail = "stats"): Commit | null {
    const { owner: requestedOwner, name: requestedName } = repository;
    const { owner, name } = canonicalRepository(requestedOwner, requestedName);
    const context = repositoryContext(owner, name, "ref", ref);
    check("github.com/commits.get", context);
    const response = githubGetNullable(repositoryPath(owner, name) + "/commits/" + segment(ref, "commit ref"));
    return response === null ? null : commitFrom(response.json() as ApiCommit, detail);
}

/** List repository releases.
 * @param repository Owner and name of the repository.
 * @param options Page size and `pageToken`; omit it for the API defaults (first page, 30 items).
 * @returns One page of releases; `nextPageToken` is "" on the last page.
 * @capability github.com/releases.list { owner: string, repo: string, branch: string, path: string, ref: string, treeSha: string, head: string, base: string }
 */
export function listReleases(repository: RepositoryRef, options: PageOptions = {}): PageResult<Release> {
    const { owner: requestedOwner, name: requestedName } = repository;
    const { owner, name } = canonicalRepository(requestedOwner, requestedName);
    const context = checkedRepository(owner, name);
    check("github.com/releases.list", context);
    const response = githubGet(repositoryPath(owner, name) + "/releases", pageQuery(options));
    const data = response.json() as ApiRelease[];
    const items: Release[] = [];
    for (const item of data) items.push(releaseFrom(item));
    return pageFrom(response, items);
}

/** Retrieve the latest release, or null when the repository has none.
 * @param repository Owner and name of the repository.
 * @returns The latest published release, or `null` when the repository has none.
 * @capability github.com/releases.getLatest { owner: string, repo: string, branch: string, path: string, ref: string, treeSha: string, head: string, base: string }
 */
export function getLatestRelease(repository: RepositoryRef): Release | null {
    const { owner: requestedOwner, name: requestedName } = repository;
    const { owner, name } = canonicalRepository(requestedOwner, requestedName);
    const context = checkedRepository(owner, name);
    check("github.com/releases.getLatest", context);
    const response = githubGetNullable(repositoryPath(owner, name) + "/releases/latest");
    return response === null ? null : releaseFrom(response.json() as ApiRelease);
}

/** Read a repository file as bytes. `ref` in the check is the branch, tag, or commit the caller
 * names, or null for the repository's default branch.
 * @param repository Owner and name of the repository.
 * @param path Repository-relative file path, without a leading slash.
 * @param options `ref` (branch, tag or commit SHA) and `maxBytes`; omit it to read the default branch with the 10485760-byte limit.
 * @returns The file with its raw `bytes`, SHA and size, or `null` when the path does not exist.
 * @capability github.com/contents.readFile { owner: string, repo: string, path: string, ref: string, branch: string, treeSha: string, head: string, base: string }
 */
export function readFile(repository: RepositoryRef, path: string, options: FileReadOptions = {}): RepositoryFile | null {
    const { owner: requestedOwner, name: requestedName } = repository;
    const { owner, name } = canonicalRepository(requestedOwner, requestedName);
    const { ref, maxBytes } = options;
    const context = repositoryContext(owner, name, "path", path);
    context.ref = namedRef(ref);
    check("github.com/contents.readFile", context);
    return readFileUnchecked(owner, name, path, { ref: ref, maxBytes: maxBytes });
}

/** Read a repository file as strict UTF-8 text.
 * @param repository Owner and name of the repository.
 * @param path Repository-relative file path, without a leading slash.
 * @param options `ref` (branch, tag or commit SHA) and `maxBytes`; omit it to read the default branch with the 10485760-byte limit.
 * @returns The file with its decoded `text`, SHA and size, or `null` when the path does not exist.
 * @capability github.com/contents.readTextFile { owner: string, repo: string, path: string, ref: string, branch: string, treeSha: string, head: string, base: string }
 */
export function readTextFile(repository: RepositoryRef, path: string, options: FileReadOptions = {}): RepositoryTextFile | null {
    const { owner: requestedOwner, name: requestedName } = repository;
    const { owner, name } = canonicalRepository(requestedOwner, requestedName);
    const { ref, maxBytes } = options;
    const context = repositoryContext(owner, name, "path", path);
    context.ref = namedRef(ref);
    check("github.com/contents.readTextFile", context);
    const file = readFileUnchecked(owner, name, path, { ref: ref, maxBytes: maxBytes });
    if (file === null) return null;
    return {
        path: file.path,
        sha: file.sha,
        size: file.size,
        htmlUrl: file.htmlUrl,
        text: new TextDecoder().decode(file.bytes),
    };
}

/** List direct entries in a repository directory.
 * @param repository Owner and name of the repository.
 * @param path Repository-relative directory path; "" lists the repository root.
 * @param options `ref` (branch, tag or commit SHA); omit it to list the default branch.
 * @returns The direct entries of the directory, each with its type, name, path, SHA and size.
 * @capability github.com/contents.listDirectory { owner: string, repo: string, path: string, ref: string, branch: string, treeSha: string, head: string, base: string }
 */
export function listDirectory(repository: RepositoryRef, path: string, options: DirectoryOptions = {}): DirectoryEntry[] {
    const { owner: requestedOwner, name: requestedName } = repository;
    const { owner, name } = canonicalRepository(requestedOwner, requestedName);
    const { ref } = options;
    const context = repositoryContext(owner, name, "path", path);
    context.ref = namedRef(ref);
    check("github.com/contents.listDirectory", context);
    const cleanPath = path.length === 0 ? "" : repositoryFilePath(path);
    const query = new Map<string, string>();
    putQuery(query, "ref", ref);
    const suffix = cleanPath.length === 0 ? "/contents" : "/contents/" + encodedFilePath(cleanPath);
    const data = githubGet(repositoryPath(owner, name) + suffix, query).json() as ApiContent[];
    const items: DirectoryEntry[] = [];
    for (const item of data) {
        items.push({
            type: str(item.type),
            name: str(item.name),
            path: str(item.path),
            sha: str(item.sha),
            size: num(item.size),
            htmlUrl: item.html_url ?? null,
        });
    }
    return items;
}

/** Retrieve a Git tree.
 * @param repository Owner and name of the repository.
 * @param treeSha SHA of the Git tree (for example a commit's tree SHA).
 * @param recursive True to include entries of all subtrees; `false` lists only the top level.
 * @returns The tree with its SHA, entries and a `truncated` flag, or `null` when the tree does not exist.
 * @capability github.com/trees.get { owner: string, repo: string, treeSha: string, branch: string, path: string, ref: string, head: string, base: string }
 */
export function getTree(repository: RepositoryRef, treeSha: string, recursive: boolean = false): RepositoryTree | null {
    const { owner: requestedOwner, name: requestedName } = repository;
    const { owner, name } = canonicalRepository(requestedOwner, requestedName);
    const context = repositoryContext(owner, name, "treeSha", treeSha);
    check("github.com/trees.get", context);
    const query = new Map<string, string>();
    if (recursive) query.set("recursive", "1");
    const response = githubGetNullable(repositoryPath(owner, name) + "/git/trees/" + segment(treeSha, "tree SHA"), query);
    if (response === null) return null;
    const data = response.json() as ApiTree;
    const entries: TreeEntry[] = [];
    for (const item of array(data.tree)) {
        entries.push({
            path: str(item.path),
            mode: str(item.mode),
            type: str(item.type),
            sha: str(item.sha),
            size: item.size ?? null,
        });
    }
    return { sha: str(data.sha), truncated: data.truncated === true, entries: entries };
}

/** Create a branch at an exact commit SHA.
 * @param repository Owner and name of the repository.
 * @param input New branch `name` and the `fromSha` commit it starts at.
 * @returns The created branch with its name and starting SHA.
 * @capability github.com/branches.create { owner: string, repo: string, branch: string, path: string, ref: string, treeSha: string, head: string, base: string }
 */
export function createBranch(repository: RepositoryRef, input: CreateBranchInput): Branch {
    const { owner: requestedOwner, name: requestedName } = repository;
    const { owner, name: repo } = canonicalRepository(requestedOwner, requestedName);
    const { name: requested, fromSha } = input;
    const context = repositoryContext(owner, repo, "branch", requested);
    check("github.com/branches.create", context);
    const name = branchName(requested);
    requireSha(fromSha, "source commit SHA");
    const body: GitRefBody = { ref: "refs/heads/" + name, sha: fromSha };
    githubPost(repositoryPath(owner, repo) + "/git/refs", body);
    return { name: name, sha: fromSha, protected: false };
}

/** Delete a repository branch.
 * @param repository Owner and name of the repository.
 * @param branch Branch name to delete.
 * @capability github.com/branches.delete { owner: string, repo: string, branch: string, path: string, ref: string, treeSha: string, head: string, base: string }
 */
export function deleteBranch(repository: RepositoryRef, branch: string): void {
    const { owner: requestedOwner, name: requestedName } = repository;
    const { owner, name: repo } = canonicalRepository(requestedOwner, requestedName);
    const context = repositoryContext(owner, repo, "branch", branch);
    check("github.com/branches.delete", context);
    const name = branchName(branch);
    githubDelete(repositoryPath(owner, repo) + "/git/refs/heads/" + segment(name, "branch"));
}

/** Commit multiple file writes and deletions with optimistic branch concurrency.
 * @param repository Owner and name of the repository.
 * @param input Target `branch`, its current `expectedHeadSha`, commit `message` and the file `changes` to write or delete.
 * @returns The created commit, including its stats and per-file patches.
 * @capability github.com/commits.create { owner: string, repo: string, branch: string, path: string, ref: string, treeSha: string, head: string, base: string }
 */
export function commitFiles(repository: RepositoryRef, input: CommitFilesInput): Commit {
    const { owner: requestedOwner, name: requestedName } = repository;
    const { owner, name } = canonicalRepository(requestedOwner, requestedName);
    const { branch: requestedBranch, expectedHeadSha, message, changes } = input;
    const context = repositoryContext(owner, name, "branch", requestedBranch);
    check("github.com/commits.create", context);
    const branch = branchName(requestedBranch);
    requireSha(expectedHeadSha, "expected head SHA");
    requireText(message, "commit message");
    validateChanges(changes);
    const current = githubGet(repositoryPath(owner, name) + "/git/ref/heads/" + segment(branch, "branch")).json() as ApiGitRef;
    const currentSha = str(current.object?.sha);
    if (currentSha !== expectedHeadSha) {
        throw validationError("branch_moved", "Branch head no longer matches expectedHeadSha");
    }
    const parent = githubGet(repositoryPath(owner, name) + "/git/commits/" + segment(expectedHeadSha, "expected head SHA")).json() as ApiGitCommit;
    const parentTree = str(parent.tree?.sha);
    if (parentTree.length === 0) throw validationError("missing_tree", "Expected commit does not include a tree SHA");
    const treeItems: GitTreeItemBody[] = [];
    for (const change of changes) {
        const path = repositoryFilePath(change.path);
        if (change.type === "delete") {
            treeItems.push({ path: path, mode: "100644", type: "blob", sha: null });
        } else {
            const content = change.content;
            if (content === undefined) throw validationError("missing_content", "Write change for " + path + " requires content");
            let encoded = "";
            let encoding = "utf-8";
            if (typeof content === "string") {
                encoded = content;
            } else {
                encoded = content.toBase64();
                encoding = "base64";
            }
            const blobBody: GitBlobBody = { content: encoded, encoding: encoding };
            const created = githubPost(repositoryPath(owner, name) + "/git/blobs", blobBody).json() as ApiCreatedSha;
            treeItems.push({ path: path, mode: "100644", type: "blob", sha: str(created.sha) });
        }
    }
    const treeBody: GitTreeBody = { base_tree: parentTree, tree: treeItems };
    const tree = githubPost(repositoryPath(owner, name) + "/git/trees", treeBody).json() as ApiCreatedSha;
    const commitBody: GitCommitBody = { message: message, tree: str(tree.sha), parents: [expectedHeadSha] };
    const createdCommit = githubPost(repositoryPath(owner, name) + "/git/commits", commitBody).json() as ApiCreatedSha;
    const createdSha = str(createdCommit.sha);
    const updateBody: GitUpdateRefBody = { sha: createdSha, force: false };
    const update = githubPatchRaw(repositoryPath(owner, name) + "/git/refs/heads/" + segment(branch, "branch"), updateBody);
    if (!update.ok) {
        if (update.status === 409 || update.status === 422) {
            throw githubError(update, "branch_moved", "Branch moved while the commit was being created");
        }
        requireOk(update);
    }
    const result = getCommitUnchecked(owner, name, createdSha, "patch");
    if (result === null) throw validationError("missing_created_commit", "GitHub created the commit but it could not be retrieved");
    return result;
}

/** Retrieve an issue, or null when absent.
 * @param repository Owner and name of the repository.
 * @param number Issue number within the repository.
 * @returns The issue, or `null` when the number does not exist; throws if the number is a pull request.
 * @capability github.com/issues.get { owner: string, repo: string, number: number, base: string }
 */
export function getIssue(repository: RepositoryRef, number: number): Issue | null {
    const { owner: requestedOwner, name: requestedName } = repository;
    const { owner, name } = canonicalRepository(requestedOwner, requestedName);
    const context = repositoryNumberContext(owner, name, number);
    check("github.com/issues.get", context);
    const response = githubGetNullable(repositoryPath(owner, name) + "/issues/" + issueNumber(number));
    if (response === null) return null;
    const data = response.json() as ApiIssue;
    if (data.pull_request !== null && data.pull_request !== undefined) {
        throw validationError("wrong_resource_type", "GitHub number identifies a pull request; use getPullRequest");
    }
    return issueFrom(data);
}

/** List true issues using cursor pagination.
 * @param repository Owner and name of the repository.
 * @param options State, labels, ordering and pagination; omit it to list open and closed issues by most recently updated, 30 per page.
 * @returns One page of issues, excluding pull requests; `nextPageToken` is "" on the last page.
 * @capability github.com/issues.list { owner: string, repo: string, branch: string, path: string, ref: string, treeSha: string, head: string, base: string }
 */
export function listIssues(repository: RepositoryRef, options: ListIssuesOptions = {}): PageResult<Issue> {
    const { owner: requestedOwner, name: requestedName } = repository;
    const { owner, name } = canonicalRepository(requestedOwner, requestedName);
    const { state, labels: requestedLabels, orderBy, direction, limit, pageToken: cursor } = options;
    let labels: string[] | undefined;
    if (requestedLabels !== undefined) {
        const copied: string[] = [];
        for (const label of requestedLabels) copied.push(label);
        labels = copied;
    }
    const context = checkedRepository(owner, name);
    check("github.com/issues.list", context);
    const variables: GraphQlIssueVariables = {
        owner: owner,
        name: name,
        first: pageLimit(limit),
        after: pageToken(cursor, false) ?? null,
        states: state === undefined ? null : [state],
        labels: labels ?? null,
        orderField: orderBy ?? "UPDATED_AT",
        direction: direction ?? "DESC",
    };
    const variableFields: string[] = [
        jsonProperty("owner", JSON.stringify(variables.owner)),
        jsonProperty("name", JSON.stringify(variables.name)),
        jsonProperty("first", variables.first.toString()),
        jsonProperty("after", variables.after === null ? "null" : JSON.stringify(variables.after)),
        jsonProperty("states", variables.states === null ? "null" : stringArrayJson(variables.states)),
        jsonProperty("labels", variables.labels === null ? "null" : stringArrayJson(variables.labels)),
        jsonProperty("orderField", JSON.stringify(variables.orderField)),
        jsonProperty("direction", JSON.stringify(variables.direction)),
    ];
    const body = jsonObject([
        jsonProperty("query", JSON.stringify(ISSUE_QUERY)),
        jsonProperty("variables", jsonObject(variableFields)),
    ]);
    const envelope = githubPostGraphQl(body).json() as GraphQlEnvelope;
    const errors = array(envelope.errors);
    if (errors.length > 0) {
        const messages: string[] = [];
        for (const item of errors) messages.push(str(item.message));
        throw new GitHubError("graphql_error", messages.join("; "), 200, "", "", null, null, null, messages);
    }
    const data = required(envelope.data, "GraphQL data") as GraphQlIssueData;
    const repositoryData = required(data.repository, "GraphQL repository");
    const connection = required(repositoryData.issues, "GraphQL issue connection");
    const items: Issue[] = [];
    for (const item of array(connection.nodes)) items.push(graphQlIssueFrom(item));
    const info = required(connection.pageInfo, "GraphQL page information");
    const next = info.hasNextPage === true ? str(info.endCursor) : "";
    return { items: items, nextPageToken: next, isComplete: next.length === 0 };
}

/** Search issues within one repository.
 * @param repository Owner and name of the repository.
 * @param query Non-empty GitHub search query for issues, which may use qualifiers such as `is:open`. `is:issue` and the repository scope are added automatically; `repo:`, `org:`, `user:`, `is:issue`, `is:pr` and `OR` are rejected.
 * @param options Sort field, order, page size and `pageToken`; omit it for best-match order, the first page and 30 items.
 * @returns One page of issues with `totalCount` and `incompleteResults`; `nextPageToken` is "" on the last page.
 * @capability github.com/issues.search { owner: string, repo: string, branch: string, path: string, ref: string, treeSha: string, head: string, base: string }
 */
export function searchIssues(repository: RepositoryRef, query: string, options: SearchOptions = {}): SearchPageResult<Issue> {
    const { owner: requestedOwner, name: requestedName } = repository;
    const { owner, name } = canonicalRepository(requestedOwner, requestedName);
    const context = checkedRepository(owner, name);
    check("github.com/issues.search", context);
    return searchIssuesOrPulls(owner, name, query, options, "is:issue", false);
}

/** Create an issue.
 * @param repository Owner and name of the repository.
 * @param input Issue `title` and optional `body`, `assignees`, `labels` and `milestone`.
 * @returns The created issue.
 * @capability github.com/issues.create { owner: string, repo: string, branch: string, path: string, ref: string, treeSha: string, head: string, base: string }
 */
export function createIssue(repository: RepositoryRef, input: CreateIssueInput): Issue {
    const { owner: requestedOwner, name: requestedName } = repository;
    const { owner, name } = canonicalRepository(requestedOwner, requestedName);
    const {
        title,
        body: description,
        assignees: requestedAssignees,
        labels: requestedLabels,
        milestone,
    } = input;
    let assignees: string[] | undefined;
    if (requestedAssignees !== undefined) {
        const copied: string[] = [];
        for (const assignee of requestedAssignees) copied.push(assignee);
        assignees = copied;
    }
    let labels: string[] | undefined;
    if (requestedLabels !== undefined) {
        const copied: string[] = [];
        for (const label of requestedLabels) copied.push(label);
        labels = copied;
    }
    const context = checkedRepository(owner, name);
    check("github.com/issues.create", context);
    requireText(title, "issue title");
    const fields: string[] = [jsonProperty("title", JSON.stringify(title))];
    if (description !== undefined) fields.push(jsonProperty("body", JSON.stringify(description)));
    if (assignees !== undefined) fields.push(jsonProperty("assignees", stringArrayJson(assignees)));
    if (labels !== undefined) fields.push(jsonProperty("labels", stringArrayJson(labels)));
    if (milestone !== undefined) fields.push(jsonProperty("milestone", milestone.toString()));
    const body = jsonObject(fields);
    return issueFrom(githubPost(repositoryPath(owner, name) + "/issues", body).json() as ApiIssue);
}

/** Update an issue.
 * @param repository Owner and name of the repository.
 * @param number Issue number within the repository.
 * @param input Fields to change; at least one must be set, omitted fields are left as they are.
 * @returns The updated issue.
 * @capability github.com/issues.update { owner: string, repo: string, number: number, base: string }
 */
export function updateIssue(repository: RepositoryRef, number: number, input: UpdateIssueInput): Issue {
    const { owner: requestedOwner, name: requestedName } = repository;
    const { owner, name } = canonicalRepository(requestedOwner, requestedName);
    const {
        title,
        body: description,
        clearBody,
        assignees: requestedAssignees,
        labels: requestedLabels,
        milestone,
        clearMilestone,
        state,
        stateReason,
    } = input;
    let assignees: string[] | undefined;
    if (requestedAssignees !== undefined) {
        const copied: string[] = [];
        for (const assignee of requestedAssignees) copied.push(assignee);
        assignees = copied;
    }
    let labels: string[] | undefined;
    if (requestedLabels !== undefined) {
        const copied: string[] = [];
        for (const label of requestedLabels) copied.push(label);
        labels = copied;
    }
    const context = repositoryNumberContext(owner, name, number);
    check("github.com/issues.update", context);
    const fields: string[] = [];
    if (title !== undefined) fields.push(jsonProperty("title", JSON.stringify(title)));
    if (description !== undefined) fields.push(jsonProperty("body", JSON.stringify(description)));
    if (clearBody === true) fields.push(jsonProperty("body", "null"));
    if (assignees !== undefined) fields.push(jsonProperty("assignees", stringArrayJson(assignees)));
    if (labels !== undefined) fields.push(jsonProperty("labels", stringArrayJson(labels)));
    if (milestone !== undefined) fields.push(jsonProperty("milestone", milestone.toString()));
    if (clearMilestone === true) fields.push(jsonProperty("milestone", "null"));
    if (state !== undefined) fields.push(jsonProperty("state", JSON.stringify(state)));
    if (stateReason !== undefined) fields.push(jsonProperty("state_reason", JSON.stringify(stateReason)));
    requireFields(fields);
    const body = jsonObject(fields);
    return issueFrom(githubPatch(repositoryPath(owner, name) + "/issues/" + issueNumber(number), body).json() as ApiIssue);
}

/** List issue comments.
 * @param repository Owner and name of the repository.
 * @param number Issue number within the repository.
 * @param options Page size and `pageToken`; omit it for the API defaults (first page, 30 items).
 * @returns One page of comments on the issue; `nextPageToken` is "" on the last page.
 * @capability github.com/issueComments.list { owner: string, repo: string, number: number, base: string }
 */
export function listIssueComments(repository: RepositoryRef, number: number, options: PageOptions = {}): PageResult<Comment> {
    const { owner: requestedOwner, name: requestedName } = repository;
    const { owner, name } = canonicalRepository(requestedOwner, requestedName);
    const context = repositoryNumberContext(owner, name, number);
    check("github.com/issueComments.list", context);
    return listCommentsUnchecked(owner, name, number, options);
}

/** Add an issue comment.
 * @param repository Owner and name of the repository.
 * @param number Issue number within the repository.
 * @param body Comment text (Markdown).
 * @returns The created comment.
 * @capability github.com/issueComments.create { owner: string, repo: string, number: number, base: string }
 */
export function addIssueComment(repository: RepositoryRef, number: number, body: string): Comment {
    const { owner: requestedOwner, name: requestedName } = repository;
    const { owner, name } = canonicalRepository(requestedOwner, requestedName);
    const context = repositoryNumberContext(owner, name, number);
    check("github.com/issueComments.create", context);
    return addCommentUnchecked(owner, name, number, body);
}

/** List labels in a repository.
 * @param repository Owner and name of the repository.
 * @param options Page size and `pageToken`; omit it for the API defaults (first page, 30 items).
 * @returns One page of labels defined in the repository; `nextPageToken` is "" on the last page.
 * @capability github.com/labels.list { owner: string, repo: string, branch: string, path: string, ref: string, treeSha: string, head: string, base: string }
 */
export function listLabels(repository: RepositoryRef, options: PageOptions = {}): PageResult<Label> {
    const { owner: requestedOwner, name: requestedName } = repository;
    const { owner, name } = canonicalRepository(requestedOwner, requestedName);
    const context = checkedRepository(owner, name);
    check("github.com/labels.list", context);
    const response = githubGet(repositoryPath(owner, name) + "/labels", pageQuery(options));
    const data = response.json() as ApiLabel[];
    const items: Label[] = [];
    for (const item of data) items.push(labelFrom(item));
    return pageFrom(response, items);
}

/** Retrieve a pull request, or null when absent.
 * @param repository Owner and name of the repository.
 * @param number Pull request number within the repository.
 * @returns The pull request, or `null` when the number does not exist or is not a pull request.
 * @capability github.com/pulls.get { owner: string, repo: string, number: number, base: string }
 */
export function getPullRequest(repository: RepositoryRef, number: number): PullRequest | null {
    const { owner: requestedOwner, name: requestedName } = repository;
    const { owner, name } = canonicalRepository(requestedOwner, requestedName);
    const context = repositoryNumberContext(owner, name, number);
    check("github.com/pulls.get", context);
    return getPullRequestUnchecked(owner, name, number);
}

/** List pull requests.
 * @param repository Owner and name of the repository.
 * @param options State, head and base filters, sorting and pagination; omit it to list open pull requests, 30 per page.
 * @returns One page of pull requests; `nextPageToken` is "" on the last page.
 * @capability github.com/pulls.list { owner: string, repo: string, branch: string, path: string, ref: string, treeSha: string, head: string, base: string }
 */
export function listPullRequests(repository: RepositoryRef, options: ListPullRequestsOptions = {}): PageResult<PullRequest> {
    const { owner: requestedOwner, name: requestedName } = repository;
    const { owner, name } = canonicalRepository(requestedOwner, requestedName);
    const { state, head, base, sort, direction } = options;
    const context = checkedRepository(owner, name);
    check("github.com/pulls.list", context);
    const query = pageQuery(options);
    putQuery(query, "state", state);
    putQuery(query, "head", head);
    putQuery(query, "base", base);
    putQuery(query, "sort", sort);
    putQuery(query, "direction", direction);
    const response = githubGet(repositoryPath(owner, name) + "/pulls", query);
    const data = response.json() as ApiPullRequest[];
    const items: PullRequest[] = [];
    for (const item of data) items.push(pullRequestFrom(item));
    return pageFrom(response, items);
}

/** Fetch issue-style reaction counts for one pull request without loading diff details.
 * @param repository Owner and repository name.
 * @param number Pull-request number within the repository.
 * @returns Reaction summary, or null if the pull request is missing or GitHub omits the summary.
 * @capability github.com/pulls.getReactions { owner: string, repo: string, number: number }
 */
export function getPullRequestReactions(repository: RepositoryRef, number: number): ReactionSummary | null {
    const { owner: requestedOwner, name: requestedName } = repository;
    const { owner, name } = canonicalRepository(requestedOwner, requestedName);
    requireIssueNumber(number);
    check("github.com/pulls.getReactions", { owner: owner, repo: name, number: number });
    const response = githubGetNullable(repositoryPath(owner, name) + "/issues/" + issueNumber(number));
    if (response === null) return null;
    const data = response.json() as ApiIssue;
    if (data.pull_request === null || data.pull_request === undefined) {
        throw validationError("wrong_resource_type", "GitHub number identifies an issue; use getIssue");
    }
    return reactionsFrom(data.reactions);
}

/** Search issue metadata across repositories. Requires a separate, broader grant.
 * @param query GitHub terms and scope qualifiers; kind, author, creation qualifiers and OR are reserved.
 * @param options Author, inclusive UTC creation dates, sorting and pagination.
 * @returns One page with repository identity, reactions, pagination and partial-result flags.
 * @capability github.com/issues.searchAcrossRepositories { query: string, author: string, createdSince: string, createdUntil: string }
 */
export function searchIssuesAcrossRepositories(query: string, options: ResearchSearchOptions = {}): ResearchPageResult<IssueSearchHit> {
    const { author, createdSince, createdUntil, sort, order, limit, pageToken } = options;
    const request = researchRequest(query, { author, createdSince, createdUntil, sort, order, limit, pageToken }, "is:issue");
    check("github.com/issues.searchAcrossRepositories", {
        query: request.query, author: request.author, createdSince: request.createdSince, createdUntil: request.createdUntil,
    });
    const response = githubGet("/search/issues", searchQuery(request.query, request.search));
    const data = researchData(response);
    const items: IssueSearchHit[] = [];
    for (const item of array(data.items)) {
        if (item.pull_request !== null && item.pull_request !== undefined) {
            throw validationError("invalid_response", "Issue search returned a pull request");
        }
        items.push({ repository: searchRepository(item), issue: issueFrom(item) });
    }
    return researchPage(response, items, data, request.search);
}

/** Search lightweight pull-request metadata across repositories with one HTTP call per page.
 * @param query GitHub terms and scope qualifiers; kind, author, creation qualifiers and OR are reserved.
 * @param options Author, inclusive UTC creation dates, sorting and pagination.
 * @returns One metadata page; select hits for explicit detail enrichment.
 * @capability github.com/pulls.searchAcrossRepositories { query: string, author: string, createdSince: string, createdUntil: string }
 */
export function searchPullRequestsAcrossRepositories(query: string, options: ResearchSearchOptions = {}): ResearchPageResult<PullRequestSearchHit> {
    const { author, createdSince, createdUntil, sort, order, limit, pageToken } = options;
    const request = researchRequest(query, { author, createdSince, createdUntil, sort, order, limit, pageToken }, "is:pr");
    check("github.com/pulls.searchAcrossRepositories", {
        query: request.query, author: request.author, createdSince: request.createdSince, createdUntil: request.createdUntil,
    });
    return pullSummaryPage(request);
}

/** Lightweight pull-request search within one repository; enrich selected hits with getPullRequest.
 * @param repository Owner and repository name.
 * @param query GitHub terms; repository scope, kind, author, creation qualifiers and OR are reserved.
 * @param options Author, inclusive UTC creation dates, sorting and pagination.
 * @returns One repository-restricted metadata page without per-hit HTTP requests.
 * @capability github.com/pulls.searchSummaries { owner: string, repo: string }
 */
export function searchPullRequestSummaries(repository: RepositoryRef, query: string, options: ResearchSearchOptions = {}): ResearchPageResult<PullRequestSearchHit> {
    const { owner: requestedOwner, name: requestedName } = repository;
    const { owner, name } = canonicalRepository(requestedOwner, requestedName);
    const { author, createdSince, createdUntil, sort, order, limit, pageToken } = options;
    const request = researchRequest(query, { author, createdSince, createdUntil, sort, order, limit, pageToken }, "is:pr");
    // Keep the same scope parser as the existing repository-restricted endpoints.
    const scoped = scopedSearch(query, owner, name, "");
    request.query = scoped + request.query.slice(query.length);
    check("github.com/pulls.searchSummaries", { owner: owner, repo: name });
    return pullSummaryPage(request);
}

/** Search pull requests within one repository.
 * @param repository Owner and name of the repository.
 * @param query Non-empty GitHub search query for pull requests, which may use qualifiers such as `is:open`. `is:pr` and the repository scope are added automatically; `repo:`, `org:`, `user:`, `is:issue`, `is:pr` and `OR` are rejected.
 * @param options Sort field, order, page size and `pageToken`; omit it for best-match order, the first page and 30 items.
 * @returns One page of pull requests with `totalCount` and `incompleteResults`; `nextPageToken` is "" on the last page.
 * @capability github.com/pulls.search { owner: string, repo: string, branch: string, path: string, ref: string, treeSha: string, head: string, base: string }
 */
export function searchPullRequests(repository: RepositoryRef, query: string, options: SearchOptions = {}): SearchPageResult<PullRequest> {
    const { owner: requestedOwner, name: requestedName } = repository;
    const { owner, name } = canonicalRepository(requestedOwner, requestedName);
    const context = checkedRepository(owner, name);
    check("github.com/pulls.search", context);
    const issues = searchIssuesOrPulls(owner, name, query, options, "is:pr", true);
    const items: PullRequest[] = [];
    for (const item of issues.items) {
        const pull = getPullRequestUnchecked(owner, name, item.number);
        if (pull !== null) items.push(pull);
    }
    return {
        items: items,
        nextPageToken: issues.nextPageToken,
        isComplete: issues.isComplete,
        totalCount: issues.totalCount,
        incompleteResults: issues.incompleteResults,
    };
}

/** Create a pull request.
 * @param repository Owner and name of the repository.
 * @param input Pull request `title`, `head` and `base` branches and optional `body`, `draft` and `maintainerCanModify`.
 * @returns The created pull request.
 * @capability github.com/pulls.create { owner: string, repo: string, head: string, base: string, branch: string, path: string, ref: string, treeSha: string }
 */
export function createPullRequest(repository: RepositoryRef, input: CreatePullRequestInput): PullRequest {
    const { owner: requestedOwner, name: requestedName } = repository;
    const { owner, name } = canonicalRepository(requestedOwner, requestedName);
    const { title, head, base, body: description, draft, maintainerCanModify } = input;
    requireText(title, "pull request title");
    requireText(head, "pull request head");
    requireText(base, "pull request base");
    const context = checkedRepository(owner, name);
    context.head = head;
    context.base = base;
    check("github.com/pulls.create", context);
    const fields: string[] = [
        jsonProperty("title", JSON.stringify(title)),
        jsonProperty("head", JSON.stringify(head)),
        jsonProperty("base", JSON.stringify(base)),
    ];
    if (description !== undefined) fields.push(jsonProperty("body", JSON.stringify(description)));
    if (draft !== undefined) fields.push(jsonProperty("draft", booleanJson(draft)));
    if (maintainerCanModify !== undefined) {
        fields.push(jsonProperty("maintainer_can_modify", booleanJson(maintainerCanModify)));
    }
    const body = jsonObject(fields);
    return pullRequestFrom(githubPost(repositoryPath(owner, name) + "/pulls", body).json() as ApiPullRequest);
}

/** Update a pull request. `base` in the check is the branch the update retargets the pull
 * request to, or null when it leaves the base as it is.
 * @param repository Owner and name of the repository.
 * @param number Pull request number within the repository.
 * @param input Fields to change; at least one must be set, omitted fields are left as they are.
 * @returns The updated pull request.
 * @capability github.com/pulls.update { owner: string, repo: string, number: number, base: string }
 */
export function updatePullRequest(repository: RepositoryRef, number: number, input: UpdatePullRequestInput): PullRequest {
    const { owner: requestedOwner, name: requestedName } = repository;
    const { owner, name } = canonicalRepository(requestedOwner, requestedName);
    const { title, body: description, clearBody, base, state, maintainerCanModify } = input;
    const context = repositoryNumberContext(owner, name, number);
    context.base = base ?? null;
    check("github.com/pulls.update", context);
    const fields: string[] = [];
    if (title !== undefined) fields.push(jsonProperty("title", JSON.stringify(title)));
    if (description !== undefined) fields.push(jsonProperty("body", JSON.stringify(description)));
    if (clearBody === true) fields.push(jsonProperty("body", "null"));
    if (base !== undefined) fields.push(jsonProperty("base", JSON.stringify(base)));
    if (state !== undefined) fields.push(jsonProperty("state", JSON.stringify(state)));
    if (maintainerCanModify !== undefined) {
        fields.push(jsonProperty("maintainer_can_modify", booleanJson(maintainerCanModify)));
    }
    requireFields(fields);
    const body = jsonObject(fields);
    return pullRequestFrom(githubPatch(repositoryPath(owner, name) + "/pulls/" + issueNumber(number), body).json() as ApiPullRequest);
}

/** Merge a pull request without force.
 * @param repository Owner and name of the repository.
 * @param number Pull request number within the repository.
 * @param input Merge `method`, commit title and message and `expectedHeadSha`; omit it for GitHub's defaults.
 * @returns The merge result with `merged`, GitHub's `message` and the merge commit `sha`.
 * @capability github.com/pulls.merge { owner: string, repo: string, number: number, base: string }
 */
export function mergePullRequest(repository: RepositoryRef, number: number, input: MergePullRequestInput = {}): MergeResult {
    const { owner: requestedOwner, name: requestedName } = repository;
    const { owner, name } = canonicalRepository(requestedOwner, requestedName);
    const { commitTitle, commitMessage, method, expectedHeadSha } = input;
    const context = repositoryNumberContext(owner, name, number);
    check("github.com/pulls.merge", context);
    const fields: string[] = [];
    if (commitTitle !== undefined) fields.push(jsonProperty("commit_title", JSON.stringify(commitTitle)));
    if (commitMessage !== undefined) fields.push(jsonProperty("commit_message", JSON.stringify(commitMessage)));
    if (method !== undefined) fields.push(jsonProperty("merge_method", JSON.stringify(method)));
    if (expectedHeadSha !== undefined) fields.push(jsonProperty("sha", JSON.stringify(expectedHeadSha)));
    const body = jsonObject(fields);
    const data = githubPut(repositoryPath(owner, name) + "/pulls/" + issueNumber(number) + "/merge", body).json() as ApiMergeResult;
    return { merged: data.merged === true, message: str(data.message), sha: str(data.sha) };
}

/** Retrieve a pull request unified diff.
 * @param repository Owner and name of the repository.
 * @param number Pull request number within the repository.
 * @returns The unified diff of the whole pull request as text.
 * @capability github.com/pulls.diff { owner: string, repo: string, number: number, base: string }
 */
export function getPullRequestDiff(repository: RepositoryRef, number: number): string {
    const { owner: requestedOwner, name: requestedName } = repository;
    const { owner, name } = canonicalRepository(requestedOwner, requestedName);
    const context = repositoryNumberContext(owner, name, number);
    check("github.com/pulls.diff", context);
    const path = repositoryPath(owner, name) + "/pulls/" + issueNumber(number);
    return githubGet(path, undefined, "application/vnd.github.diff").body;
}

/** List files changed by a pull request.
 * @param repository Owner and name of the repository.
 * @param number Pull request number within the repository.
 * @param options Page size and `pageToken`; omit it for the API defaults (first page, 30 items).
 * @returns One page of files changed by the pull request; `nextPageToken` is "" on the last page.
 * @capability github.com/pullFiles.list { owner: string, repo: string, number: number, base: string }
 */
export function listPullRequestFiles(repository: RepositoryRef, number: number, options: PageOptions = {}): PageResult<PullRequestFile> {
    const { owner: requestedOwner, name: requestedName } = repository;
    const { owner, name } = canonicalRepository(requestedOwner, requestedName);
    const context = repositoryNumberContext(owner, name, number);
    check("github.com/pullFiles.list", context);
    const response = githubGet(repositoryPath(owner, name) + "/pulls/" + issueNumber(number) + "/files", pageQuery(options));
    const data = response.json() as ApiPullFile[];
    const items: PullRequestFile[] = [];
    for (const item of data) items.push(pullFileFrom(item));
    return pageFrom(response, items);
}

/** List pull request reviews.
 * @param repository Owner and name of the repository.
 * @param number Pull request number within the repository.
 * @param options Page size and `pageToken`; omit it for the API defaults (first page, 30 items).
 * @returns One page of reviews of the pull request; `nextPageToken` is "" on the last page.
 * @capability github.com/pullReviews.list { owner: string, repo: string, number: number, base: string }
 */
export function listPullRequestReviews(repository: RepositoryRef, number: number, options: PageOptions = {}): PageResult<PullRequestReview> {
    const { owner: requestedOwner, name: requestedName } = repository;
    const { owner, name } = canonicalRepository(requestedOwner, requestedName);
    const context = repositoryNumberContext(owner, name, number);
    check("github.com/pullReviews.list", context);
    const response = githubGet(repositoryPath(owner, name) + "/pulls/" + issueNumber(number) + "/reviews", pageQuery(options));
    const data = response.json() as ApiReview[];
    const items: PullRequestReview[] = [];
    for (const item of data) items.push(reviewFrom(item));
    return pageFrom(response, items);
}

/** Create a pull request review.
 * @param repository Owner and name of the repository.
 * @param number Pull request number within the repository.
 * @param input Review `event`, optional `body`, `commitId` and inline `comments`; `body` is required for `REQUEST_CHANGES`.
 * @returns The created review.
 * @capability github.com/pullReviews.create { owner: string, repo: string, number: number, base: string }
 */
export function createPullRequestReview(repository: RepositoryRef, number: number, input: CreateReviewInput): PullRequestReview {
    const { owner: requestedOwner, name: requestedName } = repository;
    const { owner, name } = canonicalRepository(requestedOwner, requestedName);
    const { body: summary, event, commitId, comments } = input;
    const context = repositoryNumberContext(owner, name, number);
    check("github.com/pullReviews.create", context);
    if (event === "REQUEST_CHANGES") requireText(summary ?? "", "review body");
    const serializedComments: string[] = [];
    if (comments !== undefined) {
        for (const item of comments) {
            validateReviewComment(item);
            const commentFields: string[] = [
                jsonProperty("path", JSON.stringify(item.path)),
                jsonProperty("body", JSON.stringify(item.body)),
                jsonProperty("line", item.line.toString()),
                jsonProperty("side", JSON.stringify(item.side)),
            ];
            if (item.startLine !== undefined) commentFields.push(jsonProperty("start_line", item.startLine.toString()));
            if (item.startSide !== undefined) commentFields.push(jsonProperty("start_side", JSON.stringify(item.startSide)));
            serializedComments.push(jsonObject(commentFields));
        }
    }
    const fields: string[] = [jsonProperty("event", JSON.stringify(event))];
    if (summary !== undefined) fields.push(jsonProperty("body", JSON.stringify(summary)));
    if (commitId !== undefined) fields.push(jsonProperty("commit_id", JSON.stringify(commitId)));
    if (comments !== undefined) fields.push(jsonProperty("comments", "[" + serializedComments.join(",") + "]"));
    const body = jsonObject(fields);
    return reviewFrom(githubPost(repositoryPath(owner, name) + "/pulls/" + issueNumber(number) + "/reviews", body).json() as ApiReview);
}

/** List general pull request comments.
 * @param repository Owner and name of the repository.
 * @param number Pull request number within the repository.
 * @param options Page size and `pageToken`; omit it for the API defaults (first page, 30 items).
 * @returns One page of general (non-inline) comments on the pull request; `nextPageToken` is "" on the last page.
 * @capability github.com/pullComments.list { owner: string, repo: string, number: number, base: string }
 */
export function listPullRequestComments(repository: RepositoryRef, number: number, options: PageOptions = {}): PageResult<Comment> {
    const { owner: requestedOwner, name: requestedName } = repository;
    const { owner, name } = canonicalRepository(requestedOwner, requestedName);
    const context = repositoryNumberContext(owner, name, number);
    check("github.com/pullComments.list", context);
    return listCommentsUnchecked(owner, name, number, options);
}

/** Add a general pull request comment.
 * @param repository Owner and name of the repository.
 * @param number Pull request number within the repository.
 * @param body Comment text (Markdown).
 * @returns The created comment.
 * @capability github.com/pullComments.create { owner: string, repo: string, number: number, base: string }
 */
export function addPullRequestComment(repository: RepositoryRef, number: number, body: string): Comment {
    const { owner: requestedOwner, name: requestedName } = repository;
    const { owner, name } = canonicalRepository(requestedOwner, requestedName);
    const context = repositoryNumberContext(owner, name, number);
    check("github.com/pullComments.create", context);
    return addCommentUnchecked(owner, name, number, body);
}

function getPullRequestUnchecked(owner: string, name: string, number: number): PullRequest | null {
    const response = githubGetNullable(repositoryPath(owner, name) + "/pulls/" + issueNumber(number));
    return response === null ? null : pullRequestFrom(response.json() as ApiPullRequest);
}

function listCommentsUnchecked(
    owner: string,
    name: string,
    number: number,
    options: PageOptions,
): PageResult<Comment> {
    const response = githubGet(repositoryPath(owner, name) + "/issues/" + issueNumber(number) + "/comments", pageQuery(options));
    const data = response.json() as ApiComment[];
    const items: Comment[] = [];
    for (const item of data) items.push(commentFrom(item));
    return pageFrom(response, items);
}

function addCommentUnchecked(owner: string, name: string, number: number, body: string): Comment {
    requireText(body, "comment body");
    return commentFrom(
        githubPost(repositoryPath(owner, name) + "/issues/" + issueNumber(number) + "/comments", { body: body }).json() as ApiComment,
    );
}

interface ResearchRequest {
    query: string;
    author: string | null;
    createdSince: string | null;
    createdUntil: string | null;
    search: SearchOptions;
}

// Callers read the caller's options into consts and pass a fresh object: what
// reaches `check()` must be a value the caller can no longer change.
function researchRequest(query: string, options: ResearchSearchOptions, kind: string): ResearchRequest {
    const { author, createdSince: since, createdUntil: until, sort, order, limit: requestedLimit, pageToken: requestedToken } = options;
    const search: SearchOptions = { sort: sort, order: order, limit: requestedLimit, pageToken: requestedToken };
    requireText(query, "search query");
    const syntax = searchSyntax(query);
    if (SEARCH_KIND.test(syntax) || SEARCH_OR.test(syntax) || /author:|created:/.test(syntax)) {
        throw validationError("unsafe_search_query", "Use structured author/creation options; kind qualifiers and OR are not allowed");
    }
    let value = query + " " + kind;
    let normalizedAuthor: string | null = null;
    if (author !== undefined) {
        if (!/^(app\/)?[A-Za-z0-9_-]+(\[bot\])?$/.test(author)) throw validationError("invalid_author", "author must be one GitHub login or app/login");
        normalizedAuthor = author.toLowerCase();
        value += " author:" + normalizedAuthor;
    }
    const createdSince = researchDate(since);
    const createdUntil = researchDate(until);
    if (createdSince !== null && createdUntil !== null && createdSince > createdUntil) {
        throw validationError("invalid_date_window", "createdSince must be on or before createdUntil");
    }
    // Repeated creation qualifiers do not reliably intersect on REST search.
    if (createdSince !== null && createdUntil !== null) {
        value += " created:" + createdSince + ".." + createdUntil;
    } else if (createdSince !== null) {
        value += " created:>=" + createdSince;
    } else if (createdUntil !== null) {
        value += " created:<=" + createdUntil;
    }
    const limit = pageLimit(search.limit);
    const cursor = pageToken(search.pageToken, true);
    const page = cursor === undefined ? 1 : Number(cursor);
    if (!Number.isInteger(page) || page < 1 || (page - 1) * limit >= 1000) {
        throw validationError("invalid_page_token", "search page must fit within GitHub's 1,000-result ceiling");
    }
    return { query: value, author: normalizedAuthor, createdSince: createdSince, createdUntil: createdUntil, search: search };
}

function researchDate(value: string | undefined): string | null {
    if (value === undefined) return null;
    if (!/^[0-9]{4}-[0-9]{2}-[0-9]{2}$/.test(value)) throw validationError("invalid_date_window", "search dates must use YYYY-MM-DD");
    const year = Number(value.slice(0, 4));
    const month = Number(value.slice(5, 7));
    const day = Number(value.slice(8, 10));
    let daysInMonth = 31;
    if (month === 4 || month === 6 || month === 9 || month === 11) daysInMonth = 30;
    if (month === 2) daysInMonth = year % 4 === 0 && (year % 100 !== 0 || year % 400 === 0) ? 29 : 28;
    if (year < 1 || month < 1 || month > 12 || day < 1 || day > daysInMonth) {
        throw validationError("invalid_date_window", "search date is not a valid calendar date");
    }
    return value;
}

function searchRepository(item: ApiIssue): RepositoryRef {
    const url = str(item.repository_url);
    const prefix = "https://api.github.com/repos/";
    if (!url.startsWith(prefix)) throw validationError("invalid_response", "Search hit is missing its repository identity");
    const parts = url.slice(prefix.length).split("/");
    if (parts.length !== 2) throw validationError("invalid_response", "Search repository URL must name one owner and repository");
    return canonicalRepository(parts[0], parts[1]);
}

function pullSummaryPage(request: ResearchRequest): ResearchPageResult<PullRequestSearchHit> {
    const response = githubGet("/search/issues", searchQuery(request.query, request.search));
    const data = researchData(response);
    const items: PullRequestSearchHit[] = [];
    for (const item of array(data.items)) {
        if (item.pull_request === null || item.pull_request === undefined) {
            throw validationError("invalid_response", "Pull-request search returned an issue");
        }
        const issue = issueFrom(item);
        const summary: PullRequestSummary = {
            id: issue.id, number: issue.number, title: issue.title, body: issue.body, state: issue.state,
            stateReason: issue.stateReason, locked: issue.locked, htmlUrl: issue.htmlUrl, user: issue.user,
            labels: issue.labels, assignees: issue.assignees, milestone: issue.milestone, comments: issue.comments,
            createdAt: issue.createdAt, updatedAt: issue.updatedAt, closedAt: issue.closedAt,
            reactions: issue.reactions ?? null, draft: item.draft ?? null,
        };
        items.push({ repository: searchRepository(item), pullRequest: summary });
    }
    return researchPage(response, items, data, request.search);
}

function researchData(response: Response): ApiSearchIssues {
    const data = response.json() as ApiSearchIssues;
    if (data.total_count === null || data.total_count === undefined || !Number.isSafeInteger(data.total_count) || data.total_count < 0
        || typeof data.incomplete_results !== "boolean" || !Array.isArray(data.items)) {
        throw validationError("invalid_response", "GitHub search must include items, total_count and incomplete_results");
    }
    return data;
}

function researchPage<T>(response: Response, items: T[], data: ApiSearchIssues, search: SearchOptions): ResearchPageResult<T> {
    const page = searchPageFrom(response, items, data.total_count, data.incomplete_results);
    const limit = pageLimit(search.limit);
    let next = page.nextPageToken;
    if (next.length > 0 && (Number(next) - 1) * limit >= 1000) next = "";
    return {
        items: items, nextPageToken: next, isComplete: next.length === 0,
        totalCount: page.totalCount, incompleteResults: page.incompleteResults, isCapped: page.totalCount > 1000,
    };
}

function searchIssuesOrPulls(
    owner: string,
    name: string,
    query: string,
    options: SearchOptions,
    kind: string,
    allowPulls: boolean,
): SearchPageResult<Issue> {
    const scoped = scopedSearch(query, owner, name, kind);
    const response = githubGet("/search/issues", searchQuery(scoped, options));
    const data = response.json() as ApiSearchIssues;
    const items: Issue[] = [];
    for (const item of array(data.items)) {
        if (allowPulls || (item.pull_request === null || item.pull_request === undefined)) items.push(issueFrom(item));
    }
    return searchPageFrom(response, items, data.total_count, data.incomplete_results);
}

function readFileUnchecked(
    owner: string,
    name: string,
    path: string,
    options: FileReadOptions,
): RepositoryFile | null {
    const cleanPath = repositoryFilePath(path);
    const query = new Map<string, string>();
    putQuery(query, "ref", options.ref);
    const response = githubGetNullable(repositoryPath(owner, name) + "/contents/" + encodedFilePath(cleanPath), query);
    if (response === null) return null;
    const data = response.json() as ApiContent;
    if (str(data.type) !== "file") throw validationError("not_a_file", cleanPath + " is not a repository file");
    const maxBytes = fileLimit(options.maxBytes);
    const size = num(data.size);
    if (size > maxBytes) throw validationError("file_too_large", "GitHub file exceeds the " + maxBytes.toString() + " byte limit");
    let content = str(data.content);
    if (str(data.encoding) !== "base64" || (content.length === 0 && size > 0)) {
        const blob = githubGet(repositoryPath(owner, name) + "/git/blobs/" + segment(str(data.sha), "blob SHA")).json() as ApiContent;
        if (str(blob.encoding) !== "base64") throw validationError("unsupported_encoding", "GitHub blob is not base64 encoded");
        content = str(blob.content);
    }
    const bytes = decodeBase64(content);
    if (bytes.length > maxBytes) throw validationError("file_too_large", "Decoded GitHub file exceeds the configured limit");
    return { path: str(data.path), sha: str(data.sha), size: size, htmlUrl: data.html_url ?? null, bytes: bytes };
}

function githubGet(
    path: string,
    query?: Map<string, string>,
    accept: string = "application/vnd.github+json",
): Response {
    const suffix = query === undefined ? "" : querySuffix(query);
    return requireOk(get(GITHUB_API + path.slice(1) + suffix, authHeaders(accept)));
}

function githubGetNullable(path: string, query?: Map<string, string>): Response | null {
    const suffix = query === undefined ? "" : querySuffix(query);
    const response = get(GITHUB_API + path.slice(1) + suffix, authHeaders());
    if (response.status === 404) return null;
    return requireOk(response);
}

function githubPost(path: string, body: string | {}): Response {
    return requireOk(post(GITHUB_API + path.slice(1), body, authHeaders()));
}

function githubPostGraphQl(body: string): Response {
    return requireOk(post(GRAPHQL_API, body, authHeaders()));
}

function githubPut(path: string, body: string): Response {
    return requireOk(put(GITHUB_API + path.slice(1), body, authHeaders()));
}

function githubPatch(path: string, body: string): Response {
    return requireOk(githubPatchRaw(path, body));
}

function githubPatchRaw(path: string, body: string | {}): Response {
    return patch(GITHUB_API + path.slice(1), body, authHeaders());
}

function githubDelete(path: string): Response {
    return requireOk(delete(GITHUB_API + path.slice(1), authHeaders()));
}

function authHeaders(accept: string = "application/vnd.github+json"): Map<string, string> {
    const token = secrets.get("GITHUB_TOKEN");
    if (token === undefined) throw validationError("missing_token", "GITHUB_TOKEN is not bound");
    const headers = new Map<string, string>();
    headers.set("Authorization", "Bearer " + token);
    headers.set("Accept", accept);
    headers.set("Content-Type", "application/json");
    headers.set("X-GitHub-Api-Version", API_VERSION);
    headers.set("User-Agent", "submilli-github/0.1.0");
    return headers;
}

function requireOk(response: Response): Response {
    if (response.ok) return response;
    throw githubError(response);
}

function githubError(response: Response, overrideCode: string = "", overrideMessage: string = ""): GitHubError {
    let code = overrideCode.length > 0 ? overrideCode : statusCode(response);
    let message = overrideMessage.length > 0
        ? overrideMessage
        : "GitHub request failed: HTTP " + response.status.toString() + " " + response.statusText;
    let documentationUrl = "";
    const errors: string[] = [];
    if (response.body.startsWith("{")) {
        const envelope = response.json() as ApiError;
        if (overrideMessage.length === 0) message = envelope.message ?? message;
        documentationUrl = str(envelope.documentation_url);
        for (const rawDetail of array(envelope.errors)) {
            let text = "";
            if (typeof rawDetail === "string") {
                text = rawDetail;
            } else if (rawDetail !== null) {
                const detail = rawDetail as ApiErrorDetail;
                text = str(detail.message);
                if (text.length === 0) {
                    text = str(detail.resource);
                    if (detail.field) text += "." + detail.field;
                    if (detail.code) text += ": " + detail.code;
                }
            }
            if (text.length > 0) errors.push(text);
        }
    }
    const remaining = numberHeader(response, "x-ratelimit-remaining");
    if (overrideCode.length === 0 && response.status === 403 && remaining === 0) code = "rate_limited";
    return new GitHubError(
        code,
        message,
        response.status,
        header(response, "x-github-request-id"),
        documentationUrl,
        numberHeader(response, "retry-after"),
        remaining,
        nullableHeader(response, "x-ratelimit-reset"),
        errors,
    );
}

function statusCode(response: Response): string {
    if (response.status === 401) return "unauthorized";
    if (response.status === 403) return "forbidden";
    if (response.status === 404) return "not_found";
    if (response.status === 409) return "conflict";
    if (response.status === 422) return "validation_failed";
    if (response.status === 429) return "rate_limited";
    return "http_error";
}

function validationError(code: string, message: string): GitHubError {
    return new GitHubError(code, message, 0, "", "", null, null, null, []);
}

function toRfc3339(value: string, param: string): string {
    try {
        return Temporal.Instant.from(value).toString();
    } catch (e) {
        throw validationError("invalid_timestamp", param + " is not an ISO 8601 timestamp: " + (e as Error).message);
    }
}

function pageQuery(options: PageOptions): Map<string, string> {
    const query = new Map<string, string>();
    query.set("per_page", pageLimit(options.limit).toString());
    const page = pageToken(options.pageToken, true);
    if (page !== undefined) query.set("page", page);
    return query;
}

function searchQuery(value: string, options: SearchOptions): Map<string, string> {
    const query = pageQuery(options);
    query.set("q", value);
    putQuery(query, "sort", options.sort);
    putQuery(query, "order", options.order);
    return query;
}

function pageFrom<T>(response: Response, items: T[]): PageResult<T> {
    const next = nextPageToken(response);
    return { items: items, nextPageToken: next, isComplete: next.length === 0 };
}

function searchPageFrom<T>(
    response: Response,
    items: T[],
    totalCount?: number | null,
    incomplete?: boolean | null,
): SearchPageResult<T> {
    const next = nextPageToken(response);
    return {
        items: items,
        nextPageToken: next,
        isComplete: next.length === 0,
        totalCount: num(totalCount),
        incompleteResults: incomplete === true,
    };
}

function nextPageToken(response: Response): string {
    const link = header(response, "link");
    if (link.length === 0) return "";
    for (const part of link.split(",")) {
        if (part.indexOf("rel=\"next\"") < 0) continue;
        let marker = "?page=";
        let start = part.indexOf(marker);
        if (start < 0) {
            marker = "&page=";
            start = part.indexOf(marker);
        }
        if (start < 0) return "";
        const valueStart = start + marker.length;
        let end = part.length;
        const amp = part.indexOf("&", valueStart);
        const close = part.indexOf(">", valueStart);
        if (amp >= 0 && amp < end) end = amp;
        if (close >= 0 && close < end) end = close;
        const cursor = part.slice(valueStart, end);
        return pageToken(cursor, true) === undefined ? "" : cursor;
    }
    return "";
}

function pageLimit(value: number | undefined): number {
    const limit = value ?? DEFAULT_LIMIT;
    if (!Number.isInteger(limit) || limit < 1 || limit > MAX_LIMIT) {
        throw validationError("invalid_page_size", "limit must be between 1 and " + MAX_LIMIT.toString());
    }
    return limit;
}

function pageToken(value: string | undefined, numeric: boolean): string | undefined {
    if (value === undefined || value.length === 0) return undefined;
    if (numeric) {
        for (let i = 0; i < value.length; i += 1) {
            const code = value.charCodeAt(i);
            if (code < 48 || code > 57) throw validationError("invalid_page_token", "REST page token must contain only digits");
        }
        if (Number(value) < 1) throw validationError("invalid_page_token", "REST page token must be positive");
    }
    return value;
}

function scopedSearch(query: string, owner: string, name: string, kind: string): string {
    requireText(query, "query");
    checkedRepository(owner, name);
    const syntax = searchSyntax(query);
    if (SEARCH_SCOPE.test(syntax) || SEARCH_KIND.test(syntax) || SEARCH_OR.test(syntax)) {
        throw validationError("unsafe_search_query", "Repository-scoped search cannot contain scope-changing qualifiers or OR");
    }
    const suffix = kind.length === 0 ? "" : " " + kind;
    return query + " repo:" + owner + "/" + name + suffix;
}

function searchSyntax(query: string): string {
    let quoted = false;
    let quotedKind = false;
    let syntax = "";
    for (let i = 0; i < query.length; i += 1) {
        const char = query.charAt(i);
        if (char === "\"") {
            // Search endpoints differ in escape syntax. Refuse ambiguous quotes rather than
            // letting our phrase boundary disagree with the remote parser.
            if (i > 0 && query.charAt(i - 1) === "\\") {
                throw validationError("unsafe_search_query", "Repository-scoped search cannot contain escaped quotes");
            }
            if (!quoted) {
                // A quoted qualifier value is active syntax, unlike a standalone phrase.
                quotedKind = SEARCH_KIND_PREFIX.test(syntax.toLowerCase());
                if (!quotedKind) syntax += " ";
            } else {
                syntax += " ";
                quotedKind = false;
            }
            quoted = !quoted;
            continue;
        }
        if (quoted && !quotedKind) continue;
        // Fold fullwidth ASCII only for validation. Send the caller's original query unchanged.
        const code = query.charCodeAt(i);
        syntax += code >= 0xff01 && code <= 0xff5e ? String.fromCharCode(code - 0xfee0) : char;
    }
    if (quoted) throw validationError("unsafe_search_query", "Search query contains an unterminated quote");
    return syntax.toLowerCase();
}

// GitHub repository identities are case-insensitive. Validate before folding so a Unicode
// character that folds to ASCII cannot pass the identifier grammar.
function canonicalRepository(owner: string, name: string): RepositoryRef {
    const context = checkedRepository(owner, name);
    return { owner: context.owner, name: context.repo };
}
function checkedRepository(owner: string, name: string): RepositoryCapabilityContext {
    requireText(owner, "repository owner");
    requireText(name, "repository name");
    if (!OWNER_NAME.test(owner)) {
        throw validationError("invalid_input", "repository owner may contain only letters, digits, hyphens, and underscores");
    }
    if (!REPOSITORY_NAME.test(name) || name === "." || name === "..") {
        throw validationError("invalid_input", "repository name may contain only letters, digits, hyphens, underscores, and periods, and cannot be . or ..");
    }
    return { owner: owner.toLowerCase(), repo: name.toLowerCase(), branch: null, path: null, ref: null, treeSha: null, head: null, base: null };
}

function repositoryContext(owner: string, name: string, field: string, value: string): RepositoryCapabilityContext {
    const context = checkedRepository(owner, name);
    if (field === "branch") context.branch = value;
    if (field === "path") context.path = value;
    if (field === "ref") context.ref = value;
    if (field === "treeSha") context.treeSha = value;
    return context;
}

function repositoryNumberContext(owner: string, name: string, number: number): RepositoryNumberCapabilityContext {
    requireIssueNumber(number);
    const context = checkedRepository(owner, name);
    return { owner: context.owner, repo: context.repo, number: number, base: null };
}

function repositoryPath(owner: string, name: string): string {
    const context = checkedRepository(owner, name);
    return "/repos/" + encodeComponent(context.owner) + "/" + encodeComponent(context.repo);
}

// The ref a contents request names. An empty ref is not sent, so it is the default branch, which
// the check reports as null.
function namedRef(ref: string | undefined): string | null {
    return ref === undefined || ref.length === 0 ? null : ref;
}

// One path segment. `.` and `..` are refused: a URL parser reads them, escaped or not, as the
// current and the parent segment, and the request would go to another path.
function segment(value: string, label: string): string {
    requireText(value, label);
    if (value === "." || value === "..") throw validationError("invalid_input", label + " cannot be . or ..");
    return encodeComponent(value);
}

function repositoryFilePath(path: string): string {
    requireText(path, "repository path");
    if (path.startsWith("/") || path.endsWith("/")) {
        throw validationError("invalid_path", "Repository path must be relative and must not end with /");
    }
    const parts = path.split("/");
    for (const part of parts) {
        if (part.length === 0 || part === "." || part === "..") {
            throw validationError("invalid_path", "Repository path cannot contain empty, . or .. segments");
        }
    }
    return path;
}

function encodedFilePath(path: string): string {
    const encoded: string[] = [];
    for (const part of path.split("/")) encoded.push(encodeComponent(part));
    return encoded.join("/");
}

function branchName(value: string): string {
    requireText(value, "branch");
    if (value.startsWith("refs/") || value.startsWith("/") || value.endsWith("/") || value.indexOf("..") >= 0) {
        throw validationError("invalid_branch", "Branch must be a short branch name without refs/, traversal, or edge slashes");
    }
    for (const part of value.split("/")) {
        if (part.length === 0 || part === "." || part === "..") {
            throw validationError("invalid_branch", "Branch contains an invalid path segment");
        }
    }
    return value;
}

function requireSha(value: string, label: string): void {
    if (value.length !== 40 && value.length !== 64) {
        throw validationError("invalid_sha", label + " must be a 40- or 64-character hexadecimal SHA");
    }
    for (let i = 0; i < value.length; i += 1) {
        const code = value.charCodeAt(i);
        const digit = code >= 48 && code <= 57;
        const lower = code >= 97 && code <= 102;
        const upper = code >= 65 && code <= 70;
        if (!digit && !lower && !upper) throw validationError("invalid_sha", label + " must be hexadecimal");
    }
}

function issueNumber(value: number): string {
    requireIssueNumber(value);
    return value.toString();
}

function requireIssueNumber(value: number): void {
    if (value < 1 || value !== Math.floor(value)) {
        throw validationError("invalid_number", "Issue or pull request number must be a positive integer");
    }
}

function requireText(value: string, label: string): void {
    if (value.trim().length === 0) throw validationError("invalid_input", label + " cannot be empty");
}

function requireFields(fields: string[]): void {
    if (fields.length === 0) throw validationError("empty_update", "Update input must include at least one field");
}

function jsonProperty(name: string, valueJson: string): string {
    return JSON.stringify(name) + ":" + valueJson;
}

function jsonObject(fields: string[]): string {
    return "{" + fields.join(",") + "}";
}

function stringArrayJson(values: string[]): string {
    const encoded: string[] = [];
    for (const value of values) encoded.push(JSON.stringify(value));
    return "[" + encoded.join(",") + "]";
}

function booleanJson(value: boolean): string {
    return value ? "true" : "false";
}

function validateChanges(changes: CommitFileChange[]): void {
    if (changes.length === 0) throw validationError("empty_commit", "Commit must include at least one file change");
    const paths = new Map<string, boolean>();
    let bytes = 0;
    for (const change of changes) {
        const path = repositoryFilePath(change.path);
        if (paths.has(path)) throw validationError("duplicate_path", "Commit contains duplicate path " + path);
        paths.set(path, true);
        if (change.type === "write") {
            if (change.content === undefined) throw validationError("missing_content", "Write change for " + path + " requires content");
            if (typeof change.content === "string") {
                bytes += new TextEncoder().encode(change.content).length;
            } else {
                bytes += change.content.length;
            }
        } else if (change.type === "delete") {
            if (change.content !== undefined) {
                throw validationError("unexpected_content", "Delete change for " + path + " must not include content");
            }
        } else {
            throw validationError("invalid_change", "Commit change type must be write or delete");
        }
    }
    if (bytes > MAX_FILE_BYTES) {
        throw validationError("commit_too_large", "Aggregate commit content exceeds " + MAX_FILE_BYTES.toString() + " bytes");
    }
}

function validateReviewComment(comment: ReviewCommentInput): void {
    repositoryFilePath(comment.path);
    requireText(comment.body, "review comment body");
    if (comment.line < 1 || comment.line !== Math.floor(comment.line)) {
        throw validationError("invalid_line", "Review comment line must be a positive integer");
    }
    if (comment.startLine !== undefined) {
        if (comment.startLine < 1 || comment.startLine > comment.line || comment.startLine !== Math.floor(comment.startLine)) {
            throw validationError("invalid_line", "Review comment startLine must be a positive integer no greater than line");
        }
        if (comment.startSide === undefined) {
            throw validationError("missing_start_side", "Review comment startSide is required with startLine");
        }
    }
}

function fileLimit(value: number | undefined): number {
    const limit = value ?? MAX_FILE_BYTES;
    if (limit < 1 || limit > MAX_FILE_BYTES || limit !== Math.floor(limit)) {
        throw validationError("invalid_file_limit", "maxBytes must be an integer between 1 and " + MAX_FILE_BYTES.toString());
    }
    return limit;
}

function decodeBase64(value: string): Uint8Array {
    return Uint8Array.fromBase64(value.replaceAll("\n", "").replaceAll("\r", ""));
}

function querySuffix(query: Map<string, string>): string {
    const encoded = encodeQuery(query);
    return encoded.length === 0 ? "" : "?" + encoded;
}

function putQuery(query: Map<string, string>, key: string, value: string | undefined): void {
    if (value !== undefined && value.length > 0) query.set(key, value);
}

function header(response: Response, name: string): string {
    return response.headers.get(name) ?? "";
}

function nullableHeader(response: Response, name: string): string | null {
    return response.headers.get(name) ?? null;
}

function numberHeader(response: Response, name: string): number | null {
    const value = response.headers.get(name);
    if (value === undefined || value.length === 0) return null;
    const parsed = Number(value);
    return isNaN(parsed) ? null : parsed;
}

function str(value?: string | null): string {
    return value ?? "";
}

function num(value?: number | null): number {
    return value ?? 0;
}

function array<T>(value?: T[] | null): T[] {
    const empty: T[] = [];
    return value ?? empty;
}

function required<T>(value: T | null | undefined, label: string): T {
    if (value === null || value === undefined) throw validationError("invalid_response", "GitHub response is missing " + label);
    return value;
}

function userFrom(data?: ApiUser | null): User {
    if (!data) return { login: "", id: 0, htmlUrl: "", avatarUrl: "", type: "" };
    return {
        login: str(data.login),
        id: num(data.id),
        htmlUrl: str(data.html_url),
        avatarUrl: str(data.avatar_url),
        type: str(data.type),
    };
}

function teamFrom(data: ApiTeam): Team {
    return {
        id: num(data.id),
        name: str(data.name),
        slug: str(data.slug),
        description: data.description ?? null,
        privacy: str(data.privacy),
        htmlUrl: str(data.html_url),
        organization: str(data.organization?.login),
    };
}

function repositoryFrom(data: ApiRepository): Repository {
    return {
        id: num(data.id),
        name: str(data.name),
        fullName: str(data.full_name),
        description: data.description ?? null,
        htmlUrl: str(data.html_url),
        private: data.private === true,
        fork: data.fork === true,
        archived: data.archived === true,
        defaultBranch: str(data.default_branch),
        language: data.language ?? null,
        topics: array(data.topics),
        stargazersCount: num(data.stargazers_count),
        forksCount: num(data.forks_count),
        openIssuesCount: num(data.open_issues_count),
        createdAt: str(data.created_at),
        updatedAt: str(data.updated_at),
        pushedAt: data.pushed_at ?? null,
    };
}

function branchFrom(data: ApiBranch): Branch {
    return {
        name: str(data.name),
        sha: str(data.commit?.sha),
        protected: data.protected === true,
    };
}

function commitFrom(data: ApiCommit, detail: CommitDetail): Commit {
    const body = data.commit;
    const author = body?.author;
    const committer = body?.committer;
    const apiStats = data.stats;
    let stats: CommitStats | null = null;
    if (detail !== "none" && apiStats) {
        stats = {
            additions: num(apiStats.additions),
            deletions: num(apiStats.deletions),
            total: num(apiStats.total),
        };
    }
    const files: CommitFile[] = [];
    if (detail !== "none") {
        for (const file of array(data.files)) {
            files.push({
                filename: str(file.filename),
                status: str(file.status),
                additions: num(file.additions),
                deletions: num(file.deletions),
                changes: num(file.changes),
                previousFilename: file.previous_filename ?? null,
                patch: detail === "patch" ? (file.patch ?? null) : null,
            });
        }
    }
    return {
        sha: str(data.sha),
        htmlUrl: str(data.html_url),
        message: str(body?.message),
        author: data.author ? userFrom(data.author) : null,
        committer: data.committer ? userFrom(data.committer) : null,
        authorName: str(author?.name),
        authorEmail: str(author?.email),
        authoredAt: str(author?.date),
        committerName: str(committer?.name),
        committerEmail: str(committer?.email),
        committedAt: str(committer?.date),
        stats: stats,
        files: files,
    };
}

function releaseFrom(data: ApiRelease): Release {
    return {
        id: num(data.id),
        tagName: str(data.tag_name),
        name: data.name ?? null,
        body: data.body ?? null,
        htmlUrl: str(data.html_url),
        draft: data.draft === true,
        prerelease: data.prerelease === true,
        createdAt: str(data.created_at),
        publishedAt: data.published_at ?? null,
        author: userFrom(data.author),
    };
}

function labelFrom(data: ApiLabel): Label {
    return {
        id: num(data.id),
        name: str(data.name),
        color: str(data.color),
        description: data.description ?? null,
        default: data.default === true,
    };
}

function milestoneFrom(data?: ApiMilestone | null): Milestone | null {
    if (!data) return null;
    return {
        id: num(data.id),
        number: num(data.number),
        title: str(data.title),
        description: data.description ?? null,
        state: str(data.state),
        dueOn: data.due_on ?? null,
    };
}

function reactionsFrom(data?: ApiReactionSummary | null): ReactionSummary | null {
    if (!data) return null;
    return {
        totalCount: reactionCount(data.total_count), thumbsUp: reactionCount(data["+1"]),
        thumbsDown: reactionCount(data["-1"]), laugh: reactionCount(data.laugh),
        hooray: reactionCount(data.hooray), confused: reactionCount(data.confused),
        heart: reactionCount(data.heart), rocket: reactionCount(data.rocket), eyes: reactionCount(data.eyes),
    };
}

function reactionCount(value?: number | null): number | null {
    if (value === undefined || value === null) return null;
    if (!Number.isSafeInteger(value) || value < 0) throw validationError("invalid_response", "GitHub reaction count must be a nonnegative integer");
    return value;
}

function commentFrom(data: ApiComment): Comment {
    return {
        reactions: reactionsFrom(data.reactions),
        id: num(data.id),
        body: str(data.body),
        htmlUrl: str(data.html_url),
        user: userFrom(data.user),
        authorAssociation: str(data.author_association),
        createdAt: str(data.created_at),
        updatedAt: str(data.updated_at),
    };
}

function issueFrom(data: ApiIssue): Issue {
    const labels: Label[] = [];
    for (const label of array(data.labels)) labels.push(labelFrom(label));
    const assignees: User[] = [];
    for (const user of array(data.assignees)) assignees.push(userFrom(user));
    return {
        reactions: reactionsFrom(data.reactions),
        id: num(data.id),
        number: num(data.number),
        title: str(data.title),
        body: data.body ?? null,
        state: str(data.state).toLowerCase(),
        stateReason: data.state_reason ?? null,
        locked: data.locked === true,
        htmlUrl: str(data.html_url),
        user: userFrom(data.user),
        labels: labels,
        assignees: assignees,
        milestone: milestoneFrom(data.milestone),
        comments: num(data.comments),
        createdAt: str(data.created_at),
        updatedAt: str(data.updated_at),
        closedAt: data.closed_at ?? null,
    };
}

function graphQlIssueFrom(data: ApiGraphQlIssue): Issue {
    const labels: Label[] = [];
    for (const label of array(data.labels?.nodes)) labels.push(labelFrom(label));
    const assignees: User[] = [];
    for (const user of array(data.assignees?.nodes)) assignees.push(userFrom(user));
    return {
        reactions: null,
        id: num(data.id),
        number: num(data.number),
        title: str(data.title),
        body: data.body ?? null,
        state: str(data.state).toLowerCase(),
        stateReason: data.state_reason ?? null,
        locked: data.locked === true,
        htmlUrl: str(data.html_url),
        user: userFrom(data.user),
        labels: labels,
        assignees: assignees,
        milestone: milestoneFrom(data.milestone),
        comments: num(data.comments?.totalCount),
        createdAt: str(data.created_at),
        updatedAt: str(data.updated_at),
        closedAt: data.closed_at ?? null,
    };
}

function pullRefFrom(data?: ApiPullRef | null): PullRequestRef {
    return {
        label: str(data?.label),
        ref: str(data?.ref),
        sha: str(data?.sha),
        user: userFrom(data?.user),
        repository: data?.repo ? repositoryFrom(data.repo) : null,
    };
}

function pullRequestFrom(data: ApiPullRequest): PullRequest {
    const labels: Label[] = [];
    for (const label of array(data.labels)) labels.push(labelFrom(label));
    const assignees: User[] = [];
    for (const user of array(data.assignees)) assignees.push(userFrom(user));
    const reviewers: User[] = [];
    for (const user of array(data.requested_reviewers)) reviewers.push(userFrom(user));
    return {
        reactions: reactionsFrom(data.reactions),
        id: num(data.id),
        number: num(data.number),
        title: str(data.title),
        body: data.body ?? null,
        state: str(data.state),
        draft: data.draft === true,
        merged: data.merged === true,
        mergeable: data.mergeable ?? null,
        mergeableState: str(data.mergeable_state),
        htmlUrl: str(data.html_url),
        user: userFrom(data.user),
        labels: labels,
        assignees: assignees,
        requestedReviewers: reviewers,
        head: pullRefFrom(data.head),
        base: pullRefFrom(data.base),
        additions: num(data.additions),
        deletions: num(data.deletions),
        changedFiles: num(data.changed_files),
        commits: num(data.commits),
        comments: num(data.comments),
        reviewComments: num(data.review_comments),
        createdAt: str(data.created_at),
        updatedAt: str(data.updated_at),
        closedAt: data.closed_at ?? null,
        mergedAt: data.merged_at ?? null,
    };
}

function pullFileFrom(data: ApiPullFile): PullRequestFile {
    return {
        sha: str(data.sha),
        filename: str(data.filename),
        status: str(data.status),
        additions: num(data.additions),
        deletions: num(data.deletions),
        changes: num(data.changes),
        previousFilename: data.previous_filename ?? null,
        patch: data.patch ?? null,
        blobUrl: str(data.blob_url),
    };
}

function reviewFrom(data: ApiReview): PullRequestReview {
    return {
        id: num(data.id),
        state: str(data.state),
        body: str(data.body),
        htmlUrl: str(data.html_url),
        user: userFrom(data.user),
        commitId: str(data.commit_id),
        submittedAt: data.submitted_at ?? null,
        authorAssociation: str(data.author_association),
    };
}

function rateResourceFrom(data?: ApiRateResource | null): RateLimitResource {
    return {
        limit: num(data?.limit),
        remaining: num(data?.remaining),
        used: num(data?.used),
        resetAt: data?.reset?.toString() ?? "",
    };
}

function getCommitUnchecked(owner: string, name: string, ref: string, detail: CommitDetail): Commit | null {
    const response = githubGetNullable(repositoryPath(owner, name) + "/commits/" + segment(ref, "commit ref"));
    return response === null ? null : commitFrom(response.json() as ApiCommit, detail);
}
