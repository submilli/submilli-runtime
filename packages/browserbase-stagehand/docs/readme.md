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
runtime session ends it. `session.end()` ends it sooner.
The saved login context, if any, is bound by the application. Do not ask for a
context id or pass one in options. `persistContext: true` saves browser changes
back to that configured context when the browser closes; it requires a bound
context and defaults to false.
Call methods on that object:

- `goto(url)` opens a URL directly.
- `act(instruction)` sends a natural-language instruction to the model, which
  identifies the target on the current page and performs one browser action.
  For example, `session.act("click the Sign in button")`. The instruction is
  ordinary text, not JavaScript or a CSS selector. Split multi-step tasks into
  separate calls. Check the returned `success` flag: a failed action can return
  `success: false` without throwing.
- `extract(instruction, schema?)` sends a natural-language description of the
  data to read, such as "extract each product's name and price". The model reads
  the current page and returns the extracted value in `result.data`; it does
  not click or navigate. The optional schema is JSON Schema text describing
  the result's structure, not another instruction. Pass `JSON.stringify(schema)`.
  With a schema, the instruction can be omitted.
- `observe(instruction?)` takes natural language too, such as "find the checkout
  button", but only returns possible actions. It does not perform them. Pass an
  action from `observe` to `replay` to run that selector with no model call.
- `end()` closes the browser.

Nothing here polls.

Give `act` one step at a time: first fill a field, then click a button in a
separate call. Do not pass a whole task such as "sign in and download the report".

To enter a value, put `%name%` in the instruction and supply the matching
`{ name, value }` entry in the second argument's `variables` array. For example:

```ts
import { StagehandSession } from "@submilli/browserbase-stagehand";

function main(): void {
    const session = StagehandSession.current();
    const email = "person@example.com"; // Replace with the authorized value for this task.
    const filled = session.act("type %email% into the Email field", {
        variables: [{ name: "email", value: email }],
    });
    if (!filled.success) throw new Error(filled.message);

    const submitted = session.act("click the Continue button");
    if (!submitted.success) throw new Error(submitted.message);
}
```

Here `%email%` refers to the entry named `email`. Supply only values you are
authorized to enter on that site. Placeholders are not secret storage: their
values are sent to Stagehand and entered into the page.

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
