import { searchIssuesAcrossRepositories, searchPullRequestsAcrossRepositories, searchPullRequestSummaries, GitHubError } from "@submilli/github";

function denied(action: () => void, capability: string, caller: string): void {
    let result = "no error";
    try { action(); }
    catch (error: PermissionDeniedError) { result = error.caller + ":" + error.capability; }
    assert(result === caller + ":" + capability, result);
}

function main(): string {
    // Grants for repository-scoped search do not imply discovery across repositories.
    denied(() => { searchIssuesAcrossRepositories("compiler org:allowed-org"); }, "github.com/issues.searchAcrossRepositories", "main");
    denied(() => { searchPullRequestsAcrossRepositories("compiler", { author: "Person", createdSince: "2026-01-01" }); }, "github.com/pulls.searchAcrossRepositories", "main");
    denied(() => { searchPullRequestSummaries({ owner: "ALLOWED-ORG", name: "REPO" }, "compiler"); }, "secrets.get", "@submilli/github");
    denied(() => { searchPullRequestSummaries({ owner: "other", name: "repo" }, "compiler"); }, "github.com/pulls.searchSummaries", "main");
    for (const query of ["repo:other/repo", '""org:other', "owner:other", "is:issue", "compiler OR bug"]) {
        let result = "no error";
        try { searchPullRequestSummaries({ owner: "allowed-org", name: "repo" }, query); }
        catch (error: GitHubError) { result = error.code; }
        assert(result === "unsafe_search_query", query + ": " + result);
    }
    return "research capability boundaries passed";
}
