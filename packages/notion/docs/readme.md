# Notion

Use `@submilli/notion` to find and work with Notion content through a native,
synchronous API. Credentials are supplied internally; never request, accept, or
pass an access token in package calls.

<!-- submilli:if capability="submilli/notion.search" -->
- Start agent workflows with `search`.
<!-- submilli:endif -->
<!-- submilli:if capability="submilli/notion.fetch" -->
- Use the typed `fetch` resource discriminator to read any resource.
<!-- submilli:endif -->
<!-- submilli:if capability="submilli/notion.fetchPage" -->
- Use `fetchPage` to read a page.
<!-- submilli:endif -->
<!-- submilli:if capability="submilli/notion.fetchDatabase" -->
- Use `fetchDatabase` to read a database.
<!-- submilli:endif -->
<!-- submilli:if capability="submilli/notion.fetchDataSource" -->
- Use `fetchDataSource` to read a data source.
<!-- submilli:endif -->

References may be Notion IDs or Notion URLs. Data sources additionally accept
`collection://<id>`.

<!-- submilli:if any="submilli/notion.readPageMarkdown submilli/notion.updatePageMarkdown submilli/notion.replacePageMarkdown submilli/notion.appendPageMarkdown" -->

For page content, prefer the enhanced Markdown functions:

<!-- submilli:endif -->
<!-- submilli:if capability="submilli/notion.readPageMarkdown" -->
- `readPageMarkdown`
<!-- submilli:endif -->
<!-- submilli:if capability="submilli/notion.updatePageMarkdown" -->
- `updatePageMarkdown`
<!-- submilli:endif -->
<!-- submilli:if capability="submilli/notion.replacePageMarkdown" -->
- `replacePageMarkdown`
<!-- submilli:endif -->
<!-- submilli:if capability="submilli/notion.appendPageMarkdown" -->
- `appendPageMarkdown`
<!-- submilli:endif -->

<!-- submilli:if capability="submilli/notion.createPage" -->

`createPage` accepts one content strategy: Markdown, block
children, a data-source template, or no content.

<!-- submilli:endif -->

<!-- submilli:if any="submilli/notion.createPage submilli/notion.createPages submilli/notion.updatePage" -->

Page properties have typed
inputs for common property kinds and a `custom: Map<string, unknown>` escape
hatch for newer or uncommon Notion property shapes. Every `custom` entry needs a
JSON value; an `undefined` value throws `invalid_body`.

<!-- submilli:endif -->

<!-- submilli:if capability="submilli/notion.updatePage" -->

`updatePage` changes only
the fields you set; `icon: null` or `cover: null` removes the icon or cover, as
`clearIcon` and `clearCover` do.

<!-- submilli:endif -->

Options bags on list and query functions are optional; omit them for the first
page of default results.

<!-- submilli:if capability="submilli/notion.queryDataSource" -->

Use `queryDataSource` for structured filters and sorts.

<!-- submilli:endif -->

<!-- submilli:if capability="submilli/notion.listDataSourceTemplates" -->

Use `listDataSourceTemplates` before creating a page from a template.

<!-- submilli:endif -->

<!-- submilli:if capability="submilli/notion.continueViewQuery" -->

Continue a cached query with
`continueViewQuery` and its opaque cursor.

<!-- submilli:endif -->

<!-- submilli:if capability="submilli/notion.createComment" -->

Comments support inline Markdown and up to three uploaded or external
attachments.

<!-- submilli:endif -->

<!-- submilli:if capability="submilli/notion.queryMeetingNotes" -->

`queryMeetingNotes`
returns meeting-note blocks visible to the integration user.

<!-- submilli:endif -->

<!-- submilli:if any="submilli/notion.getBlock submilli/notion.listBlockChildren submilli/notion.appendBlockChildren submilli/notion.updateBlock submilli/notion.trashBlock submilli/notion.restoreBlock submilli/notion.getComments submilli/notion.createComment" -->

Block and block-comment semantic capabilities are scoped to their resolved
`pageId`, not the nested block ID. Targets without a page context fail closed.

<!-- submilli:endif -->

<!-- submilli:if capability="submilli/notion.createComment" -->

When replying to an existing discussion, provide
`target.discussionParentRef` with its page or block reference so the package
can establish the containing page before authorization.

<!-- submilli:endif -->

<!-- submilli:if capability="submilli/notion.appendBlockChildren" -->

`appendBlockChildren` accepts `childrenJson`, an array containing one serialized
Notion block object per child, plus optional `positionJson`. Prefer enhanced
Markdown functions when raw block structure is not required.

<!-- submilli:endif -->

<!-- submilli:if capability="submilli/notion.uploadFile" -->

`uploadFile` reads a VFS path and automatically uses a single request through
20 MiB or sequential multipart requests above that threshold. It returns a file
upload ID suitable for `FileReference { type: "file_upload", uploadId: id }`.

<!-- submilli:endif -->

<!-- submilli:if any="submilli/notion.createPages submilli/notion.movePages" -->

Bulk page operations are sequential. They validate every reference
before the first mutation, stop on the first failure, and throw
`BatchNotionError` with `completedIds` and `failedIndex`.

<!-- submilli:endif -->

<!-- submilli:if capability="submilli/notion.createPages" -->

`createPages` first
checks `submilli/notion.createPages`, then checks each page as `createPage` when
its turn comes, so a per-page denial can follow pages already created. That
denial arrives as `PermissionDeniedError`, without the IDs of those pages.

<!-- submilli:endif -->

<!-- submilli:if capability="submilli/notion.movePages" -->

`movePages` checks every move, with the fields `movePage` checks, before it moves
the first page.

<!-- submilli:endif -->

Failures throw `NotionError` with `code`, HTTP `status`, `requestId`, and
`retryAfter`. The package does not sleep or retry automatically; use
`retryAfter` to make an explicit retry decision.

<!-- submilli:if capability="submilli/notion.search" -->

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

<!-- submilli:endif -->
