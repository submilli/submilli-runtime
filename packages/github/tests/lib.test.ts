import { label } from "submilli:test";
import secrets from "submilli:secrets";
import {
    CommitFileChange,
    GitHubError,
    RepositoryRef,
    UpdateIssueInput,
    createBranch,
    createIssue,
    getBranch,
    getCommit,
    getTree,
    getViewer,
    listBranches,
    listCommits,
    listIssues,
    readFile,
    searchCode,
    updateIssue,
} from "@submilli/github";

function main(): void {
    const repository: RepositoryRef = { owner: "octocat", name: "Hello-World" };

    label("public inputs are compact and typed");
    const changes: CommitFileChange[] = [
        { type: "write", path: "notes/plan.md", content: "# Plan" },
        { type: "delete", path: "notes/old.md" },
    ];
    assert(changes.length === 2, "commit changes are represented");
    const update: UpdateIssueInput = { state: "closed", stateReason: "completed" };
    assert(update.state === "closed", "issue state is typed");

    label("validation errors happen before credentials or network access");
    assertError("invalid_input", () => getBranch({ owner: "", name: "repo" }, "main"));
    assertError("invalid_branch", () => createBranch(repository, { name: "refs/heads/main", fromSha: "a".repeat(40) }));
    assertError("invalid_path", () => readFile(repository, "../secret"));
    assertError("unsafe_search_query", () => searchCode(repository, "token OR repo:other/repo"));
    assertError("invalid_number", () => updateIssue(repository, 0, update));
    assertError("invalid_input", () => createIssue(repository, { title: " " }));
    assertError("invalid_timestamp", () => listCommits(repository, { since: "2026-08-11" }));
    assertError("invalid_timestamp", () => listCommits(repository, { until: "not a timestamp" }));

    label("an owner and a repository name are each exactly one name");
    for (const name of ["a b", "x repo:other/repo", "other/repo", ".", "..", "a\rb", "a\nb", "a\tb", "a%2Fb", "na\u00efve"]) {
        assertError("invalid_input", () => getBranch({ owner: "acme", name: name }, "main"));
        assertError("invalid_input", () => searchCode({ owner: "acme", name: name }, "needle"));
    }
    for (const owner of ["a b", "acme/other", ".", "..", "acme.inc", "acme\r", "repo:acme"]) {
        assertError("invalid_input", () => getBranch({ owner: owner, name: "repo" }, "main"));
        assertError("invalid_input", () => searchCode({ owner: owner, name: "repo" }, "needle"));
    }

    label("a path segment cannot be . or ..");
    for (const dots of [".", ".."]) {
        assertError("invalid_input", () => getBranch(repository, dots));
        assertError("invalid_input", () => getCommit(repository, dots));
        assertError("invalid_input", () => getTree(repository, dots));
    }

    label("a search query is split on every kind of whitespace");
    for (const separator of [" ", "\t", "\n", "\r", "\u000b", "\u000c", "\u0085", "\u00a0", "\u2003", "\u2028", "\u3000", "\ufeff"]) {
        assertError("unsafe_search_query", () => searchCode(repository, "needle" + separator + "repo:other/repo"));
        assertError("unsafe_search_query", () => searchCode(repository, "needle" + separator + "OR" + separator + "other"));
    }

    label("omitted options use the defaults and a bad page option is refused");
    assertError("invalid_page_size", () => listBranches(repository, { limit: 0 }));
    assertError("invalid_page_token", () => listBranches(repository, { pageToken: "next" }));
    assertError("invalid_page_size", () => listIssues(repository, { limit: 101 }));
    if (secrets.get("GITHUB_TOKEN") === undefined) {
        label("an unbound GITHUB_TOKEN is refused before any request");
        assertError("missing_token", () => getViewer());
        assertError("missing_token", () => listBranches(repository));
        assertError("missing_token", () => listBranches(repository, undefined));
        assertError("missing_token", () => getCommit(repository, "main"));
        assertError("missing_token", () => readFile(repository, "README.md"));
    }

    label("GitHubError preserves operational metadata");
    const error = new GitHubError("rate_limited", "slow down", 429, "request-1", "docs", 2, 0, "1234", ["quota"]);
    assert(error.code === "rate_limited", "code is preserved");
    assert(error.requestId === "request-1", "request ID is preserved");
    assert(error.retryAfterSeconds === 2, "retry delay is preserved");
    assert(error.rateLimitRemaining === 0, "rate remaining is preserved");
}

function assertError(code: string, action: () => unknown): void {
    let matched = false;
    try {
        action();
    } catch (cause) {
        if (cause instanceof GitHubError) matched = cause.code === code && cause.status === 0;
    }
    assert(matched, "expected GitHubError " + code);
}
