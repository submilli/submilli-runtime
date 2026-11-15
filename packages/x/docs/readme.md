# X API research

Use `@submilli/x` for structured public tweet data from the official X API v2.
Use `@submilli/xai` when you want a generated research answer instead.
Credentials are supplied internally. Never ask for or pass API keys in calls.
This package has no posting, messaging, liking, following, or deletion operations.

<!-- submilli:if capability="x.com/getTweet" -->

```ts
import { getTweet } from "@submilli/x";

function main(): string {
    const result = getTweet("https://x.com/XDevelopers/status/1234567890");
    return JSON.stringify(result);
}
```

`getTweet(idOrUrl)` accepts a numeric ID string or HTTPS post URL on `x.com` or
`twitter.com`, including `www` and `mobile` variants. Username/status, i/status,
and i/web/status paths are accepted, along with sharing parameters, fragments,
and photo/video suffixes. Other hosts, profiles, and path traversal are rejected.
The `x.com/getTweet` capability is checked with the normalized numeric `id`.

The result is `{ tweet, issues }`. Tweet fields: `id`, canonical `url`, `text`,
nullable `authorId`, `author`, `createdAt`, `conversationId`, `language`, `metrics`,
and `references`. `text` prefers full long-form note text when supplied. Expanded
authors contain `id`, `name`, and `username`. Missing fields and counters remain
null; missing reference lists are empty. Counts include likes, replies, reposts,
quotes, bookmarks, and impressions. There is no generated summary or raw envelope.

<!-- submilli:endif -->

<!-- submilli:if capability="x.com/searchRecent" -->

```ts
import { searchRecent, listReplies } from "@submilli/x";

function main(): string {
    const page = searchRecent('(Acme OR "Acme Corp") -is:retweet', { maxResults: 10 });
    const authorAndMentions = searchRecent('(from:XDevelopers OR @XDevelopers) -is:retweet', { maxResults: 10 });
    const replies = listReplies("1234567890", { maxResults: 10 });
    return JSON.stringify({ page: page, authorAndMentions: authorAndMentions, replies: replies });
}
```

`searchRecent(query, options?)` searches the last seven days and requires
`x.com/searchRecent`. Advanced X query operators are passed through as supplied.
Use `from:handle` for posts by an account and `@handle` for posts mentioning it;
the example combines both with `OR`.

<!-- submilli:endif -->

<!-- submilli:if capability="x.com/searchAll" -->

```ts
import { searchAll } from "@submilli/x";

function main(): string {
    return JSON.stringify(searchAll("from:XDevelopers", {
        startTime: "2024-01-01T00:00:00Z",
        endTime: "2024-02-01T00:00:00Z",
        maxResults: 10,
    }));
}
```

`searchAll(query, options?)` searches the full archive and requires
`x.com/searchAll` and provider archive access/credits.

<!-- submilli:endif -->

<!-- submilli:if any="x.com/searchRecent x.com/searchAll" -->

Searches return `{ tweets, nextToken, resultCount, issues }`. Each call fetches
one page. Pass `nextToken` in the next call with the same query and other options;
stop when it is null. `resultCount` counts this page, not all matching tweets.
An empty page is valid. No automatic pagination or retries occur.

Search options: `maxResults` defaults to 10 (10–100 recent, 10–500 archive),
`nextToken` is an opaque cursor, `sortOrder` is `recency` (default) or `relevancy`,
and `startTime`/`endTime` are RFC 3339 timestamps with timezones. Start is
inclusive; end is exclusive. Start must precede end. The provider validates
recency, query operators, and access limits. Queries must contain 1–4096 characters;
provider/account limits may be narrower.

`listReplies(conversationId, options?)` builds a `conversation_id:ID is:reply`
search. It includes nested replies and excludes the original post. Pass the
root conversation ID, not the ID of an arbitrary nested reply. Read
`getTweet(...).tweet.conversationId` when needed. Replies older than seven days
need `{ archive: true }`; this delegates to archive search and requires
`x.com/searchAll`. The default uses recent search and its capability.
The same search options and page contract apply. No completeness is promised
across time windows, unavailable posts, pagination, or indexing delays.

<!-- submilli:endif -->

Inspect `issues` for partial errors, such as an unavailable expanded author.
Responses containing errors but no tweet data throw `XError` with those issues.
HTTP errors preserve `status`, structured `issues`, nullable `retryAfter`, and
nullable `rateLimitReset` (the raw x-rate-limit-reset header, Unix seconds).
Local errors have status 0. Codes include `invalid_argument`,
`missing_credentials`, `invalid_response`, `provider_error`, `invalid_request`,
`unauthorized`, `quota_exceeded`, `forbidden`, `not_found`, `rate_limited`, and
`http_error`. Runtime permission and transport errors pass through unchanged.

Each read consumes X API credits according to the provider's current pricing;
expanded authors can incur user-read charges. Page sizes are cost controls, not
dollar caps. The package does not infer exact charges from returned record counts
because billing and deduplication are provider-managed. Use X account spending
limits. An xAI API key cannot authenticate these requests.
