import { label } from "submilli:test";
import secrets from "submilli:secrets";
import { getSelf, search } from "@submilli/notion";

function main(): void {
    label("live Notion identity and bounded search when a token is available");
    if (secrets.get("NOTION_ACCESS_TOKEN") === null) return;
    const bot = getSelf();
    assert(bot.id.length > 0, "integration bot has an ID");
    const page = search({ pageSize: 1 });
    assert(page.results.length <= 1, "search respects the requested bound");
}
