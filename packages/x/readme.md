# @submilli/x

Read-only official X API v2 client for public tweet lookup, recent/full-archive
search, and conversation replies. Returns structured source data with authors,
timestamps, public metrics, reply relationships, and explicit pagination.

## Credentials and setup

Use the **Bearer Token** from your X developer app, bound as `X_BEARER_TOKEN`.
Consumer/API key and consumer secret are not needed when the app bearer token
is already available. This package uses app-only public reads and no user login.
Your app also needs access and credits for the endpoints it calls. An xAI key is
separate and cannot be used here.

```sh
submilli install submilli/submilli-runtime @submilli/x
submilli blueprint add-package @submilli/x --no-capabilities
submilli blueprint secret add X_BEARER_TOKEN --store x_bearer_token
submilli secret put x_bearer_token
submilli blueprint capability add x.com/getTweet
submilli blueprint capability add x.com/searchRecent
submilli blueprint capability add x.com/searchAll
```

Run blueprint commands beside an existing blueprint; `secret put` prompts for
the token. Grant only the operations needed. For application-supplied credentials:

```yaml
secrets:
  X_BEARER_TOKEN:
    harness:
      required: true
```

The package requires HTTP GET to `api.x.com` and secret access; its only request
paths are `/2/tweets/:id`, `/2/tweets/search/recent`, and `/2/tweets/search/all`.
The package capability catalog is [capabilities.yaml](capabilities.yaml).

## Use

```ts
import x from "@submilli/x";

function main(): string {
    const page = x.searchRecent("WebAssembly -is:retweet", { maxResults: 10 });
    return JSON.stringify(page);
}
```

`getTweet(idOrUrl)` returns `{ tweet, issues }`. `searchRecent`, `searchAll`, and
`listReplies` return `{ tweets, nextToken, resultCount, issues }`. Missing fields
stay null and partial failures remain visible in `issues`. There is no raw
response field and no model-generated content. Named imports and the default
package namespace both work.

Recent search covers seven days; archive search needs the corresponding API
access. `listReplies` takes the conversation root ID and includes nested replies;
use `{ archive: true }` for older replies. Pagination is always explicit. Each
read, including author expansion, may consume credits. There are no writes or
automatic retries. See [agent documentation](docs/readme.md) for all types,
options, capabilities, error handling, and completeness limits.

## Development

```sh
cargo run -p submilli -- build test --skip-network -p @submilli/x
cargo run -p submilli -- build test -p @submilli/x --env-file .env
```

The first command runs offline contracts and compiles documentation examples.
The second also performs four bounded paid read requests when `X_BEARER_TOKEN`
is supplied: recent search, lookup, conversation replies, and archive search.
Without the token the live test explicitly skips. Insufficient credits or
archive access fail the live test rather than being reported as a pass.

Policy fixtures can be run after `submilli build publish-local -p @submilli/x`:
`submilli run packages/x/tests/policy/reads.ts --blueprint packages/x/tests/policy/reads.yaml`.
They require no token or network and verify caller attribution, normalized tweet
IDs, and separate recent/archive grants.

References: [app-only authentication](https://docs.x.com/fundamentals/authentication/oauth-2-0/application-only),
[lookup](https://docs.x.com/x-api/posts/lookup/introduction),
[search](https://docs.x.com/x-api/posts/search/introduction),
[conversation IDs](https://docs.x.com/x-api/fundamentals/conversation-id),
and [pricing](https://docs.x.com/x-api/getting-started/pricing).
