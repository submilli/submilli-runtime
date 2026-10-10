// expect-error-count: 0
import { check } from "submilli:security";

/** What a message is sent with. */
export interface Input {
  /** Conversation to post in. */
  channelId: string;
  /** Message text. */
  text: string;
  /** Thread to reply in. */
  threadTs: string | null;
  /** Labels to attach. */
  tags: string[];
  /** Delivery settings. */
  options: Options | null;
}

/** Delivery settings. */
export interface Options {
  /** Whether links unfurl. */
  unfurl: boolean;
  /** Users to notify. */
  notify: string[];
}

function post(channelId: string, text: string): void {}

interface Reply { ok: boolean; message: { ts: string; text: string }; members: string[]; }

function request(channelId: string): Reply {
  return { ok: true, message: { ts: "1", text: channelId }, members: [] };
}

/** What was sent. */
export interface Sent {
  /** Message timestamp. */
  ts: string;
  /** Message text. */
  text: string;
  /** Who saw it. */
  members: string[];
}

/**
 * Sends the message.
 * @param channelId Conversation to post in.
 * @returns The sent message details.
 * @capability test.com/send { channelId: string }
 */
export function send(channelId: string): Sent {
  check("test.com/send", { channelId: channelId });
  const reply = request(channelId);
  if (!reply.ok) {
    throw new Error("failed: " + reply.message.text);
  }
  const members: string[] = [];
  for (const member of reply.members) members.push(member);
  for (const member of reply.members) members.push(member + reply.message.ts);
  return { ts: reply.message.ts, text: reply.message.text, members: reply.members };
}
