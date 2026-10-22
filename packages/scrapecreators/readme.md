# @submilli/scrapecreators

Synchronous typed ScrapeCreators clients for Reddit, YouTube, LinkedIn, GitHub,
and Twitter/X. Uses native Submilli HTTP; no scraper CLI or Node SDK is required.

## Setup

In an existing blueprint directory:

```sh
submilli install submilli/submilli-runtime @submilli/scrapecreators
submilli blueprint add-package @submilli/scrapecreators --no-capabilities
submilli blueprint secret add SCRAPECREATORS_API_KEY --store scrapecreators_api_key
submilli secret put scrapecreators_api_key
submilli blueprint capability add scrapecreators.com/reddit.search
```

Enter the key at the hidden prompt. Alternatively bind `SCRAPECREATORS_API_KEY`
from a trusted harness:

```yaml
secrets:
  SCRAPECREATORS_API_KEY:
    harness:
      required: true
```

Grant only the operations the caller needs. Every operation has its own
`scrapecreators.com/<platform>.<method>` capability with no filter fields.
Authorization runs before validation, credential access, and HTTP. Package
requirements are derived: GET access to `api.scrapecreators.com`, POST access
only to `/v1/reddit/post/comments`, and access only to `SCRAPECREATORS_API_KEY`.
GET paths are fixed by the client methods; callers cannot supply a provider
origin or endpoint. Source URLs are sent to the provider, which fetches them
remotely; these capabilities do not constrain the provider's redirects.

## Use

```ts
import { scrapecreators } from "@submilli/scrapecreators";

function main(): string {
    const response = scrapecreators.reddit.search({ query: "WebAssembly", timeframe: "month" });
    return JSON.stringify({ sources: response.data.posts, issues: response.issues });
}
```

Named exports `reddit`, `youtube`, `linkedin`, `github`, and `twitter` expose the
same clients as the named `scrapecreators` aggregate. Each method takes one typed options object. Methods without
required parameters accept an omitted object, but identity lookups still need
one of the documented identifiers. Public client classes are stateless; they
never accept credentials or bypass capability checks.

## Results and failures

Each operation returns `Result<T>` with `data`, `raw`, and `issues`:

- `data` uses platform-specific types and preserves provider field names,
  timestamps, units, IDs, engagement metrics, credit charges, cache metadata,
  and pagination. Missing metadata is `undefined`; explicit `null`, zero,
  and empty arrays stay distinct. Never convert large string IDs to numbers.
- Reddit posts and comments in `data` also carry `canonical_url`, which this
  package adds: the thread or comment URL on reddit.com, or null when the
  provider returned no usable permalink or ID. It is not in `raw`.
- `raw` is the complete JSON envelope as `unknown`, retaining future fields.
  It may contain public contact information returned by the provider.
- `issues` exposes provider errors/warnings inside successful envelopes, with
  kind, code, URL, message and raw detail. Inspect it even on HTTP 200. Providers
  may silently omit inaccessible items; an empty issues list does not guarantee
  complete coverage. Match partial failures by URL or ID, not array position.

Pagination is manual: one call returns one page, with no retries or hidden
follow-up calls. Pass native tokens back unchanged. Stop on absent/null tokens
or the endpoint's explicit end flag; do not send null tokens as strings.
GitHub cursors are numeric; YouTube continuation tokens and Reddit `after`
tokens are strings. Reddit comment cursors come from `more.cursor` or a nested
reply's `more.cursor`; do not combine them. Set `usePost: true` to send long
comment cursors in JSON at the same endpoint. Pass a post's `canonical_url` to
`listComments`; a post's `url` is what it links to, such as an image, video,
gallery or external page, and `listComments` rejects it.

`ScrapeCreatorsError` includes `code`, `status`, `providerCode`, and raw
`retryAfter`. Local errors use status 0. Codes are `invalid_argument`,
`missing_credentials`, `invalid_request`, `unauthorized`, `forbidden`,
`quota_exceeded`, `rate_limited`, `unavailable_content`, `provider_error`,
and `invalid_response`. Permission and native transport errors pass through.
Provider HTTP-200 failures also throw. Missing captions reported as a successful
null transcript remain a typed result, rather than an empty successful string.
`decodeResponse(body, status?, retryAfter?)` validates recorded provider envelopes
without credentials or I/O; it returns `unknown`, not an endpoint-specific type.

## Platform limitations

- **Reddit:** global `search` supports `filter: "posts"` or `"comments"`.
  Timeframe and `comment_count` sorting apply only to posts. Subreddit names
  retain case. Native video captions can be unavailable; no AI fallback is used.
- **YouTube:** transcripts use public captions and preserve native segment times
  (`startMs`/`endMs` are provider strings). No AI fallback is used. `originalAudio`
  takes precedence over `language`; missing matching tracks return null.
  `includeExtras` is opt-in and may reduce reliability. Sponsor detection is an
  experimental inference with evidence, not an official sponsor attribution.
- **LinkedIn:** post search uses Google indexing and is incomplete; a date filter
  cannot guarantee exhaustive recent coverage. Search cursors range from 1–11.
  There is no standalone comments endpoint in this client: post details retain
  comments the provider returns. Video transcripts require public native captions.
- **GitHub:** this client retrieves public data through ScrapeCreators and consumes
  its credits. User PR results use GitHub's public search index; activity pages
  cover one month at a time. The separate `@submilli/github` package uses GitHub
  directly with `GITHUB_TOKEN` and supports issue/PR/code research and mutations.
  These clients do not fall back to one another.
- **Twitter/X:** post search uses Google indexing, is not native X search, may miss
  recent posts, and can repeat posts across pages. Search cursors range from 1–11.
  User/community tweet endpoints do not advertise pagination. There is no
  standalone replies endpoint: only returned tweet details are available.
  Video transcription uses AI. Default successful transcripts up to two minutes
  cost one credit; `allowLongTranscription: true` explicitly opts into pricing of
  ten credits per started minute, including short videos. It is never enabled
  automatically. Null transcripts cost no transcription credits.

All platforms depend on public availability and provider coverage. Preserve
unknown dates and unavailable metrics instead of fabricating recency or zero.
Calls use the native HTTP timeout (30 seconds); slow provider scrapes or long AI
transcriptions may exceed it. No automatic media downloads or paid fallback runs.
See the [provider introduction](https://docs.scrapecreators.com/introduction/)
and [current endpoint catalog](https://docs.scrapecreators.com/openapi.json).

## Platform examples

```ts
import { youtube } from "@submilli/scrapecreators";

function main(): string {
    return JSON.stringify(youtube.getTranscript({ url: "https://www.youtube.com/watch?v=dQw4w9WgXcQ", language: "en" }).data);
}
```

```ts
import { linkedin } from "@submilli/scrapecreators";

function main(): string {
    return JSON.stringify(linkedin.searchPosts({ query: "WebAssembly", datePosted: "last-month" }).data);
}
```

```ts
import { github } from "@submilli/scrapecreators";

function main(): string {
    return JSON.stringify(github.listUserPullRequests({ handle: "octocat", since: "2026-01-01" }).data);
}
```

```ts
import { twitter } from "@submilli/scrapecreators";

function main(): string {
    return JSON.stringify(twitter.searchPosts({ query: "WebAssembly" }).data);
}
```

## Recent-topic research

This bounded example retrieves three discovery pages and one selected Reddit
thread. It keeps source metadata and records failures independently. Search
filters are discovery aids; check returned timestamps against the actual desired
window before describing any source as recent.

```ts
import { reddit, linkedin, twitter } from "@submilli/scrapecreators";

function main(): string {
    const sources: unknown[] = [];
    const failures: string[] = [];
    try {
        const response = reddit.search({ query: "WebAssembly", timeframe: "month", sort: "new" });
        sources.push(response);
        const posts = response.data.posts;
        if (posts !== undefined && posts !== null && posts.length > 0) {
            const thread = posts[0].canonical_url;
            if (thread !== undefined && thread !== null) {
                try { sources.push(reddit.listComments({ url: thread })); }
                catch (error) { failures.push("Reddit thread: " + error.message); }
            }
        }
    } catch (error) { failures.push("Reddit search: " + error.message); }
    try { sources.push(linkedin.searchPosts({ query: "WebAssembly", datePosted: "last-month" })); }
    catch (error) { failures.push("LinkedIn: " + error.message); }
    try { sources.push(twitter.searchPosts({ query: "WebAssembly" })); }
    catch (error) { failures.push("Twitter/X: " + error.message); }
    return JSON.stringify({ sources: sources, failures: failures });
}
```

## Operation reference

All parameters are camelCase options. `!` marks required fields; other options
are omitted by default. Provider response fields retain their native spelling.
Follow each endpoint link for value ranges, defaults, pagination and credit costs.

### reddit

| Method and capability suffix | Options | Provider endpoint |
|---|---|---|
| `reddit.getSubreddit` | `subreddit`, `url`, `cacheMaxAge` | [/v1/reddit/subreddit/details](https://docs.scrapecreators.com/v1/reddit/subreddit/details/) |
| `reddit.listSubredditPosts` | `subreddit!`, `timeframe`, `sort`, `after`, `trim`, `cacheMaxAge` | [/v1/reddit/subreddit](https://docs.scrapecreators.com/v1/reddit/subreddit/) |
| `reddit.searchSubreddit` | `subreddit!`, `query`, `sort`, `timeframe`, `cursor` | [/v1/reddit/subreddit/search](https://docs.scrapecreators.com/v1/reddit/subreddit/search/) |
| `reddit.getPost` | `url!` | [/v1/reddit/post](https://docs.scrapecreators.com/v1/reddit/post/) |
| `reddit.listComments` | `url!`, `cursor`, `trim`, `usePost` | [/v1/reddit/post/comments](https://docs.scrapecreators.com/v1/reddit/post/comments/) |
| `reddit.getTranscript` | `url!`, `language`, `cacheMaxAge` | [/v1/reddit/post/transcript](https://docs.scrapecreators.com/v1/reddit/post/transcript/) |
| `reddit.search` | `query!`, `filter`, `sort`, `timeframe`, `after`, `trim` | [/v1/reddit/search](https://docs.scrapecreators.com/v1/reddit/search/) |

### youtube

| Method and capability suffix | Options | Provider endpoint |
|---|---|---|
| `youtube.getChannel` | `channelId`, `handle`, `url`, `cacheMaxAge` | [/v1/youtube/channel](https://docs.scrapecreators.com/v1/youtube/channel/) |
| `youtube.listChannelVideos` | `channelId`, `handle`, `sort`, `continuationToken`, `isPaidPromotions`, `includeExtras` | [/v1/youtube/channel-videos](https://docs.scrapecreators.com/v1/youtube/channel-videos/) |
| `youtube.listChannelPlaylists` | `channelId`, `handle`, `continuationToken` | [/v1/youtube/channel/playlists](https://docs.scrapecreators.com/v1/youtube/channel/playlists/) |
| `youtube.listChannelLives` | `channelId`, `handle`, `continuationToken` | [/v1/youtube/channel/lives](https://docs.scrapecreators.com/v1/youtube/channel/lives/) |
| `youtube.listChannelCommunityPosts` | `channelId`, `handle`, `continuationToken` | [/v1/youtube/channel/community-posts](https://docs.scrapecreators.com/v1/youtube/channel/community-posts/) |
| `youtube.listChannelShorts` | `handle`, `channelId`, `sort`, `continuationToken` | [/v1/youtube/channel/shorts](https://docs.scrapecreators.com/v1/youtube/channel/shorts/) |
| `youtube.getVideo` | `url!`, `language`, `cacheMaxAge` | [/v1/youtube/video](https://docs.scrapecreators.com/v1/youtube/video/) |
| `youtube.getTranscript` | `url!`, `language`, `originalAudio`, `cacheMaxAge` | [/v1/youtube/video/transcript](https://docs.scrapecreators.com/v1/youtube/video/transcript/) |
| `youtube.getSponsors` | `url!`, `language` | [/v1/youtube/video/sponsors](https://docs.scrapecreators.com/v1/youtube/video/sponsors/) |
| `youtube.search` | `query!`, `uploadDate`, `sortBy`, `type`, `duration`, `region`, `continuationToken`, `includeExtras` | [/v1/youtube/search](https://docs.scrapecreators.com/v1/youtube/search/) |
| `youtube.searchTypeahead` | `query!` | [/v1/youtube/search/typeahead](https://docs.scrapecreators.com/v1/youtube/search/typeahead/) |
| `youtube.searchHashtag` | `hashtag!`, `continuationToken`, `type` | [/v1/youtube/search/hashtag](https://docs.scrapecreators.com/v1/youtube/search/hashtag/) |
| `youtube.listComments` | `url!`, `continuationToken`, `order` | [/v1/youtube/video/comments](https://docs.scrapecreators.com/v1/youtube/video/comments/) |
| `youtube.listCommentReplies` | `continuationToken!` | [/v1/youtube/video/comment/replies](https://docs.scrapecreators.com/v1/youtube/video/comment/replies/) |
| `youtube.listTrendingShorts` | None | [/v1/youtube/shorts/trending](https://docs.scrapecreators.com/v1/youtube/shorts/trending/) |
| `youtube.getPlaylist` | `playlistId!`, `continuationToken` | [/v1/youtube/playlist](https://docs.scrapecreators.com/v1/youtube/playlist/) |
| `youtube.getCommunityPost` | `url!` | [/v1/youtube/community-post](https://docs.scrapecreators.com/v1/youtube/community-post/) |

### linkedin

| Method and capability suffix | Options | Provider endpoint |
|---|---|---|
| `linkedin.getProfile` | `url!` | [/v1/linkedin/profile](https://docs.scrapecreators.com/v1/linkedin/profile/) |
| `linkedin.getCompany` | `url!` | [/v1/linkedin/company](https://docs.scrapecreators.com/v1/linkedin/company/) |
| `linkedin.listCompanyPosts` | `url!`, `page` | [/v1/linkedin/company/posts](https://docs.scrapecreators.com/v1/linkedin/company/posts/) |
| `linkedin.searchPosts` | `query!`, `datePosted`, `cursor` | [/v1/linkedin/search/posts](https://docs.scrapecreators.com/v1/linkedin/search/posts/) |
| `linkedin.getPost` | `url!` | [/v1/linkedin/post](https://docs.scrapecreators.com/v1/linkedin/post/) |
| `linkedin.getTranscript` | `url!` | [/v1/linkedin/post/transcript](https://docs.scrapecreators.com/v1/linkedin/post/transcript/) |

### github

| Method and capability suffix | Options | Provider endpoint |
|---|---|---|
| `github.getUser` | `handle`, `url` | [/v1/github/user](https://docs.scrapecreators.com/v1/github/user/) |
| `github.listUserRepositories` | `handle`, `url`, `type`, `sort`, `direction`, `cursor` | [/v1/github/user/repositories](https://docs.scrapecreators.com/v1/github/user/repositories/) |
| `github.listUserPullRequests` | `handle!`, `since`, `until`, `cursor` | [/v1/github/user/pull-requests](https://docs.scrapecreators.com/v1/github/user/pull-requests/) |
| `github.listUserActivity` | `handle`, `url`, `year`, `cursor` | [/v1/github/user/activity](https://docs.scrapecreators.com/v1/github/user/activity/) |
| `github.listFollowers` | `handle`, `url`, `cursor` | [/v1/github/user/followers](https://docs.scrapecreators.com/v1/github/user/followers/) |
| `github.listFollowing` | `handle`, `url`, `cursor` | [/v1/github/user/following](https://docs.scrapecreators.com/v1/github/user/following/) |
| `github.getContributions` | `handle`, `url`, `year` | [/v1/github/user/contributions](https://docs.scrapecreators.com/v1/github/user/contributions/) |
| `github.getRepository` | `url!` | [/v1/github/repository](https://docs.scrapecreators.com/v1/github/repository/) |
| `github.listTrendingRepositories` | `language`, `since`, `spokenLanguageCode` | [/v1/github/trending/repositories](https://docs.scrapecreators.com/v1/github/trending/repositories/) |
| `github.listTrendingDevelopers` | `language`, `since` | [/v1/github/trending/developers](https://docs.scrapecreators.com/v1/github/trending/developers/) |

### twitter

| Method and capability suffix | Options | Provider endpoint |
|---|---|---|
| `twitter.searchPosts` | `query!`, `cursor` | [/v1/twitter/search/posts](https://docs.scrapecreators.com/v1/twitter/search/posts/) |
| `twitter.getProfile` | `handle!`, `cacheMaxAge` | [/v1/twitter/profile](https://docs.scrapecreators.com/v1/twitter/profile/) |
| `twitter.listUserTweets` | `handle!`, `trim` | [/v1/twitter/user-tweets](https://docs.scrapecreators.com/v1/twitter/user-tweets/) |
| `twitter.getTweet` | `url!`, `trim`, `cacheMaxAge` | [/v1/twitter/tweet](https://docs.scrapecreators.com/v1/twitter/tweet/) |
| `twitter.getTranscript` | `allowLongTranscription`, `url!`, `cacheMaxAge` | [/v1/twitter/tweet/transcript](https://docs.scrapecreators.com/v1/twitter/tweet/transcript/) |
| `twitter.getCommunity` | `url!` | [/v1/twitter/community](https://docs.scrapecreators.com/v1/twitter/community/) |
| `twitter.listCommunityTweets` | `url!` | [/v1/twitter/community/tweets](https://docs.scrapecreators.com/v1/twitter/community/tweets/) |

## Development

```sh
cargo run -p submilli -- build test --skip-network -p @submilli/scrapecreators
node --harmony-temporal --test packages/scrapecreators/scripts/contract.test.mjs
```

Offline native tests check recorded response types and errors. Node contracts
check requests, permission ordering, credential handling, pagination and errors
against synthetic fixtures; they do not replace Submilli's checked casts.
The Temporal flag enables Node's native Temporal API for date-validation parity.
The operation inventory and fixtures live under `scripts/` and were checked
against the provider OpenAPI catalog on 2026-10-16.

Live smoke tests cover all five platforms, including Reddit search/post/comments
and YouTube captions. They consume provider credits. Credentials are opt-in:

```sh
cargo run -p submilli -- build test -p @submilli/scrapecreators --env-file .env
```

Keep `.env` untracked. No credentials are loaded implicitly. Without the bound
key, live files report an explicit skip label. With a key, provider/network
failures fail the check; unavailable coverage must not be reported as passed.
