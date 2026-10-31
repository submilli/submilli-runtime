# @submilli/browserbase-stagehand

Hosted Stagehand `act`, `extract`, and `observe` on a cloud browser. Submilli
cannot run the Stagehand Node SDK or a CDP socket, so browser actions go through
the Stagehand session API. Connection URLs and signing keys are never returned.
The API key identifies the project; this package never sends or asks for a
project id.

## Install and bind the key

In an existing blueprint directory:

```sh
submilli install submilli/submilli-runtime @submilli/browserbase-stagehand
submilli blueprint add-package @submilli/browserbase-stagehand --no-capabilities
submilli blueprint secret add BROWSERBASE_API_KEY --store browserbase_api_key
submilli secret put browserbase_api_key
submilli blueprint capability add browserbase.com/stagehand
submilli blueprint capability add browserbase.com/navigate
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
browser time, plus $5 of Model Gateway tokens. Proxies, Verified, and CAPTCHA
solving are separate grants and are not on the free plan.

## Authority

| Operation | Caller capability and truthful fields |
| --- | --- |
| `open`, `current`, `act`, `replay`, `observe`, `extract`, `end`, `liveView` | `browserbase.com/stagehand { sessionId }` |
| `session.goto` | `browserbase.com/navigate { sessionId, host, path, protocol }` |

`open` checks `stagehand` with an empty `sessionId`, because the browser does
not exist yet. Later calls check the id. A host and path grant covers the
submitted URL, not redirects or subresources. The query is not in the check.
Proxies, Verified, and `solveCaptchas` default to false and are sent explicitly,
because the Browserbase session API would otherwise enable CAPTCHA solving.
Custom proxy URLs and passwords are not accepted. Session timeout defaults to
300 seconds so a grant can cap it; the project default is not used.

`StagehandSession.open` and `StagehandSession.current` register cleanup.
`current` stores the id and region in the runtime session under `stagehand` and
reuses that browser while it is still pending or running. The first call also
needs `stagehand`. A non-default region uses its own Stagehand host
(`api.use1`, `api.euc1`, or `api.apse1`); `end` and session close use that same
host. `session.liveView` returns the watch link. The CDP URL is dropped because
it embeds a signing key.

## Development

From the repository root:

```bash
submilli build test -p @submilli/browserbase-stagehand --skip-network
```
