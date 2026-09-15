# @submilli/notion

A synchronous, typed Notion client for agent workflows.

The package covers title search and typed resource fetches, enhanced Markdown
page creation and editing, dynamic page properties, databases and data sources,
views, comments, users, meeting notes, blocks, trash/restore, and automatic
single- or multipart VFS uploads. It targets Notion API version `2026-03-11`.

## Blueprint setup

Add the installed package and bind a Notion installation access token:

```yaml
packages:
  - "@submilli/notion"

secrets:
  NOTION_ACCESS_TOKEN:
    harness:
      required: true
```

`submilli blueprint add-package @submilli/notion` can add the package and
scaffold its semantic permissions. The Notion connection must also have the
corresponding content, property, comment, and user capabilities enabled in the
Notion developer portal.

Create an internal or public Notion connection, share the relevant pages or
databases with it, and bind its installation access token as
`NOTION_ACCESS_TOKEN`. Package calls never accept credentials as arguments.

Block and block-comment capabilities use `pageId` rather than unstable nested
block IDs. The package resolves a nested block's parent chain before the
semantic check, so page content is always authorized against its containing
page. Targets without a page context fail closed and should use the matching
database, data-source, or agent API instead.

## Example

```ts
import {
    search,
    readPageMarkdown,
    createPage,
} from "@submilli/notion";

function main(): string {
    const matches = search({ query: "Roadmap", pageSize: 5 });
    if (matches.results.length > 0) {
        return readPageMarkdown(matches.results[0].id).markdown;
    }
    const page = createPage({
        parent: {
            type: "page_id",
            id: "YOUR_PARENT_PAGE_ID",
        },
        content: {
            type: "markdown",
            markdown: "# Roadmap\n\nNew roadmap.",
        },
    });
    return page.url;
}
```

## Development

Run:

```bash
submilli build test -p @submilli/notion
```

`NOTION_ACCESS_TOKEN` enables live read coverage. Mutating tests additionally
require `NOTION_LIVE_MUTATIONS=true` and `NOTION_TEST_PARENT_PAGE_ID`. File
upload coverage requires `NOTION_LIVE_UPLOADS=true` because uploaded file
objects cannot be revoked.
