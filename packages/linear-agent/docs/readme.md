# Linear agent sessions

Use `@submilli/linear-agent` to participate in an existing Linear agent session.
Calls are synchronous and publish activities visible to the user. `thought`
means a short public progress update; do not put private reasoning in it.
Replace the example session UUID with the session supplied by your agent server.

## Progress, action, and completion

```ts
import { thought, action, respond } from "@submilli/linear-agent";

function main(): string {
    const sessionId = "YOUR_SESSION_UUID";
    thought(sessionId, "Checking the linked changes.", { ephemeral: true });
    action(sessionId, "Searching", "Related issues", { result: "Found two related issues." });
    return respond(sessionId, "The change is ready.").id;
}
```

`fail(sessionId, body, options?)` publishes an error activity. It does not catch
an execution error or turn a failed Linear mutation into success. All activity
helpers return an `Activity` with its UUID and content. They accept an optional
`id` in their options: Linear specifies UUIDv4. Supplying an ID is **not a
verified guarantee of idempotent retries**. This package performs no retries.

Only `thought` and `action` accept `ephemeral`. Only `action` accepts `result`.
Other activity options contain only `id`, except the account-linking options
below. Callers never supply activity discriminators or signal metadata.

## Ask now, respond later

```ts
import { ask, respond } from "@submilli/linear-agent";

function askRepository(sessionId: string): string {
    return ask(sessionId, "Which repository should I use?").id;
}

function answerLater(sessionId: string, repository: string): string {
    return respond(sessionId, "The change for " + repository + " is ready.").id;
}

function main(): string {
    const sessionId = "YOUR_SESSION_UUID";
    return askRepository(sessionId);
}
```

Linear marks an elicitation as waiting for user input, but emitting one does
not suspend the local runtime. Emitting a response does not terminate a Submilli
turn. The caller/server controls execution and decides when to invoke the next
operation in the same session. Receiving webhooks, routing events, storing
conversations, and honoring stop requests belong to that server.

## Present labeled choices

```ts
import { askChoice } from "@submilli/linear-agent";

function main(): string {
    const sessionId = "YOUR_SESSION_UUID";
    return askChoice(sessionId, "Which repository should I use?", [
        { label: "Frontend", value: "acme/frontend" },
        { label: "Backend", value: "acme/backend" },
    ]).id;
}
```

Choices produce an elicitation with Linear's `select` signal. A user can reply
in free text instead of choosing an option; their reply is a prompt activity.

## Request account linking

```ts
import { askAccountConnection } from "@submilli/linear-agent";

function main(): string {
    const sessionId = "YOUR_SESSION_UUID";
    const authorizationUrl = "https://auth.example.com/oauth";
    const linearUserId = "YOUR_LINEAR_USER_UUID";
    return askAccountConnection(sessionId, "Connect GitHub to continue.", authorizationUrl, {
        providerName: "GitHub",
        userId: linearUserId,
    }).id;
}
```

The URL must use HTTPS with a nonempty host. `providerName` and the target
Linear `userId` are optional. This emits an elicitation with the `auth` signal;
it does not implement OAuth or wait for account linking to finish. Linear's
account-linking UI is temporary and is dismissed by a newer agent activity.

## Read paginated history

```ts
import { getSession, listActivities } from "@submilli/linear-agent";

function main(): string {
    const sessionId = "YOUR_SESSION_UUID";
    const session = getSession(sessionId);
    const lines: string[] = [session.status];
    let after: string | null = null;
    for (let pageNumber = 0; pageNumber < 5; pageNumber++) {
        const page = listActivities(sessionId, { first: 25, after: after });
        for (const activity of page.nodes) {
            lines.push(activity.id + ": " + activity.content.type);
            if (activity.signal === "stop") lines.push("User requested a stop.");
        }
        if (!page.pageInfo.hasNextPage || page.pageInfo.endCursor === null) break;
        after = page.pageInfo.endCursor;
    }
    return lines.join("\n");
}
```

`listActivities` reads exactly one page. Pagination defaults to 50 activities
and accepts up to 250 per page. History includes user-created `prompt`
activities, nullable action results, and known `auth`, `select`, `stop`, and
`continue` signals. It does not poll, cancel execution, or emit activities.

## Session links

`setExternalLinks(sessionId, links)` replaces all links (`[]` clears them).
`addExternalLinks(sessionId, links)` adds links without replacing the list.
Each link is `{ label: string, url: string }` and URLs should be unique.
`removeExternalLinks(sessionId, urls)` removes specific URLs. Each helper
returns the updated `Session`; reading `session.externalUrls` lists its links.
Plans and proactive session creation remain available in `@submilli/linear`.

## Credentials and permissions

Bind the owning Linear app's OAuth access token to `LINEAR_API_KEY`. The
underlying Linear package sends OAuth tokens with `Bearer` and personal
`lin_api_` keys raw. Agent-session mutations require the app identity that owns
the session; a personal API key is not a substitute. Authorize with `actor=app`.

Every helper checks a separate `linear-agent.<functionName>` capability with
`sessionId`. The dependency checks its existing `linear.app/*` capabilities
against the target's `teamId`. Denial at either layer throws; do not try another
helper to bypass it. See the package README for blueprint setup and team rules.

References: [Linear agent interactions](https://linear.app/developers/agent-interaction),
[signals](https://linear.app/developers/agent-signals), and
[official schema](https://raw.githubusercontent.com/linear/linear/refs/heads/master/packages/sdk/src/schema.graphql).
