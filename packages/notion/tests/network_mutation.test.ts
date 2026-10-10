import { label } from "submilli:test";
import secrets from "submilli:secrets";
import {
    appendBlockChildren,
    appendPageMarkdown,
    createPage,
    getBlock,
    listBlockChildren,
    readPageMarkdown,
    replacePageMarkdown,
    restoreBlock,
    trashBlock,
    updateBlock,
    trashPage,
    updatePageMarkdown,
} from "@submilli/notion";

function main(): void {
    label("live enhanced Markdown page lifecycle");
    if (secrets.get("NOTION_ACCESS_TOKEN") === undefined) return;
    if (secrets.get("NOTION_LIVE_MUTATIONS") !== "true") return;
    const parentId = secrets.get("NOTION_TEST_PARENT_PAGE_ID");
    if (parentId === undefined) return;

    const stamp = Temporal.Now.instant().epochMilliseconds.toString();
    const page = createPage({
        parent: { type: "page_id", id: parentId },
        content: { type: "markdown", markdown: "# Submilli Notion test " + stamp + "\n\nOriginal marker." },
    });
    try {
        const initial = readPageMarkdown(page.id);
        assert(initial.markdown.includes("Original marker"), "created Markdown is readable");
        const updated = updatePageMarkdown(page.id, { oldText: "Original marker.", newText: "Updated marker." });
        assert(updated.markdown.includes("Updated marker"), "targeted replacement is applied");
        const replaced = replacePageMarkdown(page.id, "# Replacement " + stamp, true);
        assert(replaced.markdown.includes("Replacement"), "full replacement is applied");
        const appended = appendPageMarkdown(page.id, "\n\nAppended marker.");
        assert(appended.markdown.includes("Appended marker"), "append is applied");

        const togglePage = appendBlockChildren(page.id, {
            childrenJson: [
                "{\"object\":\"block\",\"type\":\"toggle\",\"toggle\":{\"rich_text\":[{\"type\":\"text\",\"text\":{\"content\":\"Container\"}}]}}",
            ],
        });
        const toggleId = togglePage.results[0].id;
        assert(getBlock(toggleId).type === "toggle", "nested block is retrievable");
        assert(listBlockChildren(page.id).results.length > 0, "page children are listable");

        const nestedPage = appendBlockChildren(toggleId, {
            childrenJson: [
                "{\"object\":\"block\",\"type\":\"paragraph\",\"paragraph\":{\"rich_text\":[{\"type\":\"text\",\"text\":{\"content\":\"Nested marker.\"}}]}}",
            ],
        });
        const nestedId = nestedPage.results[0].id;
        const paragraph = new Map<string, unknown>();
        paragraph.set(
            "paragraph",
            JSON.parse("{\"rich_text\":[{\"type\":\"text\",\"text\":{\"content\":\"Updated nested marker.\"}}]}"),
        );
        assert(updateBlock(nestedId, paragraph).type === "paragraph", "nested block is updateable");
        assert(trashBlock(nestedId).inTrash, "nested block can be moved to trash");
        assert(!restoreBlock(nestedId).inTrash, "nested block can be restored");
        assert(listBlockChildren(toggleId).results.length === 1, "nested children are listable");
    } finally {
        const trashed = trashPage(page.id);
        assert(trashed.inTrash, "temporary page is moved to trash");
    }
}
