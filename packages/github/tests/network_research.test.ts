// Read-only probes of search metadata and explicit enrichment.
import { label } from "submilli:test";
import secrets from "submilli:secrets";
import { getPullRequestReactions, getPullRequest, searchIssuesAcrossRepositories, searchPullRequestsAcrossRepositories, searchPullRequestSummaries } from "@submilli/github";

function main(): void {
    if (secrets.get("GITHUB_TOKEN") === undefined) {
        label("skip: GITHUB_TOKEN is not bound");
        return;
    }
    label("live broad topic search, UTC date window, identity and reactions");
    const issues = searchIssuesAcrossRepositories("compiler repo:rust-lang/rust is:public", {
        createdSince: "2025-01-01", createdUntil: "2026-01-31", limit: 2,
    });
    assert(issues.totalCount > 2, "topic query has multiple pages");
    assert(issues.nextPageToken.length > 0, "search preserves pagination");
    for (const hit of issues.items) {
        assert(hit.repository.owner === "rust-lang" && hit.repository.name === "rust", "repository identity is parsed");
        assert(hit.issue.createdAt >= "2025-01-01" && hit.issue.createdAt < "2026-02-01", "creation window holds");
        assert(hit.issue.htmlUrl.startsWith("https://github.com/rust-lang/rust/issues/"), "issue URLs are retained");
        assert(hit.issue.reactions !== null, "REST search supplies reactions");
    }
    const second = searchIssuesAcrossRepositories("compiler repo:rust-lang/rust is:public", {
        createdSince: "2025-01-01", createdUntil: "2026-01-31", limit: 2, pageToken: issues.nextPageToken,
    });
    assert(second.totalCount > 0, "next page is available");

    label("live author PR search and selected detail/reaction enrichment");
    const pulls = searchPullRequestsAcrossRepositories("is:public", {
        author: "dtolnay", createdSince: "2025-01-01", createdUntil: "2026-01-31", limit: 2,
    });
    assert(pulls.items.length > 0, "author has pull requests in the window");
    const selected = pulls.items[0];
    assert(selected.pullRequest.user.login.toLowerCase() === "dtolnay", "structured author holds");
    assert(selected.pullRequest.createdAt >= "2025-01-01" && selected.pullRequest.createdAt < "2026-02-01", "PR creation window holds");
    const detail = getPullRequest(selected.repository, selected.pullRequest.number);
    assert(detail !== null, "selected PR can be enriched");
    const reactions = getPullRequestReactions(selected.repository, selected.pullRequest.number);
    assert(reactions !== null, "issue-style PR reactions can be enriched");
    const scoped = searchPullRequestSummaries({ owner: "RUST-LANG", name: "Rust" }, "is:public", { limit: 2 });
    for (const hit of scoped.items) assert(hit.repository.owner === "rust-lang" && hit.repository.name === "rust", "summary search remains repository restricted");
}
