import { label } from "submilli:test";
import { search, getThread } from "@submilli/hacker-news";

function main(): void {
    label("exact search finds the Submilli launch story and its thread");
    const page = search({ query: "submilli", type: "story", hitsPerPage: 20 });
    assert(page.items.length > 0, "at least one story");
    const thread = getThread(page.items[page.items.length - 1].id);
    assert(thread !== null && thread.items.length >= 1 && thread.items[0].type === "story", "thread root is the story");
}
