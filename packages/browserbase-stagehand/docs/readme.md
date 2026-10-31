# Browserbase Stagehand

Use `@submilli/browserbase-stagehand` to create a cloud browser and drive it
with `goto`, `act`, `extract`, and `observe`.

Named imports and the default package namespace are supported. Credentials are
supplied internally: never ask for a key, pass one in a call, or print one.
Do not pass a project id. This package does not accept one.

A denial means the operation is forbidden, not that the call was malformed.
`BrowserbaseError` with code `quota_exceeded` means the plan allowance is used
up, including the free Model Gateway token allowance. It does not mean the
credential is invalid.

<!-- submilli:if capability="browserbase.com/stagehand" -->

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

<!-- submilli:if capability="browserbase.com/liveView" -->

`session.liveView()` returns the watch links. Open `debuggerFullscreenUrl`. That URL can be used outside these grants, so it is a separate capability.

<!-- submilli:endif -->

<!-- submilli:if all="browserbase.com/stagehand.start browserbase.com/stagehand browserbase.com/navigate" -->

```ts
import { StagehandSession } from "@submilli/browserbase-stagehand";

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

`goto` checks the submitted host, path, and protocol. The query is not in the
check, and the cloud browser can still follow redirects. Proxies, Verified, and CAPTCHA
solving default off. A session left open runs until its timeout, which defaults
to 300 seconds.

<!-- submilli:endif -->
