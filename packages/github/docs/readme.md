# GitHub package guide

Use `@submilli/github` for typed GitHub.com work inside Submilli. Credentials are read
internally from `GITHUB_TOKEN`; never pass tokens as function arguments.

- Pass repositories as `{ owner, name }`: one owner login and one repository name. A
  value with a space, a slash, or other syntax throws `GitHubError` with code
  `invalid_input`.
- A policy may limit file reads by `ref` and pull requests by `head` and `base`. Name
  the `ref` when reading a file under such a policy; an unnamed ref is the default
  branch and is checked as null, as is any other field the operation does not name. These fields are compared as written, and one branch has
  several spellings (`main`, `heads/main`, a commit SHA), so such rules are written as
  allow rules that name the permitted values.
- Prefer `searchCode`, `searchIssues`, and `searchPullRequests` for repository-scoped
  discovery. They reject query syntax that could escape the repository policy.
- Continue list calls with the opaque `nextPageToken`; do not interpret it.
- `readFile` returns bytes and `readTextFile` requires valid UTF-8. File reads are capped
  at 10 MiB.
- `listDirectory` follows GitHub's 1,000-entry directory limit; use `getTree` for larger
  directory walks.
- Use `commitFiles` for atomic multi-file changes. Read the branch SHA first and pass it
  as `expectedHeadSha`; a concurrent branch update throws `GitHubError` with code
  `branch_moved`. Branch refs are never force-updated.
- Missing read resources return `null`. Invalid input, authorization failures, mutations,
  and rate limits throw `GitHubError`.
- The package does not retry automatically. Inspect `retryAfterSeconds`,
  `rateLimitRemaining`, and `rateLimitReset` before deciding whether to retry.
  Reset values are GitHub Unix-second timestamps.
- Results are curated stable models. There is intentionally no raw REST or GraphQL escape
  hatch.

GitHub REST, search, and GraphQL have different rate-limit buckets. Call `getRateLimit`
when an agent needs an explicit budget snapshot.

## Example

Recent commits touching a path. `since`/`until` accept `Temporal` strings —
including the bracketed `toString()` form — and are normalized to UTC.

```ts
import github from "@submilli/github";

function main(): string {
    const repo = { owner: "rust-lang", name: "rust" };
    const weekAgo = Temporal.Now.instant().subtract({ hours: 24 * 7 });
    const commits = github.listCommits(repo, {
        path: "library/std",
        since: weekAgo.toString(),
        limit: 5,
    });
    if (commits.items.length === 0) return "No commits this week.";
    const lines: string[] = [];
    for (const commit of commits.items) {
        lines.push(commit.sha.slice(0, 7) + "  " + commit.message.split("\n")[0]);
    }
    return lines.join("\n");
}
```

## Search issues across repositories

Broad discovery requires `github.com/issues.searchAcrossRepositories`. Dates below
are inclusive UTC calendar days; check both partial-result flags before treating
the page as exhaustive.

```ts
import github from "@submilli/github";

function main(): string {
    const page = github.searchIssuesAcrossRepositories("wasmgc is:public", {
        createdSince: "2026-01-01",
        createdUntil: "2026-03-31",
        sort: "updated", limit: 20,
    });
    const lines: string[] = [];
    for (const hit of page.items) {
        lines.push(hit.repository.owner + "/" + hit.repository.name + " " + hit.issue.htmlUrl);
    }
    if (page.isCapped || page.incompleteResults) lines.push("Partial results: narrow the query.");
    return lines.join("\n");
}
```

## A person's pull requests in a date window

Summary search costs one request per page. Enrich only selected hits; full detail
and issue-style reaction reads need the corresponding repository grants.

```ts
import github from "@submilli/github";

function main(): string {
    const page = github.searchPullRequestsAcrossRepositories("is:public", {
        author: "dtolnay", createdSince: "2026-01-01", createdUntil: "2026-01-31",
        limit: 10,
    });
    if (page.items.length === 0) return "No matching pull requests.";
    const selected = page.items[0];
    const full = github.getPullRequest(selected.repository, selected.pullRequest.number);
    if (full === null) return "The selected pull request is no longer available.";
    const reactions = github.getPullRequestReactions(selected.repository, selected.pullRequest.number);
    let engagement = "reactions unavailable";
    if (reactions !== null && reactions.totalCount !== null) {
        engagement = reactions.totalCount.toString() + " reactions";
    }
    return full.htmlUrl + " " + full.additions.toString() + " additions; " + engagement;
}
```

## Rank a topic page by engagement

Ranking applies to this page only; unknown reactions are displayed separately
from confirmed zero. Continue with `nextPageToken` to collect additional pages,
and partition queries when `isCapped` is true.

```ts
import github from "@submilli/github";

function main(): string {
    const page = github.searchIssuesAcrossRepositories("wasmgc is:public", { limit: 30 });
    const ranked = page.items.sort((a, b) => {
        const aReactions = a.issue.reactions?.totalCount ?? 0;
        const bReactions = b.issue.reactions?.totalCount ?? 0;
        return (b.issue.comments + bReactions) - (a.issue.comments + aReactions);
    });
    const lines: string[] = [];
    for (const hit of ranked) {
        const count = hit.issue.reactions?.totalCount ?? null;
        const reactions = count === null ? "unknown" : count.toString();
        lines.push(hit.issue.htmlUrl + " comments=" + hit.issue.comments.toString() + " reactions=" + reactions);
    }
    return lines.join("\n");
}
```

Repository owners and names are lowercase in policy and requests. Update any
mixed-case owner/repo literals in blueprints. Commit listing also exposes `sha`
as `ref` and its path as `path`; these values retain their case. Ref rules should
allow specific values because branches, tags and SHAs can name the same history.
