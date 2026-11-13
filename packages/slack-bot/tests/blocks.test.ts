import { label } from "submilli:test";
import { sendMessage, SlackError } from "@submilli/slack-bot";
import { attachMessageBlocks } from "../src/blocks";
import { attachMessageMetadata, MessageMetadata } from "../src/metadata";

interface MessageBody {
    channel: string;
    text: string;
    thread_ts?: string;
    unfurl_links?: boolean;
    unfurl_media?: boolean;
    blocks?: Record<string, unknown>[];
    metadata?: MessageMetadata;
}

function main(): void {
    label("absent and empty blocks leave the request bytes unchanged");
    const bare: MessageBody = { channel: "C1", text: "fallback" };
    attachMessageBlocks(bare, undefined);
    assert(JSON.stringify(bare) === "{\"channel\":\"C1\",\"text\":\"fallback\"}", "absent blocks are omitted");
    attachMessageBlocks(bare, []);
    assert(JSON.stringify(bare) === "{\"channel\":\"C1\",\"text\":\"fallback\"}", "empty blocks are omitted");

    const options: MessageBody = {
        channel: "C1", text: "fallback", thread_ts: "123.456",
        unfurl_links: false, unfurl_media: true,
    };
    attachMessageBlocks(options, undefined);
    attachMessageMetadata(options, { event_type: "task_created", event_payload: { id: "123" } });
    assert(JSON.stringify(options) === "{\"channel\":\"C1\",\"metadata\":{\"event_payload\":{\"id\":\"123\"},\"event_type\":\"task_created\"},\"text\":\"fallback\",\"thread_ts\":\"123.456\",\"unfurl_links\":false,\"unfurl_media\":true}", "existing options and metadata keep their bytes: " + JSON.stringify(options));

    label("blocks pass through nested content alongside text and metadata");
    const blocks: Record<string, unknown>[] = [
        { type: "section", text: { type: "mrkdwn", text: "*Ready*" }, block_id: "status" },
        { type: "context", elements: [{ type: "plain_text", text: "Done" }] },
    ];
    attachMessageBlocks(options, blocks);
    const parsed = JSON.parse(JSON.stringify(options)) as MessageBody;
    assert(JSON.stringify(parsed.blocks) === JSON.stringify(blocks), "all block fields round-trip");
    assert(parsed.text === "fallback", "required fallback text is preserved");
    assert(parsed.thread_ts === "123.456", "thread timestamp is preserved");
    assert(parsed.unfurl_links === false && parsed.unfurl_media === true, "unfurl settings are preserved");
    assert(parsed.metadata?.event_type === "task_created", "metadata is preserved");

    label("50 blocks are accepted and 51 fail before token lookup or HTTP");
    const boundary: Record<string, unknown>[] = [];
    for (let i = 0; i < 50; i++) boundary.push({ type: "divider" });
    attachMessageBlocks(bare, boundary);
    assert(bare.blocks?.length === 50, "50 blocks are attached");
    boundary.push({ type: "divider" });
    let rejection = "no error";
    try {
        sendMessage({ channelId: "C1", text: "fallback", blocks: boundary });
    } catch (error: SlackError) {
        rejection = error.code + ":" + error.status.toString();
    } catch (error) {
        rejection = error.name + ": " + error.message;
    }
    assert(rejection === "invalid_blocks:0", "expected invalid_blocks:0, got " + rejection);
}
