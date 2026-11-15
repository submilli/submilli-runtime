import { label } from "submilli:test";
import { getTweet, searchRecent, searchAll, listReplies, XError } from "@submilli/x";
import { tweetId, buildSearchQuery, decodeTweet, decodePage, xHttpError } from "../src/internal";

function expectCode(code: string, action: () => void): void {
    let matched = false;
    try { action(); } catch (cause) {
        matched = cause instanceof XError && cause.code === code && cause.status === 0;
    }
    assert(matched, "expected " + code);
}

function main(): void {
    label("tweet identity preserves string precision and accepts supported sharing URLs");
    assert(tweetId("1234567890123456789") === "1234567890123456789", "no lossy number conversion");
    for (const url of ["https://x.com/user/status/123", "https://twitter.com/user/status/123?s=20", "https://www.x.com/i/status/123#reply", "https://mobile.twitter.com/i/web/status/123/video/1"]) {
        assert(tweetId(url) === "123", "numeric ID from supported URL");
    }
    for (const value of ["", "0", "-123", "123abc", "123\n", " 123", "1.5", "12345678901234567890", "https://x.com/user", "https://evil.test/user/status/123", "https://x.com.evil.test/user/status/123", "https://x.com@evil.test/user/status/123", "https://x.com/user/status/123/../456", "https://x.com/user/status/123/%2e%2e/456", "https://x.com/user/status/123\n"]) {
        expectCode("invalid_argument", () => { getTweet(value); });
        expectCode("invalid_argument", () => { listReplies(value); });
    }

    label("search encoding bounds page size and preserves opaque query and cursor");
    const params = buildSearchQuery('(Acme OR "Acme Corp") -is:retweet', { nextToken: "a+b&c", maxResults: 10, startTime: "2026-01-01T01:00:00+01:00", endTime: "2026-01-02T00:00:00Z" }, 100);
    assert(params.includes("max_results=10") && params.includes("sort_order=recency"), "bounded defaults");
    assert(params.includes("next_token=a%2Bb%26c") && params.includes("start_time=2026-01-01T00%3A00%3A00Z"), "cursor escaped and timezone normalized");
    assert(params.includes("tweet.fields=") && params.includes("expansions=author_id"), "fixed author and tweet fields");
    assert(buildSearchQuery("q", { maxResults: 500 }, 500).includes("max_results=500"), "archive page limit");
    assert(buildSearchQuery("q", { startTime: "2026-01-01T00:00:00.000001Z", endTime: "2026-01-01T00:00:00.000002Z" }, 100).includes("start_time="), "sub-millisecond windows preserve timestamp precision");
    expectCode("invalid_argument", () => { searchRecent("  "); });
    expectCode("invalid_argument", () => { searchAll("q".repeat(4097)); });
    for (const count of [0, 9, 101, 1.5, NaN, Infinity]) expectCode("invalid_argument", () => { searchRecent("q", { maxResults: count }); });
    expectCode("invalid_argument", () => { searchAll("q", { maxResults: 501 }); });
    expectCode("invalid_argument", () => { searchRecent("q", { nextToken: " " }); });
    for (const date of ["2026-01-01", "2026-02-30T00:00:00Z", "2026-01-01T00:00:00", "yesterday"]) expectCode("invalid_argument", () => { searchRecent("q", { startTime: date }); });
    expectCode("invalid_argument", () => { searchRecent("q", { startTime: "2026-02-01T00:00:00Z", endTime: "2026-01-01T00:00:00Z" }); });
    expectCode("invalid_argument", () => { searchRecent("q", { startTime: "2026-01-01T00:00:00Z", endTime: "2026-01-01T00:00:00Z" }); });

    label("lookup returns source fields, full note text, author identity and reply references");
    const result = decodeTweet('{"data":{"id":"123","text":"truncated","note_tweet":{"text":"Full long-form post"},"author_id":"42","conversation_id":"100","created_at":"2026-01-01T00:00:00Z","lang":"en","public_metrics":{"like_count":0,"reply_count":3,"retweet_count":2},"referenced_tweets":[{"type":"replied_to","id":"122"}]},"includes":{"users":[{"id":"99","name":"Other","username":"other"},{"id":"42","name":"Author","username":"author"}]}}', "123");
    assert(result.tweet.text === "Full long-form post" && result.tweet.url === "https://x.com/i/status/123", "source and full text");
    assert(result.tweet.author?.username === "author" && result.tweet.authorId === "42", "authors joined by ID");
    assert(result.tweet.conversationId === "100" && result.tweet.references[0].id === "122", "root and parent differ");
    assert(result.tweet.metrics.likes === 0 && result.tweet.metrics.replies === 3 && result.tweet.metrics.bookmarks === null, "unknown is not zero");
    assert(!Object.keys(result).includes("raw") && result.issues.length === 0, "typed result only");
    const sparse = decodeTweet('{"data":{"id":"123","text":"post"}}', "123");
    assert(sparse.tweet.author === null && sparse.tweet.metrics.likes === null && sparse.tweet.references.length === 0, "missing optional fields");
    const modern = decodeTweet('{"data":{"id":"123","text":"short","note_post":{"text":"full"},"referenced_posts":[{"id":"122","type":"quoted"}],"public_metrics":{"repost_count":7}}}', "123");
    assert(modern.tweet.text === "full" && modern.tweet.metrics.reposts === 7 && modern.tweet.references[0].type === "quoted", "documented post naming also decoded");

    label("search pages and partial expansion issues remain explicit");
    const page = decodePage('{"data":[{"id":"123","text":"reply","author_id":"42"}],"meta":{"result_count":1,"next_token":"next"},"errors":[{"title":"Not Found Error","detail":"Author unavailable","resource_id":"42"}]}');
    assert(page.tweets.length === 1 && page.resultCount === 1 && page.nextToken === "next", "one page with cursor");
    assert(page.tweets[0].author === null && page.issues[0].resourceId === "42", "partial expansion not silently dropped");
    assert(decodePage('{"meta":{"result_count":0}}').tweets.length === 0, "omitted data on zero results");
    assert(decodePage('{"data":[],"meta":{"result_count":0,"next_token":null}}').nextToken === null, "empty final page");
    expectCode("provider_error", () => { decodePage('{"errors":[{"title":"Failure"}]}'); });
    expectCode("provider_error", () => { decodePage('{"data":[],"meta":{"result_count":0},"errors":[{"title":"Failure"}]}'); });
    expectCode("provider_error", () => { decodeTweet('{"errors":[{"title":"Unavailable"}]}', "123"); });

    label("malformed source records cannot become successful defaults");
    for (const body of ["{}", "null", "[]", "<html>error</html>", '{"data":{}}', '{"data":{"id":"123","text":2}}', '{"data":{"id":"123","text":"x","public_metrics":{"like_count":-1}}}']) {
        expectCode("invalid_response", () => { decodeTweet(body, "123"); });
    }
    expectCode("invalid_response", () => { decodeTweet('{"data":{"id":"456","text":"wrong tweet"}}', "123"); });
    for (const body of ["{}", '{"data":[],"meta":{"result_count":1}}', '{"meta":{"result_count":1}}', '{"data":{},"meta":{"result_count":0}}', '{"data":[],"meta":{"result_count":0,"next_token":""}}']) {
        expectCode("invalid_response", () => { decodePage(body); });
    }

    label("HTTP errors retain credential, credit and retry failure distinctions");
    assert(xHttpError(401, "").code === "unauthorized" && xHttpError(402, "").code === "quota_exceeded", "auth and credits");
    assert(xHttpError(403, "").code === "forbidden" && xHttpError(404, "").code === "not_found", "access and not-found");
    assert(xHttpError(400, "").code === "invalid_request" && xHttpError(503, "<html>").code === "http_error", "bad requests and operational failures");
    const rate = xHttpError(429, '{"title":"Too Many Requests","detail":"Rate limit exceeded","type":"about:blank"}', "10", "1800000000");
    assert(rate.status === 429 && rate.code === "rate_limited" && rate.retryAfter === "10" && rate.rateLimitReset === "1800000000", "both retry headers");
    assert(rate.issues[0].detail === "Rate limit exceeded", "provider explanation preserved");
}
