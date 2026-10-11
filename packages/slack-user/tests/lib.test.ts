import { label } from "submilli:test";
import { SlackError, MessageRef, SearchOptions, SendMessageInput, search } from "@submilli/slack-user";

function main(): void {
    label("SlackError preserves stable machine-readable fields");
    const err = new SlackError("rate_limited", "Slack API error: rate_limited", 429);
    assert(err.code === "rate_limited", "error code is preserved");
    assert(err.status === 429, "HTTP status is preserved");
    assert(err.message.includes("rate_limited"), "message is actionable");

    label("public inputs serialize as plain structs");
    const ref: MessageRef = { channelId: "C1", ts: "123.456", threadTs: "123.000" };
    const searchOptions: SearchOptions = { limit: 10, contentTypes: ["messages", "files"] };
    const send: SendMessageInput = { channelId: "C1", text: "hello", threadTs: "123.000" };
    assert(JSON.stringify(ref).includes("threadTs"), "message refs carry an optional thread root");
    assert(searchOptions.limit === 10, "search options are typed");
    assert(send.text === "hello", "send input is typed");

    label("an unbound user token is reported before any request");
    let code = "none";
    try {
        search("quarterly plan");
    } catch (e) {
        if (e instanceof SlackError) code = e.code;
    }
    assert(code === "missing_token", "got " + code);
}
