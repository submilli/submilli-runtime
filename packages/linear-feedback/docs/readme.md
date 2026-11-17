# Linear feedback

Use `@submilli/linear-feedback` when an assistant collects product feedback from
a person and files it for the product team to review. It files one issue per
piece of feedback in a fixed Linear team, in the team's triage state, or its
first backlog state when the team has no triage.

The team and the credential are secrets, so a program cannot choose where the
feedback goes, read the team's issues, or see the key. File only feedback the
person confirmed: `submitFeedback` refuses anything without `confirmed: true`.

```ts
import { submitFeedback } from "@submilli/linear-feedback";

function main(): string {
    const filed = submitFeedback({
        original: "the run page never told me the agent was waiting for my approval",
        summary: "Run page does not show that a run is waiting for approval",
        confirmed: true,
        kind: "bug",
        goal: "Run the weekly report agent",
        happened: "The run looked stuck for 10 minutes",
        expected: "A clear note that it needs my approval",
        steps: "Agents, then weekly-report, then Run now",
        impact: "Medium: I thought it was broken",
        user: "dana@example.com",
        workspace: "acme",
        page: "/spaces/reports/agents/weekly-report",
    });
    return filed.identifier;
}
```

## How to collect it

1. Invite feedback at a natural pause, such as after finishing a task or after
   an error, at most once per conversation, and never again after the person
   declines.
2. Clarify with one or two short questions at a time, skipping what the person
   already said: what they were trying to do, what happened versus what they
   expected, the steps or screen, how much it matters, and any suggestion. Stop
   asking when they want to stop.
3. Read a short summary back and file only after they confirm or correct it.
4. Pass their first message verbatim as `original`, and later verbatim
   additions or corrections as `additions`.

## The issue

`buildFeedbackIssue` builds the title and Markdown description without network
or secrets, so a program can show exactly what will be filed. The title is
`Feedback (<kind>): <summary>`, prefixed `[TEST] ` when `test` is true. The
description holds the confirmed summary, the clarified fields, the person's
words verbatim, and the context: user, workspace, page, error, and situation.
Each value is quoted in a code fence, so Markdown, links, and mentions in it
take no effect. Words that start like a known credential, such as `lin_api_`,
`sk-`, `ghp_`, or `xoxb-`, and the word after `Bearer`, are replaced with
`[redacted]`; words end at whitespace, quotes, and punctuation such as `=`, so
`KEY="lin_api_…"` is caught too. The person's words are otherwise kept exactly
as written, and their heading says when something was redacted. The title is cut
at 120 characters and each clarified field at 4,000.

The `user` field is whatever the program was told, such as the signed-in user
named in the console's context. It is labeled "not verified" in the issue.

## Errors

`submitFeedback` throws before any request when feedback is unconfirmed, has
no original words or summary, or has an unknown `kind`. It throws when either
secret is unbound, when the team is not found, and on HTTP or GraphQL failures.
HTTP failures report the status only, not the response body. Nothing retries
automatically. Do not retry a failed call on your own: a timeout can come after
Linear created the issue, and a second call files it again.
