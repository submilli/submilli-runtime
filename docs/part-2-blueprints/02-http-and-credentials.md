---
title: "HTTP and credentials"
description: "How to let programs call an HTTP endpoint that has no Package, give that endpoint a credential the program never sees, and prove it arrived."
slug: blueprints/http-and-credentials
sidebar:
  order: 2
authorship:
  label: ai-assisted
  confirmed: true
  contentHash: "7f932d9b7938d18ee629a82cf8b4157876060db4759c836544464b0853749657"
  confirmedAt: "2026-10-05T10:59:51.490Z"
---

This guide shows you how to let programs call an HTTP endpoint that has no
Package, and how to give that endpoint a credential the program never
sees. The example is GitHub's REST API with a personal access token.
Substitute your host and its authentication.

## Authorization proxy

The program can't hold the credential itself. The model can be talked into
repeating anything generated code can read, so `secrets.get` is refused
from `main` whatever the Blueprint says. The Blueprint names the secret and
the host, and the **authorization proxy** adds the credential to each
matching request outside the program. The program sends a plain request and
sees the response, but never the header.

## Allow the request

`submilli:http` has one capability per verb, plus one for downloads, and
`http.<method>` for any other method `http.request` sends. List
them, with the fields a filter can test and the rules the Blueprint already
has for each:

```sh
submilli blueprint capability list submilli:http
```

```text
submilli:http
  http.get — HTTP GET
      fields: host: string, path: string, body_size: number, timeout_ms: number
      example filter: host == "api.example.com"
  http.post — HTTP POST
      fields: host: string, path: string, body_size: number, timeout_ms: number
      example filter: host == "api.example.com"
  http.put — HTTP PUT
      fields: host: string, path: string, body_size: number, timeout_ms: number
      example filter: host == "api.example.com"
  http.patch — HTTP PATCH
      fields: host: string, path: string, body_size: number, timeout_ms: number
      example filter: host == "api.example.com"
  http.delete — HTTP DELETE
      fields: host: string, path: string, body_size: number, timeout_ms: number
      example filter: host == "api.example.com"
  http.head — HTTP HEAD
      fields: host: string, path: string, body_size: number, timeout_ms: number
      example filter: host == "api.example.com"
  http.options — HTTP OPTIONS
      fields: host: string, path: string, body_size: number, timeout_ms: number
      example filter: host == "api.example.com"
  http.download — Download a URL straight to the VFS
      fields: host: string, url_path: string, vfs_path: string, max_bytes: number, overwrite: boolean, decompress: boolean
      example filter: host == "cdn.example.com" and overwrite == false
  http.<method> — Any other HTTP method, through `http.request`: `http.trace` gates TRACE
      fields: host: string, path: string, body_size: number, timeout_ms: number
      example filter: host == "api.example.com"
```

Grant the verb the program needs, narrowed to the host:

```sh
submilli blueprint capability add http.get --filter 'host == "api.github.com"'
```

```text
✓ added allow http.get (filter: host == "api.github.com") to caller 'main' in blueprint.yaml
  HTTP GET
  filter fields: host: string, path: string, body_size: number, timeout_ms: number
```

This allows GET alone. If the program also posts, add an `http.post` rule.
Each redirect is checked before it is sent, under the same rules.

## Run a program

Create `rate.ts`. It asks GitHub how many requests the caller has left
this hour, which GitHub answers differently for an anonymous caller and
for one with a token:

```typescript title="rate.ts"
import * as http from "submilli:http";

interface RateLimit {
    resources: { core: { limit: number; remaining: number } };
}

function main(): string {
    const headers = new Map<string, string>();
    headers.set("User-Agent", "submilli");
    const response = http.get("https://api.github.com/rate_limit", headers);
    response.throwForStatus();
    const core = (response.json() as RateLimit).resources.core;
    return `${core.remaining} of ${core.limit} requests left this hour`;
}
```

```sh
submilli run --blueprint blueprint.yaml rate.ts
```

```text
51 of 60 requests left this hour
```

The request went through as an anonymous caller, and GitHub allows sixty
of those an hour. GitHub requires the `User-Agent` header and answers 403
without one.

## Add the credential

Declare the token, store its value, and tell the proxy to add it to
requests for that host:

```sh
submilli blueprint secret add GITHUB_TOKEN --store github_token
submilli secret put github_token
submilli blueprint auth-proxy add --host api.github.com --bearer GITHUB_TOKEN
```

```text
✓ declared secret 'GITHUB_TOKEN' (store: github_token) in blueprint.yaml
Value for 'github_token': [hidden]
Stored secret 'github_token'
✓ added auth_proxy rule for host 'api.github.com' (bearer auth) in blueprint.yaml
```

```yaml title="blueprint.yaml (fragment)"
secrets:
  GITHUB_TOKEN:
    store: github_token
auth_proxy:
- host: api.github.com
  auth:
    bearer: GITHUB_TOKEN
permissions:
  main:
  - capability: http.get
    filter: host == "api.github.com"
    action: allow
```

Run the same program again, unchanged:

```sh
submilli run --blueprint blueprint.yaml rate.ts
```

```text
5000 of 5000 requests left this hour
```

GitHub now sees the token, and the program never did.

An entry applies to requests whose host matches it exactly. If the
endpoint wants basic auth, use `--basic-username` and `--basic-password`.
For anything else, use `--header 'Name: value'` or `--query`, with
`${secrets.NAME}` in the value. With a credential injected, a redirect may
go only to the same scheme, host, and port.

## Register it on a server

The server has its own secret store. Put the token there before you
register the Blueprint, because registration checks that every `store:`
secret exists:

```sh
submilli server secret put github_token
submilli server blueprint apply blueprint.yaml
```

```text
Value for 'github_token': [hidden]
Stored secret 'github_token'
Added blueprint 'support'
```

Then run the program there, the way an application would:

```sh
submilli server run-code rate.ts --blueprint support
```

```text
5000 of 5000 requests left this hour
```

## If the endpoint is plain HTTP

Blueprints require HTTPS. For a host that speaks only HTTP, such as an
internal service, opt in twice. Set the flag on the proxy entry for that
host and at the top level of the file. The top-level flag has no command:

```sh
submilli blueprint secret add LEGACY_TOKEN --store legacy_token
submilli blueprint capability add http.get --filter 'host == "legacy.internal.acme.com"'
submilli blueprint auth-proxy add --host legacy.internal.acme.com --bearer LEGACY_TOKEN --allow-insecure-http
```

```text
✓ declared secret 'LEGACY_TOKEN' (store: legacy_token) in blueprint.yaml
✓ added allow http.get (filter: host == "legacy.internal.acme.com") to caller 'main' in blueprint.yaml
  HTTP GET
  filter fields: host: string, path: string, body_size: number, timeout_ms: number
✓ added auth_proxy rule for host 'legacy.internal.acme.com' (bearer auth) in blueprint.yaml
```

```yaml title="blueprint.yaml (fragment)"
allow_insecure_http: true
auth_proxy:
- host: legacy.internal.acme.com
  allow_insecure_http: true
  auth:
    bearer: LEGACY_TOKEN
```

Both flags default to `false`. The top-level one covers all HTTP calls
the Blueprint allows, including a Package's. The entry's one covers the
credential.
