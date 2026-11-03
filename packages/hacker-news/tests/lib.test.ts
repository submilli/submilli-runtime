import { label, expectException } from "submilli:test";
import { search, getThread } from "@submilli/hacker-news";
import { searchRequest, searchPageFrom, threadFrom } from "../src/internal";

function main(): void {
    label("encodes an exact, date-sorted search with filters");
    const request = searchRequest({ query: "submilli", type: "comment", storyId: 49980140, author: "some_user",
        createdAfter: "2026-10-20T21:00:00.5Z", createdBefore: "2026-10-21T21:00:00.5Z", page: 2, hitsPerPage: 100 });
    assert(request.endpoint === "search_by_date", request.endpoint);
    assert(request.query === "?query=submilli&typoTolerance=false&queryType=prefixNone&tags=comment%2Cstory_49980140%2Cauthor_some_user"
        + "&numericFilters=created_at_i%3E%3D1792530001%2Ccreated_at_i%3C1792616401&page=2&hitsPerPage=100", request.query);
    const loose = searchRequest({ query: "wasm gc", sort: "relevance", exact: false });
    assert(loose.endpoint === "search" && loose.query === "?query=wasm%20gc&page=0&hitsPerPage=50", loose.query);

    label("maps search hits");
    const page = searchPageFrom("{\"hits\":[{\"objectID\":\"49980148\",\"created_at\":\"2026-10-06T15:40:33.000Z\",\"author\":\"somdoron\",\"comment_text\":\"Submilli is\",\"parent_id\":49980140,\"story_id\":49980140,\"story_title\":\"Show HN\",\"_tags\":[\"comment\",\"author_somdoron\",\"story_49980140\"]}],\"nbHits\":1,\"page\":0,\"nbPages\":1,\"exhaustiveNbHits\":true}");
    assert(page.items.length === 1 && page.total === 1 && page.pageCount === 1 && page.exhaustive, "page fields");
    const hit = page.items[0];
    assert(hit.id === 49980148 && hit.type === "comment" && hit.author === "somdoron" && hit.text === "Submilli is" && hit.storyTitle === "Show HN", "hit fields");
    assert(hit.link === "https://news.ycombinator.com/item?id=49980148", hit.link);

    label("flattens a thread depth-first, parents before children");
    const thread = threadFrom("{\"id\":1,\"type\":\"story\",\"title\":\"T\",\"author\":\"a\",\"created_at\":\"x\",\"children\":["
        + "{\"id\":2,\"type\":\"comment\",\"parent_id\":1,\"story_id\":1,\"children\":[{\"id\":4,\"type\":\"comment\",\"parent_id\":2,\"children\":[]}]},"
        + "{\"id\":3,\"type\":\"comment\",\"parent_id\":1,\"author\":null,\"children\":[]}]}");
    const ids: string[] = [];
    for (const item of thread.items) ids.push(item.id.toString());
    assert(ids.join(",") === "1,2,4,3", ids.join(","));
    assert(thread.items[3].author === null && thread.items[2].storyTitle === "T", "fields carried");

    label("exact whole-second bounds keep [after, before)");
    const exact = searchRequest({ query: "x", createdAfter: "2026-10-20T21:00:00Z", createdBefore: "2026-10-20T21:00:01Z" });
    assert(exact.query.includes("created_at_i%3E%3D1792530000%2Ccreated_at_i%3C1792530001"), exact.query);
    const micro = searchRequest({ query: "x", createdAfter: "2026-10-20T21:00:00.000001Z", createdBefore: Temporal.Now.instant().toString() });
    assert(micro.query.includes("created_at_i%3E%3D1792530001"), "a microsecond past the second excludes that second: " + micro.query);
    expectException(() => { searchRequest({ query: "x", createdAfter: "2026-10-20T21:00:00+00:00:00.5" }); }, "HackerNewsError");
    expectException(() => { searchRequest({ query: "x", createdAfter: "2026-10-20T21:00:00z" }); }, "HackerNewsError");
    const carried = searchRequest({ query: "x", createdAfter: "2026-10-20T23:59:59,999999999+00:00[Asia/Jerusalem]" });
    assert(carried.query.includes("created_at_i%3E%3D1792540800"), "comma, nine digits and an annotation carry into the next day: " + carried.query);
    expectException(() => { searchRequest({ query: "x", createdAfter: "2026-10-21T00:00:00Z", createdBefore: "2026-10-20T00:00:00Z" }); }, "HackerNewsError");
    expectException(() => { searchRequest({ query: "x", createdAfter: "2026-10-20T21:00:00Z", createdBefore: "yesterday" }); }, "HackerNewsError");
    expectException(() => { searchRequest({ query: "x", createdAfter: "2026-10-20T21:00:00Z", createdBefore: "2026-10-21" }); }, "HackerNewsError");
    const subSecond = searchRequest({ query: "x", createdAfter: "2026-10-20T21:00:00.2Z", createdBefore: "2026-10-20T21:00:00.8Z" });
    assert(subSecond.query.includes("created_at_i%3E%3D1792530001%2Ccreated_at_i%3C1792530001"), "a window with no whole second is valid and empty: " + subSecond.query);

    label("bodies that are not JSON, or the wrong shape, are invalid responses");
    expectException(() => { searchPageFrom("<html>"); }, "HackerNewsError");
    expectException(() => { threadFrom("<html>"); }, "HackerNewsError");
    expectException(() => { searchPageFrom("null"); }, "HackerNewsError");
    expectException(() => { threadFrom("{\"id\":1,\"children\":[null]}"); }, "HackerNewsError");

    label("operations validate before any check or request");
    expectException(() => { search({ query: " " }); }, "HackerNewsError");
    expectException(() => { search({ query: "x", hitsPerPage: 101 }); }, "HackerNewsError");
    expectException(() => { search({ query: "x", author: "a,b" }); }, "HackerNewsError");
    expectException(() => { search({ query: "x", createdAfter: "yesterday" }); }, "HackerNewsError");
    expectException(() => { getThread(0); }, "HackerNewsError");
}
