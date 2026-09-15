import { label } from "submilli:test";
import {
    CommitFileChange,
    GitHubError,
    RepositoryRef,
    UpdateIssueInput,
    createBranch,
    createIssue,
    getBranch,
    listCommits,
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
