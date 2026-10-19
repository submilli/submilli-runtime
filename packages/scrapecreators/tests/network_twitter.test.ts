import { label } from "submilli:test";
import secrets from "submilli:secrets";
import { twitter } from "@submilli/scrapecreators";
function main(): void {
    const key = secrets.get("SCRAPECREATORS_API_KEY");
    if (key === undefined || key.trim().length === 0) {
        label("skip: SCRAPECREATORS_API_KEY is not bound");
        return;
    }
    label("live Twitter/X search preserves native post IDs and source URLs");
    const response = twitter.searchPosts({ query: "OpenAI" }).data;
    assert(response.posts.length > 0, "indexed posts");
    const post = response.posts[0];
    assert(post.rest_id !== undefined && post.rest_id !== null && post.url !== undefined && post.url !== null, "post attribution");
}
