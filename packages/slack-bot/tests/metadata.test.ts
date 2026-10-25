import { label } from "submilli:test";
import { SlackError } from "@submilli/slack-bot";
import {
    MessageMetadata,
    attachMessageMetadata,
    configuredMessageMetadata,
    messageMetadataFrom,
    parseMessageMetadata,
} from "../src/metadata";

const NESTED = {
    id: "11223",
    nested: { ok: true },
    tags: ["a", "b"],
    counts: [1, 2],
    n: 1,
    flag: false,
    empty: null,
};

const MALFORMED: string[] = [
    "",
    "   ",
    "{",
    "null",
    "[]",
    "1",
    "true",
    "\"task_created\"",
    "{}",
    "{\"event_type\":\"task_created\"}",
    "{\"event_type\":1,\"event_payload\":{}}",
    "{\"event_type\":\"\",\"event_payload\":{}}",
    "{\"event_type\":\"task.created\",\"event_payload\":{}}",
    "{\"event_type\":\"task_created\",\"event_payload\":[]}",
    "{\"event_type\":\"task_created\",\"event_payload\":null}",
    "{\"event_type\":\"task_created\",\"event_payload\":\"id\"}",
    "{\"event_type\":\"task_created\",\"event_payload\":{},\"extra\":1}",
];

function rejection(raw: string): string {
    try {
        parseMessageMetadata(raw);
    } catch (error: SlackError) {
        return error.code + ":" + error.status.toString();
    } catch (error) {
        return error.name + ": " + error.message;
    }
    return "no error";
}

function main(): void {
    label("an unset variable omits metadata");
    assert(messageMetadataFrom(undefined) === undefined, "undefined raw is absent");
    assert(configuredMessageMetadata() === undefined, "the test session has no variable");
    const bare: { channel: string; text: string; metadata?: MessageMetadata } = { channel: "C1", text: "hi" };
    attachMessageMetadata(bare, undefined);
    assert(JSON.stringify(bare) === "{\"channel\":\"C1\",\"text\":\"hi\"}", "absent metadata is omitted from the body");

    label("valid metadata round-trips nested payload values");
    const raw = JSON.stringify({ event_type: "task_created", event_payload: NESTED });
    const parsed = parseMessageMetadata(raw);
    assert(parsed.event_type === "task_created", "event type is preserved");
    assert(JSON.stringify(parsed.event_payload) === JSON.stringify(NESTED), "nested payload round-trips");
    assert(parseMessageMetadata("{\"event_type\":\"task-created\",\"event_payload\":{}}").event_type === "task-created", "hyphens are allowed");
    const body: { channel: string; text: string; metadata?: MessageMetadata } = { channel: "C1", text: "hi" };
    attachMessageMetadata(body, parsed);
    const encoded = JSON.stringify(body);
    const again = JSON.parse(encoded) as { metadata: MessageMetadata };
    assert(again.metadata.event_type === "task_created", "attached event type");
    assert(JSON.stringify(again.metadata.event_payload) === JSON.stringify(NESTED), "attached payload round-trips");

    label("malformed metadata is rejected before a request body is sent");
    for (const sample of MALFORMED) {
        assert(rejection(sample) === "invalid_metadata:0", "expected invalid_metadata:0 for " + JSON.stringify(sample) + ", got " + rejection(sample));
    }
}
