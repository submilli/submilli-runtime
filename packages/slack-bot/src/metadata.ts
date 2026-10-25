// Optional Slack message metadata from `SLACK_BOT_MESSAGE_METADATA`.
// The package does not interpret `event_payload`.
import { get } from "submilli:variables";
import { SlackError } from "./error";

// Slack ignores an invalid event_type and still posts the message. Reject it
// here so a bad configuration cannot send a message without its metadata.
const EVENT_TYPE = /^[A-Za-z0-9_-]+$/;

/** Slack's generic message-metadata object. The payload is not interpreted. */
export interface MessageMetadata {
    event_type: string;
    event_payload: Record<string, unknown>;
}

/**
 * Read configured metadata, or `undefined` when the variable is unset.
 * A missing `variables.get` grant throws `PermissionDeniedError`; that is not
 * an absent variable. A present but malformed value throws `SlackError`.
 * @returns The parsed metadata, or `undefined` when the variable is unset.
 */
export function configuredMessageMetadata(): MessageMetadata | undefined {
    return messageMetadataFrom(get("SLACK_BOT_MESSAGE_METADATA"));
}

/**
 * Parse a raw variable value. `undefined` means the variable is unset.
 * @param raw The variable's JSON text, or `undefined` when it is unset.
 * @returns The parsed metadata, or `undefined` when `raw` is unset.
 */
export function messageMetadataFrom(raw: string | undefined): MessageMetadata | undefined {
    if (raw === undefined) return undefined;
    return parseMessageMetadata(raw);
}

/**
 * Parse Slack's `{ event_type, event_payload }` object.
 * @param raw JSON text of the configured metadata.
 * @returns The parsed metadata.
 * @throws SlackError when the text is empty or not that object.
 */
export function parseMessageMetadata(raw: string): MessageMetadata {
    if (raw.length === 0) throw invalidMetadata("SLACK_BOT_MESSAGE_METADATA is empty");
    let parsed: unknown;
    try {
        parsed = JSON.parse(raw);
    } catch (_cause) {
        throw invalidMetadata("SLACK_BOT_MESSAGE_METADATA is not JSON");
    }
    if (!isJsonObject(parsed)) {
        throw invalidMetadata("SLACK_BOT_MESSAGE_METADATA must be a JSON object");
    }
    const record = parsed as Record<string, unknown>;
    const keys = Object.keys(record);
    if (keys.length !== 2 || !keys.includes("event_type") || !keys.includes("event_payload")) {
        throw invalidMetadata("SLACK_BOT_MESSAGE_METADATA must contain only event_type and event_payload");
    }
    const eventType = record["event_type"];
    if (typeof eventType !== "string" || !EVENT_TYPE.test(eventType)) {
        throw invalidMetadata("event_type must be a non-empty string of letters, digits, underscores, and hyphens");
    }
    const payload = record["event_payload"];
    if (!isJsonObject(payload)) throw invalidMetadata("event_payload must be a JSON object");
    return { event_type: eventType, event_payload: payload as Record<string, unknown> };
}

/**
 * Set `metadata` only when a value was configured, so an absent variable omits the field.
 * @param body Request body that may carry Slack message metadata.
 * @param metadata Parsed metadata, or `undefined` when the variable is unset.
 */
export function attachMessageMetadata(
    body: { metadata?: MessageMetadata },
    metadata: MessageMetadata | undefined,
): void {
    if (metadata !== undefined) body.metadata = metadata;
}

function isJsonObject(value: unknown): boolean {
    return value !== null && typeof value === "object" && !Array.isArray(value);
}

function invalidMetadata(message: string): SlackError {
    return new SlackError("invalid_metadata", message, 0);
}
