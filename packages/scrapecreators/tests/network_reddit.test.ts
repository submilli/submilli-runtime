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
    const url = "https://www.reddit.com/comments/" + id;
    const post = reddit.getPost({ url: url }).data;
    assert(post.id === id, "post identity");
    const comments = reddit.listComments({ url: url }).data;
    assert(comments.post !== undefined && comments.post !== null && comments.post.id === id, "thread identity");

    label("live netsec subreddit searches accept native post metadata");
    for (const query of ["prompt injection", "AI agent", "MCP", "sandbox"]) {
        const result = reddit.searchSubreddit({ subreddit: "netsec", query: query, sort: "top", timeframe: "year" });
        assert(result.data.success === true, "subreddit search succeeds for " + query);
        assert(result.data.posts.length > 0, "subreddit search retains posts for " + query);
    }
}
