import { label } from "submilli:test";
import { SlackError, MessageRef, SearchOptions, SendMessageInput } from "@submilli/slack-user";

function main(): void {
    label("SlackError preserves stable machine-readable fields");
    const err = new SlackError("rate_limited", "Slack API error: rate_limited", 429);
    assert(err.code === "rate_limited", "error code is preserved");
    assert(err.status === 429, "HTTP status is preserved");
    assert(err.message.includes("rate_limited"), "message is actionable");

    label("public inputs serialize as plain structs");
    const ref: MessageRef = { channelId: "C1", ts: "123.456", threadTs: "123.000" };
    const search: SearchOptions = { limit: 10, contentTypes: ["messages", "files"] };
    const send: SendMessageInput = { channelId: "C1", text: "hello", threadTs: "123.000" };
    assert(JSON.stringify(ref).includes("threadTs"), "message refs carry an optional thread root");
    assert(search.limit === 10, "search options are typed");
    assert(send.text === "hello", "send input is typed");
}
