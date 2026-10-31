# @submilli/browserbase-stagehand

Hosted Stagehand `act`, `extract`, and `observe` on a cloud browser. Submilli
cannot run the Stagehand Node SDK or a CDP socket, so browser actions go through
the Stagehand session API. Connection URLs and signing keys are never returned.
The API key identifies the project; this package never sends or asks for a
project id.

`act` and `extract` use a model to interpret natural-language instructions.
`session.act("click the Sign in button")` performs one action on the current
page. `session.extract("extract each product's name and price", schema)` reads
data without clicking or navigating. The optional schema is JSON Schema text
that specifies the result's shape; the extracted value is returned in `data`.
Instructions are ordinary text, not JavaScript or CSS selectors. `observe`
also takes natural language, but returns possible actions without running them.

## Install and bind the key

In an existing blueprint directory:

```sh
submilli install submilli/submilli-runtime @submilli/browserbase-stagehand
submilli blueprint add-package @submilli/browserbase-stagehand --no-capabilities
submilli blueprint secret add BROWSERBASE_API_KEY --store browserbase_api_key
submilli secret put browserbase_api_key
submilli blueprint capability add browserbase.com/stagehand.start
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

## Saved login context

Choose the saved Browserbase Context in the blueprint/session configuration,
not in generated code. The package reads `BROWSERBASE_CONTEXT_ID` through
`submilli:variables`; `open` and `current` do not accept a `contextId` option.
Create the Context separately in Browserbase, declare this optional variable,
and bind its value when opening the runtime session:

```yaml
variables:
  BROWSERBASE_CONTEXT_ID: {}
```

The package needs `variables.get` filtered to
`name == "BROWSERBASE_CONTEXT_ID"`; `add-package` supplies that derived grant.
If the variable is absent, a new browser starts without a saved context. An
empty or malformed value is rejected, and a denied variable read is not treated
as absent. To save login changes when the browser closes, call
`StagehandSession.open({ persistContext: true })` with the variable bound.
Persistence defaults to false, so loading a context does not overwrite it.

## Authority

| Operation | Caller capability and truthful fields |
| --- | --- |
| `StagehandSession.open` and `StagehandSession.current` | `browserbase.com/stagehand.start { model, proxies, verified, solveCaptchas, keepAlive, region, timeoutSeconds, contextId, persistContext }` |
| `act`, `replay`, `observe`, `extract`, `end` | `browserbase.com/stagehand {}` |
| `session.liveView` | `browserbase.com/liveView {}` |
| `session.goto` | `browserbase.com/navigate { host, path, protocol }` |

The browser id is not a grant field. It is created at runtime and changes every
browser. `current` checks `stagehand.start` even when it reuses a stored browser,
using the options that would apply if a new one had to be started. A host and
path grant covers the submitted URL, not redirects or subresources. The query
is not in the check.
Proxies, Verified, and `solveCaptchas` default to false and are sent explicitly,
because the Browserbase session API would otherwise enable CAPTCHA solving.
Custom proxy URLs and passwords are not accepted. Session timeout defaults to
300 seconds so a grant can cap it; the project default is not used.

`StagehandSession.open` and `StagehandSession.current` register cleanup.
`current` stores the id and region in the runtime session under `stagehand` and
reuses that browser while it is still pending or running. The first call also
needs `stagehand.start`. A non-default region uses its own Stagehand host
(`api.use1`, `api.euc1`, or `api.apse1`); `end` and session close use that same
host. `session.liveView` needs `browserbase.com/liveView` and returns the watch
link. The CDP URL is dropped because it embeds a signing key.

## Development

From the repository root:

```bash
submilli build test -p @submilli/browserbase-stagehand --skip-network
```
