# Browserbase

Use `@submilli/browserbase` to search and fetch pages without a browser, or to
drive a cloud browser with Stagehand `act`, `extract`, and `observe`.

Named imports and the default package namespace are supported. Credentials are
supplied internally: never ask for a key, pass one in a call, or print one.
Do not pass a project id. This package does not accept one.

A denial means the operation is forbidden, not that the call was malformed.
`BrowserbaseError` with code `quota_exceeded` means the plan allowance is used
up, including the free Model Gateway token allowance. It does not mean the
credential is invalid.

<!-- submilli:if capability="browserbase.com/search" -->

## Search

`search(query, options?)` returns titles and URLs. It does not start a browser
and has no continuation cursor. The default is 10 results from Parallel
`advanced`. Set `provider` and `mode` together. Exa `deep` and `deep-lite` are
not part of the included search allowance.

```ts
import { search } from "@submilli/browserbase";

function main(): string {
    const found = search("WebAssembly garbage collection", { numResults: 5 });
    return JSON.stringify(found.results);
}
```

<!-- submilli:endif -->

<!-- submilli:if capability="browserbase.com/fetch" -->

## Fetch

`fetchPage(url, options?)` returns the page as markdown by default, or `raw`.
The origin status is on the result; a non-2xx page is not thrown. Proxies,
redirects, and insecure TLS default off. A context id sends that context's cookies.

<!-- submilli:if capability="browserbase.com/delegatedFetch" -->

Set `allowRedirects` only when the final host may differ from the URL you passed.

<!-- submilli:endif -->

```ts
import { fetchPage } from "@submilli/browserbase";

function main(): string {
    const page = fetchPage("https://example.com");
    return JSON.stringify({ status: page.statusCode, content: page.content });
}
```

<!-- submilli:endif -->

<!-- submilli:if capability="browserbase.com/stagehand.start" -->

## Stagehand

`StagehandSession.current()` returns the browser stored for this runtime session,
or starts one and stores it. The next execution reuses that browser. Closing the
runtime session ends it. `session.end()` ends it sooner. Starting a browser
registers cleanup, so both `current` and `open` need a session store.
Call `session.goto`, `session.act`, `session.observe`, `session.extract`, and
`session.end` on that object. `act` performs one step, such as "click the Sign in
button". Pass an action from `observe` to `replay` to run that selector with no
model call. Nothing here polls.

Keep `act` instructions atomic. Put dynamic or sensitive values in `variables`
as `%name%` placeholders. Those values are sent to Stagehand and are not logged
by this package. No separate model key is sent, so inference uses Model Gateway.
The default model is `google/gemini-2.5-flash`.

The returned `sessionId` is the Browserbase session id.

<!-- submilli:if capability="browserbase.com/sessions.read" -->

`session.liveView()` returns the watch links. Open `debuggerFullscreenUrl`.

<!-- submilli:endif -->

<!-- submilli:if all="browserbase.com/stagehand.current browserbase.com/stagehand.start browserbase.com/stagehand.navigate browserbase.com/stagehand.act browserbase.com/stagehand.extract browserbase.com/delegatedFetch" -->

```ts
import { StagehandSession } from "@submilli/browserbase";

function main(): string {
    const session = StagehandSession.current();
    session.goto("https://example.com");
    session.act("click the More information link");
    const schema = JSON.stringify({ type: "object", properties: { heading: { type: "string" } } });
    const page = session.extract("extract the heading", schema);
    const encoded = JSON.stringify(page.data);
    return encoded === undefined ? "" : encoded;
}
```

<!-- submilli:endif -->

`goto` checks the submitted host. The cloud browser can still follow redirects
when `browserbase.com/delegatedFetch` is granted. Proxies, Verified, and CAPTCHA
solving default off. A session left open runs until its timeout, which defaults
to 300 seconds.

<!-- submilli:endif -->

<!-- submilli:if capability="browserbase.com/contexts.create" -->

## Contexts

`createContext` saves cookies for a later fetch or Stagehand browser. Pass
`contextId` to `StagehandSession.open` and set `persistContext` only when the
run should write cookies back.

<!-- submilli:endif -->
