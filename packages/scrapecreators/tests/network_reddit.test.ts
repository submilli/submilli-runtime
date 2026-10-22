import { label } from "submilli:test";
import secrets from "submilli:secrets";
import { reddit } from "@submilli/scrapecreators";
function main(): void {
    const key = secrets.get("SCRAPECREATORS_API_KEY");
    if (key === undefined || key.trim().length === 0) {
        label("skip: SCRAPECREATORS_API_KEY is not bound");
        return;
    }
    label("live Reddit search, post and comments retain a source");
    const search = reddit.search({ query: "programming", timeframe: "month", sort: "top" }).data;
    const posts = search.posts;
    if (posts === undefined || posts === null || posts.length === 0)
        throw new Error("No Reddit search results");
    const id = posts[0].id;
    if (id === undefined || id === null || id.length === 0) throw new Error("Missing Reddit post ID");
    const url = posts[0].canonical_url;
    if (url === undefined || url === null) throw new Error("Missing Reddit canonical URL");
    const post = reddit.getPost({ url: url }).data;
    assert(post.id === id, "post identity");
    const comments = reddit.listComments({ url: url }).data;
    assert(comments.post !== undefined && comments.post !== null && comments.post.id === id, "thread identity");

    label("live subreddit link posts open their thread through canonical_url");
    const listing = reddit.listSubredditPosts({ subreddit: "pics", sort: "new" }).data;
    let opened = 0;
    for (const item of listing.posts) {
        if (opened === 2) break;
        const thread = item.canonical_url;
        if (thread === undefined || thread === null) throw new Error("Missing Reddit canonical URL");
        if (item.url === undefined || item.url === null || item.url.startsWith("https://www.reddit.com/r/")) continue;
        const replies = reddit.listComments({ url: thread, trim: true }).data;
        assert(replies.post !== undefined && replies.post !== null && replies.post.id === item.id, "link post thread identity");
        opened++;
    }
    assert(opened > 0, "a link post was found and opened");

    label("live netsec subreddit searches accept native post metadata");
    for (const query of ["prompt injection", "AI agent", "MCP", "sandbox"]) {
        const result = reddit.searchSubreddit({ subreddit: "netsec", query: query, sort: "top", timeframe: "year" });
        assert(result.data.success === true, "subreddit search succeeds for " + query);
        assert(result.data.posts.length > 0, "subreddit search retains posts for " + query);
    }
}
