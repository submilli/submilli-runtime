// A read-only Hacker News client over the public HN Search API
// (hn.algolia.com): full-text search of stories and comments, and whole item
// threads. The API needs no key.

import { get, Response } from "submilli:http";
import { check } from "submilli:security";
import { HackerNewsError, requireId, searchPageFrom, SearchOptions, SearchPage, searchRequest, Thread, threadFrom } from "./internal";

export { HackerNewsError, Item, SearchOptions, SearchPage, Thread } from "./internal";

/**
 * Search stories and comments. Newest first by default, with exact word
 * matching. Page with `page` while `page + 1 < pageCount`.
 * @param options Query text, filters, and paging.
 * @returns One page of matching items.
 * @capability news.ycombinator.com/search {}
 */
export function search(options: SearchOptions): SearchPage {
    const { query, sort, exact, type, storyId, author, createdAfter, createdBefore, page, hitsPerPage } = options;
    const request = searchRequest(query, sort, exact, type, storyId, author, createdAfter, createdBefore, page, hitsPerPage);
    check("news.ycombinator.com/search", {});
    const response = request.endpoint === "search_by_date"
        ? get("https://hn.algolia.com/api/v1/search_by_date" + request.query)
        : get("https://hn.algolia.com/api/v1/search" + request.query);
    return searchPageFrom(succeeded(response).body);
}

/**
 * Read an item and every reply under it, as one flat list with the root first.
 * Pass a story id for the whole discussion.
 * @param itemId Story or comment id.
 * @returns The thread, or null when the item does not exist.
 * @capability news.ycombinator.com/items.get { itemId: number }
 */
export function getThread(itemId: number): Thread | null {
    const id = requireId(itemId, "itemId");
    check("news.ycombinator.com/items.get", { itemId: id });
    const response = get("https://hn.algolia.com/api/v1/items/" + id.toString());
    if (response.status === 404) return null;
    return threadFrom(succeeded(response).body);
}

function succeeded(response: Response): Response {
    if (response.ok) return response;
    const detail = response.body.length > 300 ? response.body.substring(0, 300) : response.body;
    throw new HackerNewsError("http_" + response.status.toString(), "HN Search API returned HTTP " + response.status.toString() + ": " + detail, response.status);
}
