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
    login?: string;
    id?: number;
    html_url?: string;
    avatar_url?: string;
    type?: string;
}

interface ApiOwner {
    login?: string;
}

interface ApiRepository {
    id?: number;
    name?: string;
    full_name?: string;
    description?: string;
    html_url?: string;
    private?: boolean;
    fork?: boolean;
    archived?: boolean;
    default_branch?: string;
    language?: string;
    topics?: string[];
    stargazers_count?: number;
    forks_count?: number;
    open_issues_count?: number;
    created_at?: string;
    updated_at?: string;
    pushed_at?: string;
}

interface ApiBranchCommit {
    sha?: string;
}

interface ApiBranch {
    name?: string;
    commit?: ApiBranchCommit;
    protected?: boolean;
}

interface ApiContent {
    type?: string;
    name?: string;
    path?: string;
    sha?: string;
    size?: number;
    html_url?: string;
    encoding?: string;
    content?: string;
}

interface ApiTreeEntry {
    path?: string;
    mode?: string;
    type?: string;
    sha?: string;
    size?: number;
}

interface ApiTree {
    sha?: string;
    truncated?: boolean;
    tree?: ApiTreeEntry[];
}

interface ApiGitActor {
    name?: string;
    email?: string;
    date?: string;
}

interface ApiCommitBody {
    message?: string;
    author?: ApiGitActor;
    committer?: ApiGitActor;
    tree?: ApiBranchCommit;
}

interface ApiCommitStats {
    additions?: number;
    deletions?: number;
    total?: number;
}

interface ApiCommitFile {
    filename?: string;
    status?: string;
    additions?: number;
    deletions?: number;
    changes?: number;
    previous_filename?: string;
    patch?: string;
}

interface ApiCommit {
    sha?: string;
    html_url?: string;
    commit?: ApiCommitBody;
    author?: ApiUser;
    committer?: ApiUser;
    stats?: ApiCommitStats;
    files?: ApiCommitFile[];
}

interface ApiRelease {
    id?: number;
    tag_name?: string;
    name?: string;
    body?: string;
    html_url?: string;
    draft?: boolean;
    prerelease?: boolean;
    created_at?: string;
    published_at?: string;
    author?: ApiUser;
}

interface ApiLabel {
    id?: number;
    name?: string;
    color?: string;
    description?: string;
    default?: boolean;
}

interface ApiMilestone {
    id?: number;
    number?: number;
    title?: string;
    description?: string;
    state?: string;
    due_on?: string;
}

interface ApiComment {
    id?: number;
    body?: string;
    html_url?: string;
    user?: ApiUser;
    author_association?: string;
    created_at?: string;
    updated_at?: string;
}

interface ApiIssue {
    id?: number;
    number?: number;
    title?: string;
    body?: string;
    state?: string;
    state_reason?: string;
    locked?: boolean;
    html_url?: string;
    user?: ApiUser;
    labels?: ApiLabel[];
    assignees?: ApiUser[];
    milestone?: ApiMilestone;
    comments?: number;
    created_at?: string;
    updated_at?: string;
    closed_at?: string;
    pull_request?: unknown;
}

interface ApiPullRef {
    label?: string;
    ref?: string;
    sha?: string;
    user?: ApiUser;
    repo?: ApiRepository;
}

interface ApiPullRequest {
    id?: number;
    number?: number;
    title?: string;
    body?: string;
    state?: string;
    draft?: boolean;
    merged?: boolean;
    mergeable?: boolean;
    mergeable_state?: string;
    html_url?: string;
    user?: ApiUser;
    labels?: ApiLabel[];
    assignees?: ApiUser[];
    requested_reviewers?: ApiUser[];
    head?: ApiPullRef;
    base?: ApiPullRef;
    additions?: number;
    deletions?: number;
    changed_files?: number;
    commits?: number;
    comments?: number;
    review_comments?: number;
    created_at?: string;
    updated_at?: string;
    closed_at?: string;
    merged_at?: string;
}

interface ApiPullFile {
    sha?: string;
    filename?: string;
    status?: string;
    additions?: number;
    deletions?: number;
    changes?: number;
    previous_filename?: string;
    patch?: string;
    blob_url?: string;
}

interface ApiReview {
    id?: number;
    state?: string;
    body?: string;
    html_url?: string;
    user?: ApiUser;
    commit_id?: string;
    submitted_at?: string;
    author_association?: string;
}

interface ApiMergeResult {
    merged?: boolean;
    message?: string;
    sha?: string;
}

interface ApiTeam {
    id?: number;
    name?: string;
    slug?: string;
    description?: string;
    privacy?: string;
    html_url?: string;
    organization?: ApiOwner;
}

interface ApiRateResource {
    limit?: number;
    remaining?: number;
    used?: number;
    reset?: number;
}

interface ApiRateResources {
    core?: ApiRateResource;
    search?: ApiRateResource;
    graphql?: ApiRateResource;
}

interface ApiRateLimit {
    resources?: ApiRateResources;
}

interface ApiSearchRepositoryItem {
    id?: number;
    name?: string;
    full_name?: string;
    description?: string;
    html_url?: string;
    private?: boolean;
    fork?: boolean;
    archived?: boolean;
    default_branch?: string;
    language?: string;
    topics?: string[];
    stargazers_count?: number;
    forks_count?: number;
    open_issues_count?: number;
    created_at?: string;
    updated_at?: string;
    pushed_at?: string;
    score?: number;
}

interface ApiCodeSearchItem {
    name?: string;
    path?: string;
    sha?: string;
    html_url?: string;
    repository?: ApiRepository;
}

interface ApiUserSearchItem {
    login?: string;
    id?: number;
    html_url?: string;
    avatar_url?: string;
    type?: string;
    score?: number;
}

interface ApiSearchRepositories {
    total_count?: number;
    incomplete_results?: boolean;
    items?: ApiSearchRepositoryItem[];
}

interface ApiSearchCode {
    total_count?: number;
    incomplete_results?: boolean;
    items?: ApiCodeSearchItem[];
}

interface ApiSearchUsers {
    total_count?: number;
    incomplete_results?: boolean;
    items?: ApiUserSearchItem[];
}

interface ApiSearchIssues {
    total_count?: number;
    incomplete_results?: boolean;
    items?: ApiIssue[];
}

interface ApiError {
    message?: string;
    documentation_url?: string;
    status?: string;
    errors?: unknown[];
}

interface ApiErrorDetail {
    resource?: string;
    field?: string;
    code?: string;
    message?: string;
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
    nodes?: ApiGraphQlIssue[];
    pageInfo?: GraphQlPageInfo;
}

interface ApiGraphQlCount {
    totalCount?: number;
}

interface ApiGraphQlLabels {
    nodes?: ApiLabel[];
}

interface ApiGraphQlUsers {
    nodes?: ApiUser[];
}

interface ApiGraphQlIssue {
    id?: number;
    number?: number;
    title?: string;
    body?: string;
    state?: string;
    state_reason?: string;
    locked?: boolean;
    html_url?: string;
    user?: ApiUser;
    labels?: ApiGraphQlLabels;
    assignees?: ApiGraphQlUsers;
    milestone?: ApiMilestone;
    comments?: ApiGraphQlCount;
    created_at?: string;
    updated_at?: string;
    closed_at?: string;
}

interface GraphQlRepository {
    issues?: GraphQlIssueConnection;
}

interface GraphQlIssueData {
    repository?: GraphQlRepository;
}

interface GraphQlPageInfo {
    hasNextPage?: boolean;
    endCursor?: string;
}

interface GraphQlErrorItem {
    message?: string;
}

interface GraphQlIssueEnvelope {
    data?: GraphQlIssueData;
    errors?: GraphQlErrorItem[];
}

interface GitBlobBody {
    content: string;
    encoding: string;
}

interface ApiCreatedSha {
    sha?: string;
}

interface ApiGitCommit {
    tree?: ApiBranchCommit;
}

interface ApiGitRefObject {
    sha?: string;
}

interface ApiGitRef {
    object?: ApiGitRefObject;
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

interface RepositoryCapabilityContext {
    owner: string;
    repo: string;
    branch?: string;
    path?: string;
    ref?: string;
    treeSha?: string;
}

interface RepositoryNumberCapabilityContext {
    owner: string;
    repo: string;
    number: number;
}

interface PageRequest {
    limit: number | null;
    cursor: string | null;
}

interface SearchRequest {
    sort: string | null;
    order: string | null;
    limit: number | null;
    cursor: string | null;
}

interface FileReadRequest {
    ref: string | null;
    maxBytes: number | null;
}

const ISSUE_QUERY = "query($owner:String!,$name:String!,$first:Int!,$after:String,$states:[IssueState!],$labels:[String!],$orderField:IssueOrderField!,$direction:OrderDirection!){repository(owner:$owner,name:$name){issues(first:$first,after:$after,states:$states,labels:$labels,orderBy:{field:$orderField,direction:$direction}){nodes{id:databaseId number title body state state_reason:stateReason locked html_url:url comments{totalCount} created_at:createdAt updated_at:updatedAt closed_at:closedAt user:author{login html_url:url avatar_url:avatarUrl type:__typename}labels(first:100){nodes{name color description}}assignees(first:100){nodes{login id:databaseId html_url:url avatar_url:avatarUrl type:__typename}}milestone{number title description state due_on:dueOn}}pageInfo{hasNextPage endCursor}}}}";

/** Retrieve the authenticated GitHub user.
 * @capability github.com/viewer.get {}
 */
export function getViewer(): User {
    check("github.com/viewer.get", {});
    return userFrom(githubGet("/user").json() as ApiUser);
}

/** List teams visible to the authenticated user.
 * @capability github.com/teams.list {}
 */
export function listTeams(options: PageOptions | null = null): PageResult<Team> {
    const limit = options === null ? null : options.limit;
    const cursor = options === null ? null : options.pageToken;
    check("github.com/teams.list", {});
    const response = githubGet("/user/teams", pageQuery({ limit: limit, cursor: cursor }));
    const data = response.json() as ApiTeam[];
    const items: Team[] = [];
    for (const item of data) items.push(teamFrom(item));
    return pageFrom(response, items);
}

/** List members of an organization team.
 * @capability github.com/teamMembers.list { org: string, teamSlug: string }
 */
export function listTeamMembers(org: string, teamSlug: string, options: PageOptions | null = null): PageResult<User> {
    const limit = options === null ? null : options.limit;
    const cursor = options === null ? null : options.pageToken;
    check("github.com/teamMembers.list", { org: org, teamSlug: teamSlug });
    const path = "/orgs/" + segment(org, "organization") + "/teams/" + segment(teamSlug, "team slug") + "/members";
    const response = githubGet(path, pageQuery({ limit: limit, cursor: cursor }));
    const data = response.json() as ApiUser[];
    const items: User[] = [];
    for (const item of data) items.push(userFrom(item));
    return pageFrom(response, items);
}

/** Search repositories visible to the token.
 * @capability github.com/repositories.search {}
 */
export function searchRepositories(query: string, options: SearchOptions | null = null): SearchPageResult<RepositorySearchHit> {
    const sort = options === null ? null : options.sort;
    const order = options === null ? null : options.order;
    const limit = options === null ? null : options.limit;
    const cursor = options === null ? null : options.pageToken;
    check("github.com/repositories.search", {});
    requireText(query, "query");
    const response = githubGet("/search/repositories", searchQuery(query, { sort: sort, order: order, limit: limit, cursor: cursor }));
    const data = response.json() as ApiSearchRepositories;
    const items: RepositorySearchHit[] = [];
    for (const item of array(data.items)) {
        items.push({ score: num(item.score), repository: repositoryFrom(item) });
    }
    return searchPageFrom(response, items, data.total_count, data.incomplete_results);
}

/** Search code within one repository.
 * @capability github.com/code.search { owner: string, repo: string }
 */
export function searchCode(repository: RepositoryRef, query: string, options: SearchOptions | null = null): SearchPageResult<CodeSearchHit> {
    const { owner, name } = repository;
    const sort = options === null ? null : options.sort;
    const order = options === null ? null : options.order;
    const limit = options === null ? null : options.limit;
    const cursor = options === null ? null : options.pageToken;
    const context = checkedRepository(owner, name);
    check("github.com/code.search", context);
    const scoped = scopedSearch(query, owner, name, "");
    const response = githubGet("/search/code", searchQuery(scoped, { sort: sort, order: order, limit: limit, cursor: cursor }));
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
 * @capability github.com/users.search {}
 */
export function searchUsers(query: string, options: SearchOptions | null = null): SearchPageResult<UserSearchHit> {
    const sort = options === null ? null : options.sort;
    const order = options === null ? null : options.order;
    const limit = options === null ? null : options.limit;
    const cursor = options === null ? null : options.pageToken;
    check("github.com/users.search", {});
    requireText(query, "query");
    const response = githubGet("/search/users", searchQuery(query, { sort: sort, order: order, limit: limit, cursor: cursor }));
    const data = response.json() as ApiSearchUsers;
    const items: UserSearchHit[] = [];
    for (const item of array(data.items)) items.push({ score: num(item.score), user: userFrom(item) });
    return searchPageFrom(response, items, data.total_count, data.incomplete_results);
}

/** Retrieve current REST, search, and GraphQL rate limits.
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
 * @capability github.com/repositories.get { owner: string, repo: string }
 */
export function getRepository(repository: RepositoryRef): Repository | null {
    const { owner, name } = repository;
    const context = checkedRepository(owner, name);
    check("github.com/repositories.get", context);
    const response = githubGetNullable(repositoryPath(owner, name));
    return response === null ? null : repositoryFrom(response.json() as ApiRepository);
}

/** Retrieve a branch, or null when it does not exist.
 * @capability github.com/branches.get { owner: string, repo: string, branch: string }
 */
export function getBranch(repository: RepositoryRef, branch: string): Branch | null {
    const { owner, name } = repository;
    const context = repositoryContext(owner, name, "branch", branch);
    check("github.com/branches.get", context);
    requireText(branch, "branch");
    const response = githubGetNullable(repositoryPath(owner, name) + "/branches/" + encodeComponent(branch));
    return response === null ? null : branchFrom(response.json() as ApiBranch);
}

/** List repository branches.
 * @capability github.com/branches.list { owner: string, repo: string }
 */
export function listBranches(repository: RepositoryRef, options: PageOptions | null = null): PageResult<Branch> {
    const { owner, name } = repository;
    const limit = options === null ? null : options.limit;
    const cursor = options === null ? null : options.pageToken;
    const context = checkedRepository(owner, name);
    check("github.com/branches.list", context);
    const response = githubGet(repositoryPath(owner, name) + "/branches", pageQuery({ limit: limit, cursor: cursor }));
    const data = response.json() as ApiBranch[];
    const items: Branch[] = [];
    for (const item of data) items.push(branchFrom(item));
    return pageFrom(response, items);
}

/** List repository commits.
 * @capability github.com/commits.list { owner: string, repo: string }
 */
export function listCommits(repository: RepositoryRef, options: ListCommitsOptions | null = null): PageResult<Commit> {
    const { owner, name } = repository;
    const sha = options === null ? null : options.sha;
    const path = options === null ? null : options.path;
    const author = options === null ? null : options.author;
    const since = options === null ? null : options.since;
    const until = options === null ? null : options.until;
    const limit = options === null ? null : options.limit;
    const cursor = options === null ? null : options.pageToken;
    const context = checkedRepository(owner, name);
    check("github.com/commits.list", context);
    const query = pageQuery({ limit: limit, cursor: cursor });
    putQuery(query, "sha", sha);
    putQuery(query, "path", path);
    putQuery(query, "author", author);
    putQuery(query, "since", since !== null ? toRfc3339(since, "since") : null);
    putQuery(query, "until", until !== null ? toRfc3339(until, "until") : null);
    const response = githubGet(repositoryPath(owner, name) + "/commits", query);
    const data = response.json() as ApiCommit[];
    const items: Commit[] = [];
    for (const item of data) items.push(commitFrom(item, "none"));
    return pageFrom(response, items);
}

/** Retrieve one commit with configurable detail.
 * @capability github.com/commits.get { owner: string, repo: string, ref: string }
 */
export function getCommit(repository: RepositoryRef, ref: string, detail: CommitDetail | null = null): Commit | null {
    const { owner, name } = repository;
    const context = repositoryContext(owner, name, "ref", ref);
    check("github.com/commits.get", context);
    requireText(ref, "commit ref");
    const response = githubGetNullable(repositoryPath(owner, name) + "/commits/" + encodeComponent(ref));
    return response === null ? null : commitFrom(response.json() as ApiCommit, detail === null ? "stats" : detail);
}

/** List repository releases.
 * @capability github.com/releases.list { owner: string, repo: string }
 */
export function listReleases(repository: RepositoryRef, options: PageOptions | null = null): PageResult<Release> {
    const { owner, name } = repository;
    const limit = options === null ? null : options.limit;
    const cursor = options === null ? null : options.pageToken;
    const context = checkedRepository(owner, name);
    check("github.com/releases.list", context);
    const response = githubGet(repositoryPath(owner, name) + "/releases", pageQuery({ limit: limit, cursor: cursor }));
    const data = response.json() as ApiRelease[];
    const items: Release[] = [];
    for (const item of data) items.push(releaseFrom(item));
    return pageFrom(response, items);
}

/** Retrieve the latest release, or null when the repository has none.
 * @capability github.com/releases.getLatest { owner: string, repo: string }
 */
export function getLatestRelease(repository: RepositoryRef): Release | null {
    const { owner, name } = repository;
    const context = checkedRepository(owner, name);
    check("github.com/releases.getLatest", context);
    const response = githubGetNullable(repositoryPath(owner, name) + "/releases/latest");
    return response === null ? null : releaseFrom(response.json() as ApiRelease);
}

/** Read a repository file as bytes.
 * @capability github.com/contents.readFile { owner: string, repo: string, path: string }
 */
export function readFile(repository: RepositoryRef, path: string, options: FileReadOptions | null = null): RepositoryFile | null {
    const { owner, name } = repository;
    const ref = options === null ? null : options.ref;
    const maxBytes = options === null ? null : options.maxBytes;
    const context = repositoryContext(owner, name, "path", path);
    check("github.com/contents.readFile", context);
    return readFileUnchecked(owner, name, path, { ref: ref, maxBytes: maxBytes });
}

/** Read a repository file as strict UTF-8 text.
 * @capability github.com/contents.readTextFile { owner: string, repo: string, path: string }
 */
export function readTextFile(repository: RepositoryRef, path: string, options: FileReadOptions | null = null): RepositoryTextFile | null {
    const { owner, name } = repository;
    const ref = options === null ? null : options.ref;
    const maxBytes = options === null ? null : options.maxBytes;
    const context = repositoryContext(owner, name, "path", path);
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
 * @capability github.com/contents.listDirectory { owner: string, repo: string, path: string }
 */
export function listDirectory(repository: RepositoryRef, path: string, options: DirectoryOptions | null = null): DirectoryEntry[] {
    const { owner, name } = repository;
    const ref = options === null ? null : options.ref;
    const context = repositoryContext(owner, name, "path", path);
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
            htmlUrl: nullableString(item.html_url),
        });
    }
    return items;
}

/** Retrieve a Git tree.
 * @capability github.com/trees.get { owner: string, repo: string, treeSha: string }
 */
export function getTree(repository: RepositoryRef, treeSha: string, recursive: boolean = false): RepositoryTree | null {
    const { owner, name } = repository;
    const context = repositoryContext(owner, name, "treeSha", treeSha);
    check("github.com/trees.get", context);
    requireText(treeSha, "tree SHA");
    const query = new Map<string, string>();
    if (recursive) query.set("recursive", "1");
    const response = githubGetNullable(repositoryPath(owner, name) + "/git/trees/" + encodeComponent(treeSha), query);
    if (response === null) return null;
    const data = response.json() as ApiTree;
    const entries: TreeEntry[] = [];
    for (const item of array(data.tree)) {
        entries.push({
            path: str(item.path),
            mode: str(item.mode),
            type: str(item.type),
            sha: str(item.sha),
            size: nullableNumber(item.size),
        });
    }
    return { sha: str(data.sha), truncated: data.truncated === true, entries: entries };
}

/** Create a branch at an exact commit SHA.
 * @capability github.com/branches.create { owner: string, repo: string, branch: string }
 */
export function createBranch(repository: RepositoryRef, input: CreateBranchInput): Branch {
    const { owner, name: repo } = repository;
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
 * @capability github.com/branches.delete { owner: string, repo: string, branch: string }
 */
export function deleteBranch(repository: RepositoryRef, branch: string): void {
    const { owner, name: repo } = repository;
    const context = repositoryContext(owner, repo, "branch", branch);
    check("github.com/branches.delete", context);
    const name = branchName(branch);
    githubDelete(repositoryPath(owner, repo) + "/git/refs/heads/" + encodeComponent(name));
}

/** Commit multiple file writes and deletions with optimistic branch concurrency.
 * @capability github.com/commits.create { owner: string, repo: string, branch: string }
 */
export function commitFiles(repository: RepositoryRef, input: CommitFilesInput): Commit {
    const { owner, name } = repository;
    const { branch: requestedBranch, expectedHeadSha, message, changes } = input;
    const context = repositoryContext(owner, name, "branch", requestedBranch);
    check("github.com/commits.create", context);
    const branch = branchName(requestedBranch);
    requireSha(expectedHeadSha, "expected head SHA");
    requireText(message, "commit message");
    validateChanges(changes);
    const current = githubGet(repositoryPath(owner, name) + "/git/ref/heads/" + encodeComponent(branch)).json() as ApiGitRef;
    const currentSha = current.object === null ? "" : str(current.object.sha);
    if (currentSha !== expectedHeadSha) {
        throw validationError("branch_moved", "Branch head no longer matches expectedHeadSha");
    }
    const parent = githubGet(repositoryPath(owner, name) + "/git/commits/" + encodeComponent(expectedHeadSha)).json() as ApiGitCommit;
    const parentTree = parent.tree === null ? "" : str(parent.tree.sha);
    if (parentTree.length === 0) throw validationError("missing_tree", "Expected commit does not include a tree SHA");
    const treeItems: GitTreeItemBody[] = [];
    for (const change of changes) {
        const path = repositoryFilePath(change.path);
        if (change.type === "delete") {
            treeItems.push({ path: path, mode: "100644", type: "blob", sha: null });
        } else {
            const content = change.content;
            if (content === null) throw validationError("missing_content", "Write change for " + path + " requires content");
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
    const update = githubPatchRaw(repositoryPath(owner, name) + "/git/refs/heads/" + encodeComponent(branch), updateBody);
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
 * @capability github.com/issues.get { owner: string, repo: string, number: number }
 */
export function getIssue(repository: RepositoryRef, number: number): Issue | null {
    const { owner, name } = repository;
    const context = repositoryNumberContext(owner, name, number);
    check("github.com/issues.get", context);
    const response = githubGetNullable(repositoryPath(owner, name) + "/issues/" + issueNumber(number));
    if (response === null) return null;
    const data = response.json() as ApiIssue;
    if (data.pull_request !== null) {
        throw validationError("wrong_resource_type", "GitHub number identifies a pull request; use getPullRequest");
    }
    return issueFrom(data);
}

/** List true issues using cursor pagination.
 * @capability github.com/issues.list { owner: string, repo: string }
 */
export function listIssues(repository: RepositoryRef, options: ListIssuesOptions | null = null): PageResult<Issue> {
    const { owner, name } = repository;
    const state = options === null ? null : options.state;
    const requestedLabels = options === null ? null : options.labels;
    const orderBy = options === null ? null : options.orderBy;
    const direction = options === null ? null : options.direction;
    const limit = options === null ? null : options.limit;
    const cursor = options === null ? null : options.pageToken;
    let labels: string[] | null = null;
    if (requestedLabels !== null) {
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
        after: pageToken(cursor, false),
        states: state === null ? null : [state],
        labels: labels,
        orderField: orderBy === null ? "UPDATED_AT" : orderBy,
        direction: direction === null ? "DESC" : direction,
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
    const envelope = githubPostGraphQl(body).json() as GraphQlIssueEnvelope;
    if (envelope.errors !== null && envelope.errors.length > 0) {
        const messages: string[] = [];
        for (const item of envelope.errors) messages.push(str(item.message));
        throw new GitHubError("graphql_error", messages.join("; "), 200, "", "", null, null, null, messages);
    }
    const data = required(envelope.data, "GraphQL data");
    const repositoryData = required(data.repository, "GraphQL repository");
    const connection = required(repositoryData.issues, "GraphQL issue connection");
    const items: Issue[] = [];
    for (const item of array(connection.nodes)) items.push(graphQlIssueFrom(item));
    const info = required(connection.pageInfo, "GraphQL page information");
    const next = info.hasNextPage === true ? str(info.endCursor) : "";
    return { items: items, nextPageToken: next, isComplete: next.length === 0 };
}

/** Search issues within one repository.
 * @capability github.com/issues.search { owner: string, repo: string }
 */
export function searchIssues(repository: RepositoryRef, query: string, options: SearchOptions | null = null): SearchPageResult<Issue> {
    const { owner, name } = repository;
    const sort = options === null ? null : options.sort;
    const order = options === null ? null : options.order;
    const limit = options === null ? null : options.limit;
    const cursor = options === null ? null : options.pageToken;
    const context = checkedRepository(owner, name);
    check("github.com/issues.search", context);
    return searchIssuesOrPulls(owner, name, query, { sort: sort, order: order, limit: limit, cursor: cursor }, "is:issue", false);
}

/** Create an issue.
 * @capability github.com/issues.create { owner: string, repo: string }
 */
export function createIssue(repository: RepositoryRef, input: CreateIssueInput): Issue {
    const { owner, name } = repository;
    const {
        title,
        body: description,
        assignees: requestedAssignees,
        labels: requestedLabels,
        milestone,
    } = input;
    let assignees: string[] | null = null;
    if (requestedAssignees !== null) {
        const copied: string[] = [];
        for (const assignee of requestedAssignees) copied.push(assignee);
        assignees = copied;
    }
    let labels: string[] | null = null;
    if (requestedLabels !== null) {
        const copied: string[] = [];
        for (const label of requestedLabels) copied.push(label);
        labels = copied;
    }
    const context = checkedRepository(owner, name);
    check("github.com/issues.create", context);
    requireText(title, "issue title");
    const fields: string[] = [jsonProperty("title", JSON.stringify(title))];
    if (description !== null) fields.push(jsonProperty("body", JSON.stringify(description)));
    if (assignees !== null) fields.push(jsonProperty("assignees", stringArrayJson(assignees)));
    if (labels !== null) fields.push(jsonProperty("labels", stringArrayJson(labels)));
    if (milestone !== null) fields.push(jsonProperty("milestone", milestone.toString()));
    const body = jsonObject(fields);
    return issueFrom(githubPost(repositoryPath(owner, name) + "/issues", body).json() as ApiIssue);
}

/** Update an issue.
 * @capability github.com/issues.update { owner: string, repo: string, number: number }
 */
export function updateIssue(repository: RepositoryRef, number: number, input: UpdateIssueInput): Issue {
    const { owner, name } = repository;
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
    let assignees: string[] | null = null;
    if (requestedAssignees !== null) {
        const copied: string[] = [];
        for (const assignee of requestedAssignees) copied.push(assignee);
        assignees = copied;
    }
    let labels: string[] | null = null;
    if (requestedLabels !== null) {
        const copied: string[] = [];
        for (const label of requestedLabels) copied.push(label);
        labels = copied;
    }
    const context = repositoryNumberContext(owner, name, number);
    check("github.com/issues.update", context);
    const fields: string[] = [];
    if (title !== null) fields.push(jsonProperty("title", JSON.stringify(title)));
    if (description !== null) fields.push(jsonProperty("body", JSON.stringify(description)));
    if (clearBody === true) fields.push(jsonProperty("body", "null"));
    if (assignees !== null) fields.push(jsonProperty("assignees", stringArrayJson(assignees)));
    if (labels !== null) fields.push(jsonProperty("labels", stringArrayJson(labels)));
    if (milestone !== null) fields.push(jsonProperty("milestone", milestone.toString()));
    if (clearMilestone === true) fields.push(jsonProperty("milestone", "null"));
    if (state !== null) fields.push(jsonProperty("state", JSON.stringify(state)));
    if (stateReason !== null) fields.push(jsonProperty("state_reason", JSON.stringify(stateReason)));
    requireFields(fields);
    const body = jsonObject(fields);
    return issueFrom(githubPatch(repositoryPath(owner, name) + "/issues/" + issueNumber(number), body).json() as ApiIssue);
}

/** List issue comments.
 * @capability github.com/issueComments.list { owner: string, repo: string, number: number }
 */
export function listIssueComments(repository: RepositoryRef, number: number, options: PageOptions | null = null): PageResult<Comment> {
    const { owner, name } = repository;
    const limit = options === null ? null : options.limit;
    const cursor = options === null ? null : options.pageToken;
    const context = repositoryNumberContext(owner, name, number);
    check("github.com/issueComments.list", context);
    return listCommentsUnchecked(owner, name, number, { limit: limit, cursor: cursor });
}

/** Add an issue comment.
 * @capability github.com/issueComments.create { owner: string, repo: string, number: number }
 */
export function addIssueComment(repository: RepositoryRef, number: number, body: string): Comment {
    const { owner, name } = repository;
    const context = repositoryNumberContext(owner, name, number);
    check("github.com/issueComments.create", context);
    return addCommentUnchecked(owner, name, number, body);
}

/** List labels in a repository.
 * @capability github.com/labels.list { owner: string, repo: string }
 */
export function listLabels(repository: RepositoryRef, options: PageOptions | null = null): PageResult<Label> {
    const { owner, name } = repository;
    const limit = options === null ? null : options.limit;
    const cursor = options === null ? null : options.pageToken;
    const context = checkedRepository(owner, name);
    check("github.com/labels.list", context);
    const response = githubGet(repositoryPath(owner, name) + "/labels", pageQuery({ limit: limit, cursor: cursor }));
    const data = response.json() as ApiLabel[];
    const items: Label[] = [];
    for (const item of data) items.push(labelFrom(item));
    return pageFrom(response, items);
}

/** Retrieve a pull request, or null when absent.
 * @capability github.com/pulls.get { owner: string, repo: string, number: number }
 */
export function getPullRequest(repository: RepositoryRef, number: number): PullRequest | null {
    const { owner, name } = repository;
    const context = repositoryNumberContext(owner, name, number);
    check("github.com/pulls.get", context);
    return getPullRequestUnchecked(owner, name, number);
}

/** List pull requests.
 * @capability github.com/pulls.list { owner: string, repo: string }
 */
export function listPullRequests(repository: RepositoryRef, options: ListPullRequestsOptions | null = null): PageResult<PullRequest> {
    const { owner, name } = repository;
    const state = options === null ? null : options.state;
    const head = options === null ? null : options.head;
    const base = options === null ? null : options.base;
    const sort = options === null ? null : options.sort;
    const direction = options === null ? null : options.direction;
    const limit = options === null ? null : options.limit;
    const cursor = options === null ? null : options.pageToken;
    const context = checkedRepository(owner, name);
    check("github.com/pulls.list", context);
    const query = pageQuery({ limit: limit, cursor: cursor });
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

/** Search pull requests within one repository.
 * @capability github.com/pulls.search { owner: string, repo: string }
 */
export function searchPullRequests(repository: RepositoryRef, query: string, options: SearchOptions | null = null): SearchPageResult<PullRequest> {
    const { owner, name } = repository;
    const sort = options === null ? null : options.sort;
    const order = options === null ? null : options.order;
    const limit = options === null ? null : options.limit;
    const cursor = options === null ? null : options.pageToken;
    const context = checkedRepository(owner, name);
    check("github.com/pulls.search", context);
    const issues = searchIssuesOrPulls(owner, name, query, { sort: sort, order: order, limit: limit, cursor: cursor }, "is:pr", true);
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
 * @capability github.com/pulls.create { owner: string, repo: string }
 */
export function createPullRequest(repository: RepositoryRef, input: CreatePullRequestInput): PullRequest {
    const { owner, name } = repository;
    const { title, head, base, body: description, draft, maintainerCanModify } = input;
    const context = checkedRepository(owner, name);
    check("github.com/pulls.create", context);
    requireText(title, "pull request title");
    requireText(head, "pull request head");
    requireText(base, "pull request base");
    const fields: string[] = [
        jsonProperty("title", JSON.stringify(title)),
        jsonProperty("head", JSON.stringify(head)),
        jsonProperty("base", JSON.stringify(base)),
    ];
    if (description !== null) fields.push(jsonProperty("body", JSON.stringify(description)));
    if (draft !== null) fields.push(jsonProperty("draft", booleanJson(draft)));
    if (maintainerCanModify !== null) {
        fields.push(jsonProperty("maintainer_can_modify", booleanJson(maintainerCanModify)));
    }
    const body = jsonObject(fields);
    return pullRequestFrom(githubPost(repositoryPath(owner, name) + "/pulls", body).json() as ApiPullRequest);
}

/** Update a pull request.
 * @capability github.com/pulls.update { owner: string, repo: string, number: number }
 */
export function updatePullRequest(repository: RepositoryRef, number: number, input: UpdatePullRequestInput): PullRequest {
    const { owner, name } = repository;
    const { title, body: description, clearBody, base, state, maintainerCanModify } = input;
    const context = repositoryNumberContext(owner, name, number);
    check("github.com/pulls.update", context);
    const fields: string[] = [];
    if (title !== null) fields.push(jsonProperty("title", JSON.stringify(title)));
    if (description !== null) fields.push(jsonProperty("body", JSON.stringify(description)));
    if (clearBody === true) fields.push(jsonProperty("body", "null"));
    if (base !== null) fields.push(jsonProperty("base", JSON.stringify(base)));
    if (state !== null) fields.push(jsonProperty("state", JSON.stringify(state)));
    if (maintainerCanModify !== null) {
        fields.push(jsonProperty("maintainer_can_modify", booleanJson(maintainerCanModify)));
    }
    requireFields(fields);
    const body = jsonObject(fields);
    return pullRequestFrom(githubPatch(repositoryPath(owner, name) + "/pulls/" + issueNumber(number), body).json() as ApiPullRequest);
}

/** Merge a pull request without force.
 * @capability github.com/pulls.merge { owner: string, repo: string, number: number }
 */
export function mergePullRequest(repository: RepositoryRef, number: number, input: MergePullRequestInput | null = null): MergeResult {
    const { owner, name } = repository;
    const commitTitle = input === null ? null : input.commitTitle;
    const commitMessage = input === null ? null : input.commitMessage;
    const method = input === null ? null : input.method;
    const expectedHeadSha = input === null ? null : input.expectedHeadSha;
    const context = repositoryNumberContext(owner, name, number);
    check("github.com/pulls.merge", context);
    const fields: string[] = [];
    if (commitTitle !== null) fields.push(jsonProperty("commit_title", JSON.stringify(commitTitle)));
    if (commitMessage !== null) fields.push(jsonProperty("commit_message", JSON.stringify(commitMessage)));
    if (method !== null) fields.push(jsonProperty("merge_method", JSON.stringify(method)));
    if (expectedHeadSha !== null) fields.push(jsonProperty("sha", JSON.stringify(expectedHeadSha)));
    const body = jsonObject(fields);
    const data = githubPut(repositoryPath(owner, name) + "/pulls/" + issueNumber(number) + "/merge", body).json() as ApiMergeResult;
    return { merged: data.merged === true, message: str(data.message), sha: str(data.sha) };
}

/** Retrieve a pull request unified diff.
 * @capability github.com/pulls.diff { owner: string, repo: string, number: number }
 */
export function getPullRequestDiff(repository: RepositoryRef, number: number): string {
    const { owner, name } = repository;
    const context = repositoryNumberContext(owner, name, number);
    check("github.com/pulls.diff", context);
    const path = repositoryPath(owner, name) + "/pulls/" + issueNumber(number);
    return githubGet(path, null, "application/vnd.github.diff").body;
}

/** List files changed by a pull request.
 * @capability github.com/pullFiles.list { owner: string, repo: string, number: number }
 */
export function listPullRequestFiles(repository: RepositoryRef, number: number, options: PageOptions | null = null): PageResult<PullRequestFile> {
    const { owner, name } = repository;
    const limit = options === null ? null : options.limit;
    const cursor = options === null ? null : options.pageToken;
    const context = repositoryNumberContext(owner, name, number);
    check("github.com/pullFiles.list", context);
    const response = githubGet(repositoryPath(owner, name) + "/pulls/" + issueNumber(number) + "/files", pageQuery({ limit: limit, cursor: cursor }));
    const data = response.json() as ApiPullFile[];
    const items: PullRequestFile[] = [];
    for (const item of data) items.push(pullFileFrom(item));
    return pageFrom(response, items);
}

/** List pull request reviews.
 * @capability github.com/pullReviews.list { owner: string, repo: string, number: number }
 */
export function listPullRequestReviews(repository: RepositoryRef, number: number, options: PageOptions | null = null): PageResult<PullRequestReview> {
    const { owner, name } = repository;
    const limit = options === null ? null : options.limit;
    const cursor = options === null ? null : options.pageToken;
    const context = repositoryNumberContext(owner, name, number);
    check("github.com/pullReviews.list", context);
    const response = githubGet(repositoryPath(owner, name) + "/pulls/" + issueNumber(number) + "/reviews", pageQuery({ limit: limit, cursor: cursor }));
    const data = response.json() as ApiReview[];
    const items: PullRequestReview[] = [];
    for (const item of data) items.push(reviewFrom(item));
    return pageFrom(response, items);
}

/** Create a pull request review.
 * @capability github.com/pullReviews.create { owner: string, repo: string, number: number }
 */
export function createPullRequestReview(repository: RepositoryRef, number: number, input: CreateReviewInput): PullRequestReview {
    const { owner, name } = repository;
    const { body: summary, event, commitId, comments } = input;
    const context = repositoryNumberContext(owner, name, number);
    check("github.com/pullReviews.create", context);
    if (event === "REQUEST_CHANGES") requireText(summary === null ? "" : summary, "review body");
    const serializedComments: string[] = [];
    if (comments !== null) {
        for (const item of comments) {
            validateReviewComment(item);
            const commentFields: string[] = [
                jsonProperty("path", JSON.stringify(item.path)),
                jsonProperty("body", JSON.stringify(item.body)),
                jsonProperty("line", item.line.toString()),
                jsonProperty("side", JSON.stringify(item.side)),
            ];
            if (item.startLine !== null) commentFields.push(jsonProperty("start_line", item.startLine.toString()));
            if (item.startSide !== null) commentFields.push(jsonProperty("start_side", JSON.stringify(item.startSide)));
            serializedComments.push(jsonObject(commentFields));
        }
    }
    const fields: string[] = [jsonProperty("event", JSON.stringify(event))];
    if (summary !== null) fields.push(jsonProperty("body", JSON.stringify(summary)));
    if (commitId !== null) fields.push(jsonProperty("commit_id", JSON.stringify(commitId)));
    if (comments !== null) fields.push(jsonProperty("comments", "[" + serializedComments.join(",") + "]"));
    const body = jsonObject(fields);
    return reviewFrom(githubPost(repositoryPath(owner, name) + "/pulls/" + issueNumber(number) + "/reviews", body).json() as ApiReview);
}

/** List general pull request comments.
 * @capability github.com/pullComments.list { owner: string, repo: string, number: number }
 */
export function listPullRequestComments(repository: RepositoryRef, number: number, options: PageOptions | null = null): PageResult<Comment> {
    const { owner, name } = repository;
    const limit = options === null ? null : options.limit;
    const cursor = options === null ? null : options.pageToken;
    const context = repositoryNumberContext(owner, name, number);
    check("github.com/pullComments.list", context);
    return listCommentsUnchecked(owner, name, number, { limit: limit, cursor: cursor });
}

/** Add a general pull request comment.
 * @capability github.com/pullComments.create { owner: string, repo: string, number: number }
 */
export function addPullRequestComment(repository: RepositoryRef, number: number, body: string): Comment {
    const { owner, name } = repository;
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
    page: PageRequest,
): PageResult<Comment> {
    const response = githubGet(repositoryPath(owner, name) + "/issues/" + issueNumber(number) + "/comments", pageQuery(page));
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

function searchIssuesOrPulls(
    owner: string,
    name: string,
    query: string,
    search: SearchRequest,
    kind: string,
    allowPulls: boolean,
): SearchPageResult<Issue> {
    const scoped = scopedSearch(query, owner, name, kind);
    const response = githubGet("/search/issues", searchQuery(scoped, search));
    const data = response.json() as ApiSearchIssues;
    const items: Issue[] = [];
    for (const item of array(data.items)) {
        if (allowPulls || item.pull_request === null) items.push(issueFrom(item));
    }
    return searchPageFrom(response, items, data.total_count, data.incomplete_results);
}

function readFileUnchecked(
    owner: string,
    name: string,
    path: string,
    request: FileReadRequest,
): RepositoryFile | null {
    const cleanPath = repositoryFilePath(path);
    const query = new Map<string, string>();
    putQuery(query, "ref", request.ref);
    const response = githubGetNullable(repositoryPath(owner, name) + "/contents/" + encodedFilePath(cleanPath), query);
    if (response === null) return null;
    const data = response.json() as ApiContent;
    if (str(data.type) !== "file") throw validationError("not_a_file", cleanPath + " is not a repository file");
    const maxBytes = fileLimit(request.maxBytes);
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
    return { path: str(data.path), sha: str(data.sha), size: size, htmlUrl: nullableString(data.html_url), bytes: bytes };
}

function githubGet(
    path: string,
    query: Map<string, string> | null = null,
    accept: string = "application/vnd.github+json",
): Response {
    const suffix = query === null ? "" : querySuffix(query);
    return requireOk(get(GITHUB_API + path.slice(1) + suffix, authHeaders(accept)));
}

function githubGetNullable(path: string, query: Map<string, string> | null = null): Response | null {
    const suffix = query === null ? "" : querySuffix(query);
    const response = get(GITHUB_API + path.slice(1) + suffix, authHeaders());
    if (response.status === 404) return null;
    return requireOk(response);
}

function githubPost(path: string, body: string | Uint8Array | {} | unknown[] | null): Response {
    return requireOk(post(GITHUB_API + path.slice(1), body, authHeaders()));
}

function githubPostGraphQl(body: string | Uint8Array | {} | unknown[] | null): Response {
    return requireOk(post(GRAPHQL_API, body, authHeaders()));
}

function githubPut(path: string, body: string | Uint8Array | {} | unknown[] | null): Response {
    return requireOk(put(GITHUB_API + path.slice(1), body, authHeaders()));
}

function githubPatch(path: string, body: string | Uint8Array | {} | unknown[] | null): Response {
    return requireOk(githubPatchRaw(path, body));
}

function githubPatchRaw(path: string, body: string | Uint8Array | {} | unknown[] | null): Response {
    return patch(GITHUB_API + path.slice(1), body, authHeaders());
}

function githubDelete(path: string): Response {
    return requireOk(delete(GITHUB_API + path.slice(1), authHeaders()));
}

function authHeaders(accept: string = "application/vnd.github+json"): Map<string, string> {
    const token = secrets.get("GITHUB_TOKEN");
    if (token === null) throw validationError("missing_token", "GITHUB_TOKEN is not bound");
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
        if (overrideMessage.length === 0 && envelope.message !== null) message = envelope.message;
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
                    if (detail.field !== null) text += "." + detail.field;
                    if (detail.code !== null) text += ": " + detail.code;
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

function pageQuery(request: PageRequest): Map<string, string> {
    const query = new Map<string, string>();
    query.set("per_page", pageLimit(request.limit).toString());
    const page = pageToken(request.cursor, true);
    if (page !== null) query.set("page", page);
    return query;
}

function searchQuery(value: string, search: SearchRequest): Map<string, string> {
    const query = pageQuery({ limit: search.limit, cursor: search.cursor });
    query.set("q", value);
    putQuery(query, "sort", search.sort);
    putQuery(query, "order", search.order);
    return query;
}

function pageFrom<T>(response: Response, items: T[]): PageResult<T> {
    const next = nextPageToken(response);
    return { items: items, nextPageToken: next, isComplete: next.length === 0 };
}

function searchPageFrom<T>(
    response: Response,
    items: T[],
    totalCount: number | null,
    incomplete: boolean | null,
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
        return pageToken(cursor, true) === null ? "" : cursor;
    }
    return "";
}

function pageLimit(value: number | null): number {
    const limit = value === null ? DEFAULT_LIMIT : value;
    if (limit < 1 || limit > MAX_LIMIT) {
        throw validationError("invalid_page_size", "limit must be between 1 and " + MAX_LIMIT.toString());
    }
    return limit;
}

function pageToken(value: string | null, numeric: boolean): string | null {
    if (value === null || value.length === 0) return null;
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
    const lower = query.toLowerCase();
    let quoted = false;
    let word = "";
    const words: string[] = [];
    for (let i = 0; i < lower.length; i += 1) {
        const char = lower.charAt(i);
        if (char === "\"") {
            quoted = !quoted;
            continue;
        }
        if (quoted) continue;
        if (char === " " || char === "\t" || char === "\n" || char === "(" || char === ")") {
            if (word.length > 0) {
                words.push(word);
                word = "";
            }
        } else {
            word += char;
        }
    }
    if (word.length > 0) words.push(word);
    if (quoted) throw validationError("unsafe_search_query", "Search query contains an unterminated quote");
    for (const item of words) {
        if (
            item === "or"
            || item.startsWith("repo:")
            || item.startsWith("org:")
            || item.startsWith("user:")
            || item.startsWith("is:issue")
            || item.startsWith("is:pr")
        ) {
            throw validationError("unsafe_search_query", "Repository-scoped search cannot contain scope-changing qualifiers or OR");
        }
    }
    const suffix = kind.length === 0 ? "" : " " + kind;
    return query + " repo:" + owner + "/" + name + suffix;
}

function checkedRepository(owner: string, name: string): RepositoryCapabilityContext {
    requireText(owner, "repository owner");
    requireText(name, "repository name");
    return { owner: owner, repo: name };
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
    checkedRepository(owner, name);
    return { owner: owner, repo: name, number: number };
}

function repositoryPath(owner: string, name: string): string {
    checkedRepository(owner, name);
    return "/repos/" + encodeComponent(owner) + "/" + encodeComponent(name);
}

function segment(value: string, label: string): string {
    requireText(value, label);
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
            if (change.content === null) throw validationError("missing_content", "Write change for " + path + " requires content");
            if (typeof change.content === "string") {
                bytes += new TextEncoder().encode(change.content).length;
            } else {
                bytes += change.content.length;
            }
        } else if (change.type === "delete") {
            if (change.content !== null) {
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
    if (comment.startLine !== null) {
        if (comment.startLine < 1 || comment.startLine > comment.line || comment.startLine !== Math.floor(comment.startLine)) {
            throw validationError("invalid_line", "Review comment startLine must be a positive integer no greater than line");
        }
        if (comment.startSide === null) {
            throw validationError("missing_start_side", "Review comment startSide is required with startLine");
        }
    }
}

function fileLimit(value: number | null): number {
    const limit = value === null ? MAX_FILE_BYTES : value;
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

function putQuery(query: Map<string, string>, key: string, value: string | null): void {
    if (value !== null && value.length > 0) query.set(key, value);
}

function header(response: Response, name: string): string {
    const value = response.headers.get(name);
    return value === null ? "" : value;
}

function nullableHeader(response: Response, name: string): string | null {
    return response.headers.get(name);
}

function numberHeader(response: Response, name: string): number | null {
    const value = response.headers.get(name);
    if (value === null || value.length === 0) return null;
    const parsed = Number(value);
    return isNaN(parsed) ? null : parsed;
}

function str(value: string | null): string {
    return value === null ? "" : value;
}

function num(value: number | null): number {
    return value === null ? 0 : value;
}

function nullableString(value: string | null): string | null {
    return value;
}

function nullableNumber(value: number | null): number | null {
    return value;
}

function array<T>(value: T[] | null): T[] {
    return value === null ? [] : value;
}

function required<T>(value: T | null, label: string): T {
    if (value === null) throw validationError("invalid_response", "GitHub response is missing " + label);
    return value;
}

function userFrom(data: ApiUser | null): User {
    if (data === null) return { login: "", id: 0, htmlUrl: "", avatarUrl: "", type: "" };
    const value = data!;
    return {
        login: str(value.login),
        id: num(value.id),
        htmlUrl: str(value.html_url),
        avatarUrl: str(value.avatar_url),
        type: str(value.type),
    };
}

function teamFrom(data: ApiTeam): Team {
    return {
        id: num(data.id),
        name: str(data.name),
        slug: str(data.slug),
        description: nullableString(data.description),
        privacy: str(data.privacy),
        htmlUrl: str(data.html_url),
        organization: data.organization === null ? "" : str(data.organization.login),
    };
}

function repositoryFrom(data: ApiRepository): Repository {
    return {
        id: num(data.id),
        name: str(data.name),
        fullName: str(data.full_name),
        description: nullableString(data.description),
        htmlUrl: str(data.html_url),
        private: data.private === true,
        fork: data.fork === true,
        archived: data.archived === true,
        defaultBranch: str(data.default_branch),
        language: nullableString(data.language),
        topics: array(data.topics),
        stargazersCount: num(data.stargazers_count),
        forksCount: num(data.forks_count),
        openIssuesCount: num(data.open_issues_count),
        createdAt: str(data.created_at),
        updatedAt: str(data.updated_at),
        pushedAt: nullableString(data.pushed_at),
    };
}

function branchFrom(data: ApiBranch): Branch {
    return {
        name: str(data.name),
        sha: data.commit === null ? "" : str(data.commit.sha),
        protected: data.protected === true,
    };
}

function commitFrom(data: ApiCommit, detail: CommitDetail): Commit {
    const body = data.commit;
    const author = body === null ? null : body.author;
    const committer = body === null ? null : body.committer;
    let stats: CommitStats | null = null;
    if (detail !== "none" && data.stats !== null) {
        stats = {
            additions: num(data.stats.additions),
            deletions: num(data.stats.deletions),
            total: num(data.stats.total),
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
                previousFilename: nullableString(file.previous_filename),
                patch: detail === "patch" ? nullableString(file.patch) : null,
            });
        }
    }
    return {
        sha: str(data.sha),
        htmlUrl: str(data.html_url),
        message: body === null ? "" : str(body.message),
        author: data.author === null ? null : userFrom(data.author),
        committer: data.committer === null ? null : userFrom(data.committer),
        authorName: author === null ? "" : str(author.name),
        authorEmail: author === null ? "" : str(author.email),
        authoredAt: author === null ? "" : str(author.date),
        committerName: committer === null ? "" : str(committer.name),
        committerEmail: committer === null ? "" : str(committer.email),
        committedAt: committer === null ? "" : str(committer.date),
        stats: stats,
        files: files,
    };
}

function releaseFrom(data: ApiRelease): Release {
    return {
        id: num(data.id),
        tagName: str(data.tag_name),
        name: nullableString(data.name),
        body: nullableString(data.body),
        htmlUrl: str(data.html_url),
        draft: data.draft === true,
        prerelease: data.prerelease === true,
        createdAt: str(data.created_at),
        publishedAt: nullableString(data.published_at),
        author: userFrom(data.author),
    };
}

function labelFrom(data: ApiLabel): Label {
    return {
        id: num(data.id),
        name: str(data.name),
        color: str(data.color),
        description: nullableString(data.description),
        default: data.default === true,
    };
}

function milestoneFrom(data: ApiMilestone | null): Milestone | null {
    if (data === null) return null;
    const value = data!;
    return {
        id: num(value.id),
        number: num(value.number),
        title: str(value.title),
        description: nullableString(value.description),
        state: str(value.state),
        dueOn: nullableString(value.due_on),
    };
}

function commentFrom(data: ApiComment): Comment {
    return {
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
        id: num(data.id),
        number: num(data.number),
        title: str(data.title),
        body: nullableString(data.body),
        state: str(data.state).toLowerCase(),
        stateReason: nullableString(data.state_reason),
        locked: data.locked === true,
        htmlUrl: str(data.html_url),
        user: userFrom(data.user),
        labels: labels,
        assignees: assignees,
        milestone: milestoneFrom(data.milestone),
        comments: num(data.comments),
        createdAt: str(data.created_at),
        updatedAt: str(data.updated_at),
        closedAt: nullableString(data.closed_at),
    };
}

function graphQlIssueFrom(data: ApiGraphQlIssue): Issue {
    const labels: Label[] = [];
    if (data.labels !== null) {
        for (const label of array(data.labels.nodes)) labels.push(labelFrom(label));
    }
    const assignees: User[] = [];
    if (data.assignees !== null) {
        for (const user of array(data.assignees.nodes)) assignees.push(userFrom(user));
    }
    return {
        id: num(data.id),
        number: num(data.number),
        title: str(data.title),
        body: nullableString(data.body),
        state: str(data.state).toLowerCase(),
        stateReason: nullableString(data.state_reason),
        locked: data.locked === true,
        htmlUrl: str(data.html_url),
        user: userFrom(data.user),
        labels: labels,
        assignees: assignees,
        milestone: milestoneFrom(data.milestone),
        comments: data.comments === null ? 0 : num(data.comments.totalCount),
        createdAt: str(data.created_at),
        updatedAt: str(data.updated_at),
        closedAt: nullableString(data.closed_at),
    };
}

function pullRefFrom(data: ApiPullRef | null): PullRequestRef {
    if (data === null) {
        return {
            label: "",
            ref: "",
            sha: "",
            user: userFrom(null),
            repository: null,
        };
    }
    const value = data!;
    return {
        label: str(value.label),
        ref: str(value.ref),
        sha: str(value.sha),
        user: userFrom(value.user),
        repository: value.repo === null ? null : repositoryFrom(value.repo),
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
        id: num(data.id),
        number: num(data.number),
        title: str(data.title),
        body: nullableString(data.body),
        state: str(data.state),
        draft: data.draft === true,
        merged: data.merged === true,
        mergeable: data.mergeable,
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
        closedAt: nullableString(data.closed_at),
        mergedAt: nullableString(data.merged_at),
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
        previousFilename: nullableString(data.previous_filename),
        patch: nullableString(data.patch),
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
        submittedAt: nullableString(data.submitted_at),
        authorAssociation: str(data.author_association),
    };
}

function rateResourceFrom(data: ApiRateResource | null): RateLimitResource {
    if (data === null) return { limit: 0, remaining: 0, used: 0, resetAt: "" };
    const value = data!;
    return {
        limit: num(value.limit),
        remaining: num(value.remaining),
        used: num(value.used),
        resetAt: value.reset === null ? "" : value.reset.toString(),
    };
}

function getCommitUnchecked(owner: string, name: string, ref: string, detail: CommitDetail): Commit | null {
    const response = githubGetNullable(repositoryPath(owner, name) + "/commits/" + encodeComponent(ref));
    return response === null ? null : commitFrom(response.json() as ApiCommit, detail);
}
