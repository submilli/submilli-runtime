# GitHub package guide

Use `@submilli/github` for typed GitHub.com work inside Submilli. Credentials are supplied
internally; never pass tokens as function arguments.

- Pass repositories as `{ owner, name }`: one owner login and one repository name. A
  value with a space, a slash, or other syntax throws `GitHubError` with code
  `invalid_input`.

<!-- submilli:if any="github.com/contents.readFile github.com/contents.readTextFile github.com/contents.listDirectory" -->

- A policy may limit file reads by `ref`. Name the `ref` when reading a file under such
  a policy; an unnamed ref is the default branch and is checked as null.

<!-- submilli:endif -->

<!-- submilli:if any="github.com/pulls.create github.com/pulls.update github.com/pulls.list" -->

- A policy may limit pull requests by `head` and `base`.

<!-- submilli:endif -->

<!-- submilli:if any="github.com/contents.readFile github.com/contents.readTextFile github.com/contents.listDirectory github.com/pulls.create github.com/pulls.update github.com/pulls.list" -->

- Any other field the operation does not name is checked as null. These fields are
  compared as written, and one branch has several spellings (`main`, `heads/main`, a
  commit SHA).

<!-- submilli:endif -->

<!-- submilli:if capability="github.com/code.search" -->
- Prefer `searchCode` for repository-scoped code discovery.
<!-- submilli:endif -->
<!-- submilli:if capability="github.com/issues.search" -->
- Prefer `searchIssues` for repository-scoped issue discovery.
<!-- submilli:endif -->
<!-- submilli:if capability="github.com/pulls.search" -->
- Prefer `searchPullRequests` for repository-scoped pull request discovery.
<!-- submilli:endif -->
<!-- submilli:if capability="github.com/repositories.search" -->
- A policy may limit `searchRepositories` to exact queries. It compares the query exactly as
  written, so a change in spacing or case is a different query.
<!-- submilli:endif -->
<!-- submilli:if any="github.com/code.search github.com/issues.search github.com/pulls.search" -->
- The repository-scoped search functions reject query syntax that could escape the
  repository policy.
<!-- submilli:endif -->

- Continue list calls with the opaque `nextPageToken`; do not interpret it.

<!-- submilli:if capability="github.com/contents.readFile" -->
- `readFile` returns bytes.
<!-- submilli:endif -->
<!-- submilli:if capability="github.com/contents.readTextFile" -->
- `readTextFile` requires valid UTF-8.
<!-- submilli:endif -->
<!-- submilli:if any="github.com/contents.readFile github.com/contents.readTextFile" -->
- File reads are capped at 10 MiB.
<!-- submilli:endif -->

<!-- submilli:if capability="github.com/contents.listDirectory" -->

- `listDirectory` follows GitHub's 1,000-entry directory limit.

<!-- submilli:endif -->

<!-- submilli:if all="github.com/contents.listDirectory github.com/trees.get" -->

- Use `getTree` for larger directory walks.

<!-- submilli:endif -->

<!-- submilli:if capability="github.com/commits.create" -->

- Use `commitFiles` for atomic multi-file changes. Read the branch SHA first and pass it
  as `expectedHeadSha`; a concurrent branch update throws `GitHubError` with code
  `branch_moved`. Branch refs are never force-updated.

<!-- submilli:endif -->

- Missing read resources return `null`. Invalid input, authorization failures, mutations,
  and rate limits throw `GitHubError`.
- The package does not retry automatically. Inspect `retryAfterSeconds`,
  `rateLimitRemaining`, and `rateLimitReset` before deciding whether to retry.
  Reset values are GitHub Unix-second timestamps.
- Results are curated stable models. There is intentionally no raw REST or GraphQL escape
  hatch.

<!-- submilli:if capability="github.com/rateLimit.get" -->

GitHub REST, search, and GraphQL have different rate-limit buckets. Call `getRateLimit`
when an agent needs an explicit budget snapshot.

<!-- submilli:endif -->

<!-- submilli:if capability="github.com/commits.list" -->

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

<!-- submilli:endif -->

<!-- submilli:if capability="github.com/issues.searchAcrossRepositories" -->

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

<!-- submilli:endif -->

<!-- submilli:if all="github.com/pulls.searchAcrossRepositories github.com/pulls.get github.com/pulls.getReactions" -->

## A person's pull requests in a date window

Summary search costs one request per page. Enrich only selected hits; full detail
and issue-style reaction reads need the corresponding repository capabilities.

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

<!-- submilli:endif -->

<!-- submilli:if capability="github.com/issues.searchAcrossRepositories" -->

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

<!-- submilli:endif -->

<!-- submilli:if any="github.com/events.list github.com/stargazers.list github.com/forks.list" -->

## Who starred or forked a repository

These list calls take the owner and the repository name as two strings. A missing
repository throws `GitHubError` with code `not_found`, and any other refusal throws too;
they do not return `null`.

<!-- submilli:endif -->

<!-- submilli:if capability="github.com/events.list" -->

- `listRepositoryEvents` returns recent public activity of every type, newest first.
  Filter by `type`: `WatchEvent` is a star (`action` is `"started"`), and `ForkEvent` is a
  fork, with the new repository in `forkFullName`. `action` and `forkFullName` are `""`
  where they do not apply. GitHub keeps only recent events, so this is not a full history.

<!-- submilli:endif -->

<!-- submilli:if capability="github.com/stargazers.list" -->

- `listStargazers` returns each account and its `starredAt` time. GitHub restricts this
  list to a repository's admins and collaborators, and refuses other tokens with a
  `GitHubError`.

<!-- submilli:endif -->

<!-- submilli:if all="github.com/stargazers.list github.com/events.list" -->

- When `listStargazers` is refused, read stars from `listRepositoryEvents` instead.

<!-- submilli:endif -->

<!-- submilli:if capability="github.com/forks.list" -->

- `listForks` returns forks, newest first, with each `ownerLogin` and `createdAt`.

<!-- submilli:endif -->

<!-- submilli:if any="github.com/users.get github.com/userOrganizations.list github.com/userSocialAccounts.list" -->

## Read a person's public profile

Pass one login, optionally ending in `[bot]`. A value that is not a GitHub login throws
`GitHubError` with code `invalid_input`. An unknown login throws with code `not_found`.
Logins are lowercase in policy and requests.

<!-- submilli:endif -->

<!-- submilli:if capability="github.com/users.get" -->

- `getUser` returns the public profile. Unset text fields are `""`. `hasPublicEmail`
  reports whether the profile shows an email address; the address itself is not available.

<!-- submilli:endif -->

<!-- submilli:if capability="github.com/userOrganizations.list" -->

- `listUserOrganizations` returns public memberships only.

<!-- submilli:endif -->

<!-- submilli:if capability="github.com/userSocialAccounts.list" -->

- `listUserSocialAccounts` returns the accounts linked from the profile, each with its
  `provider` and `url`.

<!-- submilli:endif -->

<!-- submilli:if all="github.com/events.list github.com/users.get" -->

## Profile recent stargazers and forkers

```ts
import github from "@submilli/github";

function main(): string {
    const page = github.listRepositoryEvents("octocat", "Hello-World", { limit: 50 });
    const lines: string[] = [];
    for (const event of page.items) {
        if (event.type !== "WatchEvent" && event.type !== "ForkEvent") continue;
        const profile = github.getUser(event.actorLogin);
        lines.push(event.type + " " + profile.login + " " + profile.company);
    }
    return lines.join("\n");
}
```

<!-- submilli:endif -->

Repository owners and names are lowercase in policy and requests.

<!-- submilli:if capability="github.com/commits.list" -->

Commit listing also exposes `sha` as `ref` and its path as `path`; these values
retain their case.

<!-- submilli:endif -->
