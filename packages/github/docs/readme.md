# GitHub package guide

Use `@submilli/github` for typed GitHub.com work inside Submilli. Credentials are read
internally from `GITHUB_TOKEN`; never pass tokens as function arguments.

- Pass repositories as `{ owner, name }`.
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
