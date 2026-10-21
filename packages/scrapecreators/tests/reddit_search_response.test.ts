import { label } from "submilli:test";
import { decodeResponse, RedditSearchSubredditData } from "@submilli/scrapecreators";

function main(): void {
    label("subreddit search accepts the live string subreddit shape");
    const result = decodeResponse('{"success":true,"posts":[{"id":"post","post_id":"t3_post","subreddit":"netsec","title":"Example","score":91,"ups":91,"downs":0,"upvote_ratio":null,"created":1787060726,"created_utc":1787060726,"created_at_iso":"2026-08-18T13:45:26.000Z","num_comments":0,"thumbnail":null}],"cursor":null}') as RedditSearchSubredditData;
    assert(result.posts.length === 1, "search results survive the checked cast");
    assert(result.posts[0].title === "Example", "post title survives");
    assert(result.posts[0].subreddit === "netsec", "native subreddit name stays a string");
    assert(result.posts[0].votes === undefined, "missing votes remain missing");
    assert((result.posts[0].votes ?? result.posts[0].score ?? result.posts[0].ups ?? null) === 91, "native vote fields are typed");
    assert(result.posts[0].created_utc === 1787060726, "native timestamps stay in seconds");
    assert(result.posts[0].num_comments === 0, "zero comments remain zero");
    assert(result.cursor === null, "end of pagination remains null");
}
