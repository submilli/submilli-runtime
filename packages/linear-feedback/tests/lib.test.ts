// Offline tests: issue building, validation, redaction, state choice. No network, no secret.
import { label } from "submilli:test";
import { Feedback, buildFeedbackIssue, feedbackState, isTeamId, redactCredentials } from "@submilli/linear-feedback";

function base(): Feedback {
    return {
        original: "the run page never told me it was waiting for **my** approval @everyone",
        summary: "Run page does not show that a run waits for approval",
        confirmed: true,
        kind: "bug",
        goal: "Run the weekly report",
        happened: "It looked stuck",
        expected: "A note that it needs approval",
        user: "dana@example.com",
        workspace: "acme",
        page: "/spaces/reports",
    };
}

function throwsWith(text: string, action: () => void): void {
    let matched = false;
    try { action(); } catch (error) { matched = error.message.includes(text); }
    assert(matched, "expected an error mentioning " + text);
}

function main(): void {
    label("title carries kind and summary; test filings start with [TEST]");
    const issue = buildFeedbackIssue(base());
    assert(issue.title === "Feedback (bug): Run page does not show that a run waits for approval", issue.title);
    const test = base();
    test.test = true;
    const testIssue = buildFeedbackIssue(test);
    assert(testIssue.title.startsWith("[TEST] Feedback (bug): "), testIssue.title);
    assert(testIssue.description.includes("This is a test filing"), "test notice");

    label("description holds clarified fields, verbatim words in fences, and context");
    const d = issue.description;
    assert(d.includes("```text\nthe run page never told me it was waiting for **my** approval @everyone\n```"), "verbatim in a fence");
    assert(d.includes("**What they were trying to do**") && d.includes("Run the weekly report"), "goal");
    assert(d.includes("**What they expected**") && d.includes("A note that it needs approval"), "expected");
    assert(!d.includes("**Their suggestion**"), "absent fields are left out");
    assert(d.includes("**User (as told to the assistant, not verified)**") && d.includes("dana@example.com"), "user");
    assert(d.includes("**Error**\n\n_not given_"), "missing context is marked");

    label("a backtick run in the words gets a longer fence");
    const ticks = base();
    ticks.original = "it printed ``` and then broke";
    assert(buildFeedbackIssue(ticks).description.includes("````text\nit printed ``` and then broke\n````"), "longer fence");

    label("nothing is filed before the person confirms");
    const unconfirmed = base();
    unconfirmed.confirmed = false;
    throwsWith("confirms", () => { buildFeedbackIssue(unconfirmed); });
    const empty = base();
    empty.original = "   ";
    throwsWith("original words", () => { buildFeedbackIssue(empty); });
    const noSummary = base();
    noSummary.summary = "";
    throwsWith("summary", () => { buildFeedbackIssue(noSummary); });

    label("a person who skips every question still gets filed");
    const bare: Feedback = { original: "love it", summary: "Likes the console", confirmed: true, kind: "praise" };
    assert(buildFeedbackIssue(bare).description.includes("chose not to add details"), "no details");

    label("titles are one line and capped");
    const long = base();
    long.summary = "line one\nline two " + "x".repeat(200);
    const longTitle = buildFeedbackIssue(long).title;
    assert(!longTitle.includes("\n") && longTitle.includes("[cut at 120 characters]"), longTitle);

    label("pasted credentials are redacted");
    assert(redactCredentials("key lin_api_abcdefghijk1234 failed") === "key [redacted] failed", "linear key");
    assert(redactCredentials("Authorization: Bearer abc.def.ghi\nnext") === "Authorization: Bearer [redacted]\nnext", "bearer");
    assert(redactCredentials("sk-short ok") === "sk-short ok", "short words stay");
    assert(redactCredentials("LINEAR_API_KEY=\"lin_api_abcdefghijk1234\"") === "LINEAR_API_KEY=\"[redacted]\"", "quoted assignment");
    assert(redactCredentials("token=ghp_0123456789abcdef, next") === "token=[redacted], next", "unquoted assignment");
    assert(redactCredentials("{'key': 'sk-proj-0123456789'}") === "{'key': '[redacted]'}", "JSON-ish");
    assert(redactCredentials("authorization: bearer abcdefghij") === "authorization: bearer [redacted]", "lower-case bearer");
    const leaky = base();
    leaky.error = "401 with ghp_0123456789abcdefghij";
    assert(!buildFeedbackIssue(leaky).description.includes("ghp_0123456789"), "error field redacted");

    label("the person's words are kept exactly, without trimming or cutting");
    const exact = base();
    exact.original = "  first line\n" + "y".repeat(9000) + "  ";
    const exactIssue = buildFeedbackIssue(exact).description;
    assert(exactIssue.includes("```text\n" + exact.original + "\n```"), "original unchanged");
    assert(exactIssue.includes("## Their words (verbatim)\n"), "plain heading without redaction");
    const pasted = base();
    pasted.original = "it failed with key lin_api_abcdefghijk1234";
    assert(buildFeedbackIssue(pasted).description.includes("## Their words (verbatim, credential-like words redacted)"), "redaction is disclosed");

    label("triage first, else the first backlog state");
    const states = [
        { id: "b2", name: "Later", type: "backlog", position: 2 },
        { id: "b1", name: "Backlog", type: "backlog", position: 1 },
        { id: "t", name: "Todo", type: "unstarted", position: 0 },
    ];
    assert(feedbackState(states)?.id === "b1", "backlog");
    states.push({ id: "tr", name: "Triage", type: "triage", position: 9 });
    assert(feedbackState(states)?.id === "tr", "triage");
    assert(feedbackState([{ id: "t", name: "Todo", type: "unstarted", position: 0 }]) === undefined, "neither");

    label("team setting: UUID or key");
    assert(isTeamId("2b508a4d-dabf-4ccd-b3af-79f066c981c0"), "uuid");
    assert(!isTeamId("SUB") && !isTeamId("2b508a4d-dabf-4ccd-b3af-79f066c981cz"), "key");

}
