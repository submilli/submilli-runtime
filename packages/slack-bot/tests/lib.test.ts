import { label } from "submilli:test";
import { SlackError, MessageRef, SendMessageInput } from "@submilli/slack-bot";

function main(): void {
    label("SlackError preserves stable machine-readable fields");
    const err = new SlackError("not_in_channel", "Slack API error: not_in_channel", 200);
    assert(err.code === "not_in_channel", "error code is preserved");
    assert(err.status === 200, "Slack application errors retain the HTTP status");

    label("bot message inputs are plain structs");
    const ref: MessageRef = { channelId: "C1", ts: "123.456" };
    const send: SendMessageInput = { channelId: ref.channelId, text: "hello" };
    assert(send.channelId === "C1", "channel id passes through");
    assert(send.text === "hello", "message text passes through");
}
