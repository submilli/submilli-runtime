# ScrapeCreators

Synchronous typed ScrapeCreators clients for Reddit, YouTube, LinkedIn, GitHub,
and Twitter/X. Uses native Submilli HTTP; no scraper CLI or Node SDK is required.

## Credentials and permissions

The ScrapeCreators API key is supplied internally. Never pass a key. Every
operation has its own `scrapecreators.com/<platform>.<method>` capability with
no filter fields. Authorization runs before validation, credential access, and
HTTP, so a permission denial means the operation is forbidden, not mis-called.
Request paths are fixed by the client methods; callers cannot supply a provider
origin or endpoint. Source URLs are sent to the provider, which fetches them
remotely.

## Use

<!-- submilli:if capability="scrapecreators.com/reddit.search" -->

```ts
import { scrapecreators } from "@submilli/scrapecreators";

function main(): string {
    const response = scrapecreators.reddit.search({ query: "WebAssembly", timeframe: "month" });
    return JSON.stringify({ sources: response.data.posts, issues: response.issues });
}
```

<!-- submilli:endif -->

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
- `raw` is the complete JSON envelope as `unknown`, retaining future fields.
  It may contain public contact information returned by the provider.
- `issues` exposes provider errors/warnings inside successful envelopes, with
  kind, code, URL, message and raw detail. Inspect it even on HTTP 200. Providers
  may silently omit inaccessible items; an empty issues list does not guarantee
  complete coverage. Match partial failures by URL or ID, not array position.

Pagination is manual: one call returns one page, with no retries or hidden
follow-up calls. Pass native tokens back unchanged. Stop on absent/null tokens
or the endpoint's explicit end flag; do not send null tokens as strings.

<!-- submilli:if any="scrapecreators.com/github.listUserRepositories scrapecreators.com/github.listUserPullRequests scrapecreators.com/github.listUserActivity scrapecreators.com/github.listFollowers scrapecreators.com/github.listFollowing" -->

GitHub cursors are numeric.

<!-- submilli:endif -->

<!-- submilli:if any="scrapecreators.com/youtube.getPlaylist scrapecreators.com/youtube.listChannelCommunityPosts scrapecreators.com/youtube.listChannelLives scrapecreators.com/youtube.listChannelPlaylists scrapecreators.com/youtube.listChannelShorts scrapecreators.com/youtube.listChannelVideos scrapecreators.com/youtube.listCommentReplies scrapecreators.com/youtube.listComments scrapecreators.com/youtube.search scrapecreators.com/youtube.searchHashtag" -->

YouTube continuation tokens are strings.

<!-- submilli:endif -->

<!-- submilli:if any="scrapecreators.com/reddit.listSubredditPosts scrapecreators.com/reddit.search" -->

Reddit `after` tokens are strings.

<!-- submilli:endif -->

<!-- submilli:if capability="scrapecreators.com/reddit.listComments" -->

Reddit comment cursors come from `more.cursor` or a nested reply's
`more.cursor`; do not combine them. Set `usePost: true` to send long comment
cursors in JSON at the same endpoint.

<!-- submilli:endif -->

`ScrapeCreatorsError` includes `code`, `status`, `providerCode`, and raw
`retryAfter`. Local errors use status 0. Codes are `invalid_argument`,
`missing_credentials`, `invalid_request`, `unauthorized`, `forbidden`,
`quota_exceeded`, `rate_limited`, `unavailable_content`, `provider_error`,
and `invalid_response`. Permission and native transport errors pass through.
Provider HTTP-200 failures also throw.

<!-- submilli:if any="scrapecreators.com/reddit.getTranscript scrapecreators.com/youtube.getTranscript scrapecreators.com/linkedin.getTranscript scrapecreators.com/twitter.getTranscript" -->

Missing captions reported as a successful null transcript remain a typed result,
rather than an empty successful string.

<!-- submilli:endif -->

`decodeResponse(body, status?, retryAfter?)` validates recorded provider envelopes
without credentials or I/O; it returns `unknown`, not an endpoint-specific type.

## Platform limitations

<!-- submilli:if capability="scrapecreators.com/reddit.search" -->
- **Reddit:** global `search` supports `filter: "posts"` or `"comments"`.
  Timeframe and `comment_count` sorting apply only to posts.
<!-- submilli:endif -->
<!-- submilli:if any="scrapecreators.com/reddit.getSubreddit scrapecreators.com/reddit.listSubredditPosts scrapecreators.com/reddit.searchSubreddit" -->
- **Reddit:** subreddit names retain case.
<!-- submilli:endif -->
<!-- submilli:if capability="scrapecreators.com/reddit.getTranscript" -->
- **Reddit:** native video captions can be unavailable; no AI fallback is used.
<!-- submilli:endif -->
<!-- submilli:if capability="scrapecreators.com/youtube.getTranscript" -->
- **YouTube:** transcripts use public captions and preserve native segment times
  (`startMs`/`endMs` are provider strings). No AI fallback is used. `originalAudio`
  takes precedence over `language`; missing matching tracks return null.
<!-- submilli:endif -->
<!-- submilli:if any="scrapecreators.com/youtube.listChannelVideos scrapecreators.com/youtube.search" -->
- **YouTube:** `includeExtras` is opt-in and may reduce reliability.
<!-- submilli:endif -->
<!-- submilli:if capability="scrapecreators.com/youtube.getSponsors" -->
- **YouTube:** sponsor detection is an experimental inference with evidence, not an
  official sponsor attribution.
<!-- submilli:endif -->
<!-- submilli:if capability="scrapecreators.com/linkedin.searchPosts" -->
- **LinkedIn:** post search uses Google indexing and is incomplete; a date filter
  cannot guarantee exhaustive recent coverage. Search cursors range from 1–11.
<!-- submilli:endif -->
<!-- submilli:if capability="scrapecreators.com/linkedin.getPost" -->
- **LinkedIn:** there is no standalone comments endpoint in this client: post details
  retain comments the provider returns.
<!-- submilli:endif -->
<!-- submilli:if capability="scrapecreators.com/linkedin.getTranscript" -->
- **LinkedIn:** video transcripts require public native captions.
<!-- submilli:endif -->
<!-- submilli:if any="scrapecreators.com/github.getContributions scrapecreators.com/github.getRepository scrapecreators.com/github.getUser scrapecreators.com/github.listFollowers scrapecreators.com/github.listFollowing scrapecreators.com/github.listTrendingDevelopers scrapecreators.com/github.listTrendingRepositories scrapecreators.com/github.listUserActivity scrapecreators.com/github.listUserPullRequests scrapecreators.com/github.listUserRepositories" -->
- **GitHub:** this client retrieves public data through ScrapeCreators and consumes
  its credits. The separate `@submilli/github` package uses GitHub directly with
  `GITHUB_TOKEN` and supports issue/PR/code research and mutations. These clients
  do not fall back to one another.
<!-- submilli:endif -->
<!-- submilli:if capability="scrapecreators.com/github.listUserPullRequests" -->
- **GitHub:** user PR results use GitHub's public search index.
<!-- submilli:endif -->
<!-- submilli:if capability="scrapecreators.com/github.listUserActivity" -->
- **GitHub:** activity pages cover one month at a time.
<!-- submilli:endif -->
<!-- submilli:if capability="scrapecreators.com/twitter.searchPosts" -->
- **Twitter/X:** post search uses Google indexing, is not native X search, may miss
  recent posts, and can repeat posts across pages. Search cursors range from 1–11.
<!-- submilli:endif -->
<!-- submilli:if any="scrapecreators.com/twitter.listUserTweets scrapecreators.com/twitter.listCommunityTweets" -->
- **Twitter/X:** user/community tweet endpoints do not advertise pagination.
<!-- submilli:endif -->
<!-- submilli:if capability="scrapecreators.com/twitter.getTweet" -->
- **Twitter/X:** there is no standalone replies endpoint: only returned tweet details
  are available.
<!-- submilli:endif -->
<!-- submilli:if capability="scrapecreators.com/twitter.getTranscript" -->
- **Twitter/X:** video transcription uses AI. Default successful transcripts up to two
  minutes cost one credit; `allowLongTranscription: true` explicitly opts into
  pricing of ten credits per started minute, including short videos. It is never
  enabled automatically. Null transcripts cost no transcription credits.
<!-- submilli:endif -->

All platforms depend on public availability and provider coverage. Preserve
unknown dates and unavailable metrics instead of fabricating recency or zero.
Calls use the native HTTP timeout (30 seconds); slow provider scrapes or long AI
transcriptions may exceed it. No automatic media downloads or paid fallback runs.
See the [provider introduction](https://docs.scrapecreators.com/introduction/)
and [current endpoint catalog](https://docs.scrapecreators.com/openapi.json).

<!-- submilli:if any="scrapecreators.com/youtube.getTranscript scrapecreators.com/linkedin.searchPosts scrapecreators.com/github.listUserPullRequests scrapecreators.com/twitter.searchPosts" -->

## Platform examples

<!-- submilli:if capability="scrapecreators.com/youtube.getTranscript" -->

```ts
import { youtube } from "@submilli/scrapecreators";

function main(): string {
    return JSON.stringify(youtube.getTranscript({ url: "https://www.youtube.com/watch?v=dQw4w9WgXcQ", language: "en" }).data);
}
```

<!-- submilli:endif -->

<!-- submilli:if capability="scrapecreators.com/linkedin.searchPosts" -->

```ts
import { linkedin } from "@submilli/scrapecreators";

function main(): string {
    return JSON.stringify(linkedin.searchPosts({ query: "WebAssembly", datePosted: "last-month" }).data);
}
```

<!-- submilli:endif -->

<!-- submilli:if capability="scrapecreators.com/github.listUserPullRequests" -->

```ts
import { github } from "@submilli/scrapecreators";

function main(): string {
    return JSON.stringify(github.listUserPullRequests({ handle: "octocat", since: "2026-01-01" }).data);
}
```

<!-- submilli:endif -->

<!-- submilli:if capability="scrapecreators.com/twitter.searchPosts" -->

```ts
import { twitter } from "@submilli/scrapecreators";

function main(): string {
    return JSON.stringify(twitter.searchPosts({ query: "WebAssembly" }).data);
}
```

<!-- submilli:endif -->

<!-- submilli:endif -->

<!-- submilli:if all="scrapecreators.com/reddit.search scrapecreators.com/reddit.listComments scrapecreators.com/linkedin.searchPosts scrapecreators.com/twitter.searchPosts" -->

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
            const id = posts[0].id;
            if (id !== undefined && id !== null) {
                try { sources.push(reddit.listComments({ url: "https://www.reddit.com/comments/" + id })); }
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

<!-- submilli:endif -->

## Operation reference

All parameters are camelCase options. `!` marks required fields; other options
are omitted by default. Provider response fields retain their native spelling.
Follow each endpoint link for value ranges, defaults, pagination and credit costs.

<!-- submilli:if any="scrapecreators.com/reddit.getPost scrapecreators.com/reddit.getSubreddit scrapecreators.com/reddit.getTranscript scrapecreators.com/reddit.listComments scrapecreators.com/reddit.listSubredditPosts scrapecreators.com/reddit.search scrapecreators.com/reddit.searchSubreddit" -->

### reddit

| Method and capability suffix | Options | Provider endpoint |
|---|---|---|
<!-- submilli:if capability="scrapecreators.com/reddit.getSubreddit" -->
| `reddit.getSubreddit` | `subreddit`, `url`, `cacheMaxAge` | [/v1/reddit/subreddit/details](https://docs.scrapecreators.com/v1/reddit/subreddit/details/) |
<!-- submilli:endif -->
<!-- submilli:if capability="scrapecreators.com/reddit.listSubredditPosts" -->
| `reddit.listSubredditPosts` | `subreddit!`, `timeframe`, `sort`, `after`, `trim`, `cacheMaxAge` | [/v1/reddit/subreddit](https://docs.scrapecreators.com/v1/reddit/subreddit/) |
<!-- submilli:endif -->
<!-- submilli:if capability="scrapecreators.com/reddit.searchSubreddit" -->
| `reddit.searchSubreddit` | `subreddit!`, `query`, `sort`, `timeframe`, `cursor` | [/v1/reddit/subreddit/search](https://docs.scrapecreators.com/v1/reddit/subreddit/search/) |
<!-- submilli:endif -->
<!-- submilli:if capability="scrapecreators.com/reddit.getPost" -->
| `reddit.getPost` | `url!` | [/v1/reddit/post](https://docs.scrapecreators.com/v1/reddit/post/) |
<!-- submilli:endif -->
<!-- submilli:if capability="scrapecreators.com/reddit.listComments" -->
| `reddit.listComments` | `url!`, `cursor`, `trim`, `usePost` | [/v1/reddit/post/comments](https://docs.scrapecreators.com/v1/reddit/post/comments/) |
<!-- submilli:endif -->
<!-- submilli:if capability="scrapecreators.com/reddit.getTranscript" -->
| `reddit.getTranscript` | `url!`, `language`, `cacheMaxAge` | [/v1/reddit/post/transcript](https://docs.scrapecreators.com/v1/reddit/post/transcript/) |
<!-- submilli:endif -->
<!-- submilli:if capability="scrapecreators.com/reddit.search" -->
| `reddit.search` | `query!`, `filter`, `sort`, `timeframe`, `after`, `trim` | [/v1/reddit/search](https://docs.scrapecreators.com/v1/reddit/search/) |
<!-- submilli:endif -->

<!-- submilli:endif -->

<!-- submilli:if any="scrapecreators.com/youtube.getChannel scrapecreators.com/youtube.getCommunityPost scrapecreators.com/youtube.getPlaylist scrapecreators.com/youtube.getSponsors scrapecreators.com/youtube.getTranscript scrapecreators.com/youtube.getVideo scrapecreators.com/youtube.listChannelCommunityPosts scrapecreators.com/youtube.listChannelLives scrapecreators.com/youtube.listChannelPlaylists scrapecreators.com/youtube.listChannelShorts scrapecreators.com/youtube.listChannelVideos scrapecreators.com/youtube.listCommentReplies scrapecreators.com/youtube.listComments scrapecreators.com/youtube.listTrendingShorts scrapecreators.com/youtube.search scrapecreators.com/youtube.searchHashtag scrapecreators.com/youtube.searchTypeahead" -->

### youtube

| Method and capability suffix | Options | Provider endpoint |
|---|---|---|
<!-- submilli:if capability="scrapecreators.com/youtube.getChannel" -->
| `youtube.getChannel` | `channelId`, `handle`, `url`, `cacheMaxAge` | [/v1/youtube/channel](https://docs.scrapecreators.com/v1/youtube/channel/) |
<!-- submilli:endif -->
<!-- submilli:if capability="scrapecreators.com/youtube.listChannelVideos" -->
| `youtube.listChannelVideos` | `channelId`, `handle`, `sort`, `continuationToken`, `isPaidPromotions`, `includeExtras` | [/v1/youtube/channel-videos](https://docs.scrapecreators.com/v1/youtube/channel-videos/) |
<!-- submilli:endif -->
<!-- submilli:if capability="scrapecreators.com/youtube.listChannelPlaylists" -->
| `youtube.listChannelPlaylists` | `channelId`, `handle`, `continuationToken` | [/v1/youtube/channel/playlists](https://docs.scrapecreators.com/v1/youtube/channel/playlists/) |
<!-- submilli:endif -->
<!-- submilli:if capability="scrapecreators.com/youtube.listChannelLives" -->
| `youtube.listChannelLives` | `channelId`, `handle`, `continuationToken` | [/v1/youtube/channel/lives](https://docs.scrapecreators.com/v1/youtube/channel/lives/) |
<!-- submilli:endif -->
<!-- submilli:if capability="scrapecreators.com/youtube.listChannelCommunityPosts" -->
| `youtube.listChannelCommunityPosts` | `channelId`, `handle`, `continuationToken` | [/v1/youtube/channel/community-posts](https://docs.scrapecreators.com/v1/youtube/channel/community-posts/) |
<!-- submilli:endif -->
<!-- submilli:if capability="scrapecreators.com/youtube.listChannelShorts" -->
| `youtube.listChannelShorts` | `handle`, `channelId`, `sort`, `continuationToken` | [/v1/youtube/channel/shorts](https://docs.scrapecreators.com/v1/youtube/channel/shorts/) |
<!-- submilli:endif -->
<!-- submilli:if capability="scrapecreators.com/youtube.getVideo" -->
| `youtube.getVideo` | `url!`, `language`, `cacheMaxAge` | [/v1/youtube/video](https://docs.scrapecreators.com/v1/youtube/video/) |
<!-- submilli:endif -->
<!-- submilli:if capability="scrapecreators.com/youtube.getTranscript" -->
| `youtube.getTranscript` | `url!`, `language`, `originalAudio`, `cacheMaxAge` | [/v1/youtube/video/transcript](https://docs.scrapecreators.com/v1/youtube/video/transcript/) |
<!-- submilli:endif -->
<!-- submilli:if capability="scrapecreators.com/youtube.getSponsors" -->
| `youtube.getSponsors` | `url!`, `language` | [/v1/youtube/video/sponsors](https://docs.scrapecreators.com/v1/youtube/video/sponsors/) |
<!-- submilli:endif -->
<!-- submilli:if capability="scrapecreators.com/youtube.search" -->
| `youtube.search` | `query!`, `uploadDate`, `sortBy`, `type`, `duration`, `region`, `continuationToken`, `includeExtras` | [/v1/youtube/search](https://docs.scrapecreators.com/v1/youtube/search/) |
<!-- submilli:endif -->
<!-- submilli:if capability="scrapecreators.com/youtube.searchTypeahead" -->
| `youtube.searchTypeahead` | `query!` | [/v1/youtube/search/typeahead](https://docs.scrapecreators.com/v1/youtube/search/typeahead/) |
<!-- submilli:endif -->
<!-- submilli:if capability="scrapecreators.com/youtube.searchHashtag" -->
| `youtube.searchHashtag` | `hashtag!`, `continuationToken`, `type` | [/v1/youtube/search/hashtag](https://docs.scrapecreators.com/v1/youtube/search/hashtag/) |
<!-- submilli:endif -->
<!-- submilli:if capability="scrapecreators.com/youtube.listComments" -->
| `youtube.listComments` | `url!`, `continuationToken`, `order` | [/v1/youtube/video/comments](https://docs.scrapecreators.com/v1/youtube/video/comments/) |
<!-- submilli:endif -->
<!-- submilli:if capability="scrapecreators.com/youtube.listCommentReplies" -->
| `youtube.listCommentReplies` | `continuationToken!` | [/v1/youtube/video/comment/replies](https://docs.scrapecreators.com/v1/youtube/video/comment/replies/) |
<!-- submilli:endif -->
<!-- submilli:if capability="scrapecreators.com/youtube.listTrendingShorts" -->
| `youtube.listTrendingShorts` | None | [/v1/youtube/shorts/trending](https://docs.scrapecreators.com/v1/youtube/shorts/trending/) |
<!-- submilli:endif -->
<!-- submilli:if capability="scrapecreators.com/youtube.getPlaylist" -->
| `youtube.getPlaylist` | `playlistId!`, `continuationToken` | [/v1/youtube/playlist](https://docs.scrapecreators.com/v1/youtube/playlist/) |
<!-- submilli:endif -->
<!-- submilli:if capability="scrapecreators.com/youtube.getCommunityPost" -->
| `youtube.getCommunityPost` | `url!` | [/v1/youtube/community-post](https://docs.scrapecreators.com/v1/youtube/community-post/) |
<!-- submilli:endif -->

<!-- submilli:endif -->

<!-- submilli:if any="scrapecreators.com/linkedin.getCompany scrapecreators.com/linkedin.getPost scrapecreators.com/linkedin.getProfile scrapecreators.com/linkedin.getTranscript scrapecreators.com/linkedin.listCompanyPosts scrapecreators.com/linkedin.searchPosts" -->

### linkedin

| Method and capability suffix | Options | Provider endpoint |
|---|---|---|
<!-- submilli:if capability="scrapecreators.com/linkedin.getProfile" -->
| `linkedin.getProfile` | `url!` | [/v1/linkedin/profile](https://docs.scrapecreators.com/v1/linkedin/profile/) |
<!-- submilli:endif -->
<!-- submilli:if capability="scrapecreators.com/linkedin.getCompany" -->
| `linkedin.getCompany` | `url!` | [/v1/linkedin/company](https://docs.scrapecreators.com/v1/linkedin/company/) |
<!-- submilli:endif -->
<!-- submilli:if capability="scrapecreators.com/linkedin.listCompanyPosts" -->
| `linkedin.listCompanyPosts` | `url!`, `page` | [/v1/linkedin/company/posts](https://docs.scrapecreators.com/v1/linkedin/company/posts/) |
<!-- submilli:endif -->
<!-- submilli:if capability="scrapecreators.com/linkedin.searchPosts" -->
| `linkedin.searchPosts` | `query!`, `datePosted`, `cursor` | [/v1/linkedin/search/posts](https://docs.scrapecreators.com/v1/linkedin/search/posts/) |
<!-- submilli:endif -->
<!-- submilli:if capability="scrapecreators.com/linkedin.getPost" -->
| `linkedin.getPost` | `url!` | [/v1/linkedin/post](https://docs.scrapecreators.com/v1/linkedin/post/) |
<!-- submilli:endif -->
<!-- submilli:if capability="scrapecreators.com/linkedin.getTranscript" -->
| `linkedin.getTranscript` | `url!` | [/v1/linkedin/post/transcript](https://docs.scrapecreators.com/v1/linkedin/post/transcript/) |
<!-- submilli:endif -->

<!-- submilli:endif -->

<!-- submilli:if any="scrapecreators.com/github.getContributions scrapecreators.com/github.getRepository scrapecreators.com/github.getUser scrapecreators.com/github.listFollowers scrapecreators.com/github.listFollowing scrapecreators.com/github.listTrendingDevelopers scrapecreators.com/github.listTrendingRepositories scrapecreators.com/github.listUserActivity scrapecreators.com/github.listUserPullRequests scrapecreators.com/github.listUserRepositories" -->

### github

| Method and capability suffix | Options | Provider endpoint |
|---|---|---|
<!-- submilli:if capability="scrapecreators.com/github.getUser" -->
| `github.getUser` | `handle`, `url` | [/v1/github/user](https://docs.scrapecreators.com/v1/github/user/) |
<!-- submilli:endif -->
<!-- submilli:if capability="scrapecreators.com/github.listUserRepositories" -->
| `github.listUserRepositories` | `handle`, `url`, `type`, `sort`, `direction`, `cursor` | [/v1/github/user/repositories](https://docs.scrapecreators.com/v1/github/user/repositories/) |
<!-- submilli:endif -->
<!-- submilli:if capability="scrapecreators.com/github.listUserPullRequests" -->
| `github.listUserPullRequests` | `handle!`, `since`, `until`, `cursor` | [/v1/github/user/pull-requests](https://docs.scrapecreators.com/v1/github/user/pull-requests/) |
<!-- submilli:endif -->
<!-- submilli:if capability="scrapecreators.com/github.listUserActivity" -->
| `github.listUserActivity` | `handle`, `url`, `year`, `cursor` | [/v1/github/user/activity](https://docs.scrapecreators.com/v1/github/user/activity/) |
<!-- submilli:endif -->
<!-- submilli:if capability="scrapecreators.com/github.listFollowers" -->
| `github.listFollowers` | `handle`, `url`, `cursor` | [/v1/github/user/followers](https://docs.scrapecreators.com/v1/github/user/followers/) |
<!-- submilli:endif -->
<!-- submilli:if capability="scrapecreators.com/github.listFollowing" -->
| `github.listFollowing` | `handle`, `url`, `cursor` | [/v1/github/user/following](https://docs.scrapecreators.com/v1/github/user/following/) |
<!-- submilli:endif -->
<!-- submilli:if capability="scrapecreators.com/github.getContributions" -->
| `github.getContributions` | `handle`, `url`, `year` | [/v1/github/user/contributions](https://docs.scrapecreators.com/v1/github/user/contributions/) |
<!-- submilli:endif -->
<!-- submilli:if capability="scrapecreators.com/github.getRepository" -->
| `github.getRepository` | `url!` | [/v1/github/repository](https://docs.scrapecreators.com/v1/github/repository/) |
<!-- submilli:endif -->
<!-- submilli:if capability="scrapecreators.com/github.listTrendingRepositories" -->
| `github.listTrendingRepositories` | `language`, `since`, `spokenLanguageCode` | [/v1/github/trending/repositories](https://docs.scrapecreators.com/v1/github/trending/repositories/) |
<!-- submilli:endif -->
<!-- submilli:if capability="scrapecreators.com/github.listTrendingDevelopers" -->
| `github.listTrendingDevelopers` | `language`, `since` | [/v1/github/trending/developers](https://docs.scrapecreators.com/v1/github/trending/developers/) |
<!-- submilli:endif -->

<!-- submilli:endif -->

<!-- submilli:if any="scrapecreators.com/twitter.getCommunity scrapecreators.com/twitter.getProfile scrapecreators.com/twitter.getTranscript scrapecreators.com/twitter.getTweet scrapecreators.com/twitter.listCommunityTweets scrapecreators.com/twitter.listUserTweets scrapecreators.com/twitter.searchPosts" -->

### twitter

| Method and capability suffix | Options | Provider endpoint |
|---|---|---|
<!-- submilli:if capability="scrapecreators.com/twitter.searchPosts" -->
| `twitter.searchPosts` | `query!`, `cursor` | [/v1/twitter/search/posts](https://docs.scrapecreators.com/v1/twitter/search/posts/) |
<!-- submilli:endif -->
<!-- submilli:if capability="scrapecreators.com/twitter.getProfile" -->
| `twitter.getProfile` | `handle!`, `cacheMaxAge` | [/v1/twitter/profile](https://docs.scrapecreators.com/v1/twitter/profile/) |
<!-- submilli:endif -->
<!-- submilli:if capability="scrapecreators.com/twitter.listUserTweets" -->
| `twitter.listUserTweets` | `handle!`, `trim` | [/v1/twitter/user-tweets](https://docs.scrapecreators.com/v1/twitter/user-tweets/) |
<!-- submilli:endif -->
<!-- submilli:if capability="scrapecreators.com/twitter.getTweet" -->
| `twitter.getTweet` | `url!`, `trim`, `cacheMaxAge` | [/v1/twitter/tweet](https://docs.scrapecreators.com/v1/twitter/tweet/) |
<!-- submilli:endif -->
<!-- submilli:if capability="scrapecreators.com/twitter.getTranscript" -->
| `twitter.getTranscript` | `allowLongTranscription`, `url!`, `cacheMaxAge` | [/v1/twitter/tweet/transcript](https://docs.scrapecreators.com/v1/twitter/tweet/transcript/) |
<!-- submilli:endif -->
<!-- submilli:if capability="scrapecreators.com/twitter.getCommunity" -->
| `twitter.getCommunity` | `url!` | [/v1/twitter/community](https://docs.scrapecreators.com/v1/twitter/community/) |
<!-- submilli:endif -->
<!-- submilli:if capability="scrapecreators.com/twitter.listCommunityTweets" -->
| `twitter.listCommunityTweets` | `url!` | [/v1/twitter/community/tweets](https://docs.scrapecreators.com/v1/twitter/community/tweets/) |
<!-- submilli:endif -->

<!-- submilli:endif -->
