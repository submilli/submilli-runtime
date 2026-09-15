import { label } from "submilli:test";
import {
    CreatePageInput,
    FileReference,
    NotionError,
    PropertyBag,
    PropertyValue,
    createComment,
    notionId,
} from "@submilli/notion";

function main(): void {
    label("Notion references accept compact IDs, UUIDs, URLs, and collection references");
    const compact = "0123456789abcdef0123456789abcdef";
    const dashed = "01234567-89ab-cdef-0123-456789abcdef";
    assert(notionId(compact) === compact, "compact ID is preserved");
    assert(notionId(dashed) === dashed, "dashed UUID is preserved");
    assert(
        notionId("https://www.notion.so/Project-0123456789abcdef0123456789abcdef?pvs=4") === compact,
        "ID is extracted from a Notion URL",
    );
    assert(notionId("collection://" + compact) === compact, "collection reference is accepted");

    label("invalid references fail before network access");
    let invalid = false;
    try {
        notionId("not-a-notion-id");
    } catch (error) {
        if (error instanceof NotionError) {
            invalid = error.code === "invalid_reference" && error.status === 0;
        }
    }
    assert(invalid, "invalid reference throws a stable validation error");

    label("typed properties compose with custom dynamic values");
    const values: PropertyValue[] = [
        { name: "Name", type: "title", text: "Launch plan" },
        { name: "Done", type: "checkbox", checked: false },
        { name: "Priority", type: "select", value: "High" },
        { name: "Estimate", type: "number", number: 3 },
    ];
    const custom = new Map<string, unknown>();
    custom.set("Future property", JSON.parse("{\"status\":{\"name\":\"Ready\"}}"));
    const properties: PropertyBag = { values: values, custom: custom };
    const icon: FileReference = { type: "external", url: "https://example.com/icon.png" };
    const input: CreatePageInput = {
        parent: { type: "page_id", id: compact },
        properties: properties,
        content: { type: "markdown", markdown: "# Launch plan" },
        icon: icon,
    };
    assert(input.properties !== null, "property bag is represented");
    assert(input.content !== null && input.content.type === "markdown", "content strategy is explicit");

    label("NotionError preserves request and retry metadata");
    const error = new NotionError("rate_limited", "slow down", 429, "request-1", "2");
    assert(error.code === "rate_limited", "code is preserved");
    assert(error.status === 429, "status is preserved");
    assert(error.requestId === "request-1", "request ID is preserved");
    assert(error.retryAfter === "2", "Retry-After is preserved");

    label("discussion replies require a verifiable parent before authorization");
    let missingParent = false;
    try {
        createComment({
            target: { type: "discussion", id: dashed },
            markdown: "Reply",
        });
    } catch (cause) {
        if (cause instanceof NotionError) missingParent = cause.code === "missing_discussion_parent";
    }
    assert(missingParent, "bare discussion IDs cannot claim an arbitrary page context");
}
