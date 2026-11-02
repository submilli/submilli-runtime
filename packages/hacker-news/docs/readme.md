# Hacker News package guide

Use `@submilli/hacker-news` to find Hacker News stories and comments and to
read whole discussions. It is read-only and needs no credentials.

- Failures throw `HackerNewsError` with a `code`: `invalid_input` for an
  argument refused before any request, `http_<status>`, or `invalid_response`.
- A `PermissionDeniedError` means the blueprint forbids that call, not that it
  was mis-called.
- `text` is HN's HTML. `author` is an HN username, which is not evidence of
  who the person is on any other site.

<!-- submilli:if capability="news.ycombinator.com/search" -->

## Search

`search({ query, sort, exact, type, storyId, author, createdAfter, createdBefore, page, hitsPerPage })`
returns one page of matching stories and comments, newest first unless
`sort: "relevance"`. Words match exactly unless `exact: false`. Times are
ISO-8601 instants. Request `page + 1` while it is below `pageCount`. When
`exhaustive` is false the total is an estimate, so an empty page then is not
proof that nothing matches.

<!-- submilli:endif -->

<!-- submilli:if capability="news.ycombinator.com/items.get" -->

## Read a thread

`getThread(itemId)` returns the item and every reply under it as one flat
list, the root first and each parent before its children, or `null` when the
item does not exist. Replies that do not repeat the search words are still
there, so read the whole thread of a story you care about.

<!-- submilli:endif -->

<!-- submilli:if all="news.ycombinator.com/search news.ycombinator.com/items.get" -->

## Example

New comments in every story that mentions Submilli since a time.

```ts
import { search, getThread } from "@submilli/hacker-news";

function main(): string {
    const since = "2026-10-20T21:00:00Z";
    const stories = search({ query: "submilli", type: "story", hitsPerPage: 50 });
    const lines: string[] = [];
    for (const story of stories.items) {
        const thread = getThread(story.id);
        if (thread === null) continue;
        for (const item of thread.items) {
            if (item.type === "comment" && item.createdAt >= since) {
                lines.push((item.author ?? "[deleted]") + " " + item.createdAt + " " + item.link);
            }
        }
    }
    return lines.length === 0 ? "No new comments." : lines.join("\n");
}
```

<!-- submilli:endif -->
