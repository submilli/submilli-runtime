# Notion

Use `@submilli/notion` to find and work with Notion content through a native,
synchronous API. Credentials are supplied internally; never request, accept, or
pass an access token in package calls.

Start agent workflows with `search`, then use the typed `fetch` resource
discriminator or `fetchPage`, `fetchDatabase`, and `fetchDataSource`. References
may be Notion IDs or Notion URLs. Data sources additionally accept
`collection://<id>`.

For page content, prefer the enhanced Markdown functions:
`readPageMarkdown`, `updatePageMarkdown`, `replacePageMarkdown`, and
`appendPageMarkdown`. `createPage` accepts one content strategy: Markdown, block
children, a data-source template, or no content. Page properties have typed
inputs for common property kinds and a `custom: Map<string, unknown>` escape
hatch for newer or uncommon Notion property shapes.

Use `queryDataSource` for structured filters and sorts, and
`listDataSourceTemplates` before creating a page from a template. Database views
can be created, updated, listed, and queried. Continue a cached query with
`continueViewQuery` and its opaque cursor.

Comments support inline Markdown and up to three uploaded or external
attachments. Block functions retrieve and edit block trees. `queryMeetingNotes`
returns meeting-note blocks visible to the integration user.

Block and block-comment semantic capabilities are scoped to their resolved
`pageId`, not the nested block ID. Targets without a page context fail closed.
When replying to an existing discussion, provide
`target.discussionParentRef` with its page or block reference so the package
can establish the containing page before authorization.

`appendBlockChildren` accepts `childrenJson`, an array containing one serialized
Notion block object per child, plus optional `positionJson`. Prefer enhanced
Markdown functions when raw block structure is not required.

`uploadFile` reads a VFS path and automatically uses a single request through
20 MiB or sequential multipart requests above that threshold. It returns a file
upload ID suitable for `FileReference { type: "file_upload", uploadId: id }`.

Bulk page creation and moves are sequential. They validate every reference
before the first mutation, stop on the first failure, and throw
`BatchNotionError` with `completedIds` and `failedIndex`. `createPages` first
checks `submilli/notion.createPages`, then checks each page as `createPage` when
its turn comes, so a per-page denial can follow pages already created. That
denial arrives as `PermissionDeniedError`, without the IDs of those pages.
`movePages` checks every move, with the fields `movePage` checks, before it moves
the first page.

Failures throw `NotionError` with `code`, HTTP `status`, `requestId`, and
`retryAfter`. The package does not sleep or retry automatically; use
`retryAfter` to make an explicit retry decision.

## Example

Find pages by title and report where they live.

```ts
import notion from "@submilli/notion";

function main(): string {
    const results = notion.search({ query: "roadmap", kind: "page", pageSize: 5 });
    if (results.results.length === 0) return "No matching pages.";
    const lines: string[] = [];
    for (const result of results.results) {
        lines.push(result.id + "  " + result.url);
    }
    return lines.join("\n");
}
```
