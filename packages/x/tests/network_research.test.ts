import { label } from "submilli:test";
import secrets from "submilli:secrets";
import x from "@submilli/x";

function main(): void {
    const token = secrets.get("X_BEARER_TOKEN");
    if (token === undefined || token.trim().length === 0) {
        label("skip: X_BEARER_TOKEN is not bound");
        return;
    }
    label("live recent search returns structured public tweets");
    const page = x.searchRecent("AI lang:en -is:retweet", { maxResults: 10 });
    assert(page.tweets.length > 0 && page.tweets.length <= 10, "bounded nonempty search");
    const found = page.tweets[0];
    assert(found.id.length > 0 && found.text.length > 0 && found.author?.username !== undefined, "source record and expanded author");
    label("live tweet lookup preserves identity and source content");
    const lookup = x.getTweet(found.url);
    assert(lookup.tweet.id === found.id && lookup.tweet.text.length > 0, "lookup identity and text");
    label("live replies return one page of the same conversation");
    const root = lookup.tweet.conversationId;
    assert(root !== null, "conversation ID requested");
    if (root !== null) {
        const replies = x.listReplies(root, { maxResults: 10 });
        for (const reply of replies.tweets) {
            assert(reply.conversationId === root, "same conversation");
            assert(reply.references.some((reference) => reference.type === "replied_to"), "reply relationship");
        }
    }
    label("live full-archive search accepts a bounded explicit time window");
    const archive = x.searchAll("from:XDevelopers -is:retweet", { startTime: "2024-01-01T00:00:00Z", endTime: "2024-02-01T00:00:00Z", maxResults: 10 });
    assert(archive.tweets.length <= 10 && archive.resultCount === archive.tweets.length, "bounded archive page, including valid empty results");
    for (const tweet of archive.tweets) {
        const createdAt = tweet.createdAt;
        assert(createdAt !== null, "publication timestamp requested");
        if (createdAt !== null) {
            const published = Temporal.Instant.from(createdAt);
            assert(Temporal.Instant.compare(published, Temporal.Instant.from("2024-01-01T00:00:00Z")) >= 0, "archive start inclusive");
            assert(Temporal.Instant.compare(published, Temporal.Instant.from("2024-02-01T00:00:00Z")) < 0, "archive end exclusive");
        }
    }
}
