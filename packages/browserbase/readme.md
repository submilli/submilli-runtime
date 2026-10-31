# @submilli/browserbase

Browserbase Search and Fetch, plus hosted Stagehand `act`, `extract`, and `observe`
on a cloud browser. Submilli cannot run the Stagehand Node SDK or a CDP socket, so
browser actions go through the Stagehand session API. Connection URLs and signing
keys are never returned. The API key identifies the project; this package never
sends or asks for a project id.

## Install and bind the key

In an existing blueprint directory:

```sh
submilli install submilli/submilli-runtime @submilli/browserbase
submilli blueprint add-package @submilli/browserbase --no-capabilities
submilli blueprint secret add BROWSERBASE_API_KEY --store browserbase_api_key
submilli secret put browserbase_api_key
submilli blueprint capability add browserbase.com/search
submilli blueprint capability add browserbase.com/fetch
```

Enter the key from [Browserbase settings](https://browserbase.com/settings) at the
hidden prompt. Programs never accept credentials as arguments. A harness can bind
the same name:

```yaml
secrets:
  BROWSERBASE_API_KEY:
    harness:
      required: true
```

`add-package` grants the package its derived HTTP and secret permissions. Caller
operation grants remain separate. Leave any model provider key unset. Stagehand
inference uses Model Gateway on the Browserbase key. A placeholder provider key
is not accepted by this package. The free plan includes a limited allowance of
Search, Fetch, and browser time, plus $5 of Model Gateway tokens. Proxies,
Verified, and CAPTCHA solving are separate grants and are not on the free plan.

## Authority

| Operation | Caller capability and truthful fields |
| --- | --- |
| Search | `browserbase.com/search { numResults, provider, mode }` |
| Fetch | `browserbase.com/fetch { host, proxies, allowRedirects, allowInsecureSsl, contextId }` |
| Fetch with redirects, and Stagehand navigation | Also `browserbase.com/delegatedFetch {}` |
| `StagehandSession.open` | `browserbase.com/stagehand.start { model, proxies, verified, solveCaptchas, keepAlive, region, timeoutSeconds, contextId, persistContext }` |
| `StagehandSession.current` when a stored browser is alive | `browserbase.com/stagehand.current { sessionId }` |
| `session.goto` | `browserbase.com/stagehand.navigate { sessionId, host }` |
| `session.act` and `session.replay` | `browserbase.com/stagehand.act { sessionId }` |
| `session.observe` | `browserbase.com/stagehand.observe { sessionId }` |
| `session.extract` | `browserbase.com/stagehand.extract { sessionId }` |
| `session.end` | `browserbase.com/stagehand.end { sessionId }` |
| Read a session or its live view | `browserbase.com/sessions.read { sessionId }` |
| List sessions | `browserbase.com/sessions.list {}` |
| Release a session | `browserbase.com/sessions.release { sessionId }` |

A host grant authorizes the submitted URL, not redirects or subresources. Do not
grant `delegatedFetch` to a role that needs a hard downstream host boundary.
Proxies, Verified, and `solveCaptchas` default to false and are sent explicitly,
because the Browserbase session API would otherwise enable CAPTCHA solving.
Custom proxy URLs and passwords are not accepted. Session timeout defaults to
300 seconds so a grant can cap it; the project default is not used.

`StagehandSession.open` and `StagehandSession.current` register cleanup.
`current` stores the id and region in the runtime session under `stagehand` and
reuses that browser while it is still pending or running. The first call also
needs `stagehand.start`. A non-default region uses its own Stagehand host
(`api.use1`, `api.euc1`, or `api.apse1`); `end` and session close use that same
host. `session.liveView` returns the watch link. The CDP URL is dropped because
it embeds a signing key. There is no public raw-session create: a browser you
cannot drive is not a useful handle.

## Development

From the repository root:

```bash
submilli build test -p @submilli/browserbase --skip-network
```

Set `BROWSERBASE_API_KEY` to exercise the network tests. They call Search and
Fetch only. Stagehand calls spend browser time and Model Gateway tokens; they
are not in the default network tests.
