# @submilli/exa

A synchronous, typed Exa client for agent web search and known-URL extraction.
Uses native HTTP through Submilli; no Node SDK or promises are needed.

## Install and bind the key

In an existing blueprint directory:

```sh
submilli install submilli/submilli-runtime @submilli/exa
submilli blueprint add-package @submilli/exa --no-capabilities
submilli blueprint secret add EXA_API_KEY --store exa_api_key
submilli secret put exa_api_key
submilli blueprint capability add exa.ai/search
submilli blueprint capability add exa.ai/contents
```

Enter the key from the [Exa dashboard](https://dashboard.exa.ai) at the hidden
prompt. Programs never accept credentials as arguments. To supply it from a
trusted harness instead, use a required harness binding:

```yaml
secrets:
  EXA_API_KEY:
    harness:
      required: true
```

`add-package` installs the package's derived HTTP and secret permissions. Its
HTTP requirements are restricted to `api.exa.ai`, `POST /search`, and
`POST /contents`. Caller permissions are separate. Restrict known-URL reads
with a filter such as `host == "exa.ai"` on `exa.ai/contents`; every URL in a
batch must be allowed before the package reads the key or sends HTTP.
The host filter applies to requested URLs; Exa may follow redirects remotely.

## Use

```ts
import { search, getContents } from "@submilli/exa";

function main(): string {
    const found = search("WebAssembly garbage collection design");
    if (found.results.length === 0) return "No results";
    const pages = getContents([found.results[0].url]);
    return JSON.stringify(pages);
}
```

Defaults follow Exa's build-with-exa skill: auto search with highlights, without
extra filters, count, or freshness overrides. Full text is available when
explicitly requested. Results preserve source URLs; extraction responses expose
per-URL failures even on HTTP 200. See [agent documentation](docs/readme.md)
for options and error handling.

API references: [Search](https://exa.ai/docs/reference/search),
[Contents](https://exa.ai/docs/reference/get-contents).
Guidance: [build-with-exa](https://github.com/exa-labs/agent-skills).

## Development

```sh
cargo run -p submilli -- build test -p @submilli/exa
```

The package test runner reads `EXA_API_KEY` from the process environment or the
repository-root `.env` file. Keep the file untracked. Live tests skip explicitly
when the key is missing or blank; with a key they make one search and one known-URL
request and consume provider credits. Offline tests do not require a key.
