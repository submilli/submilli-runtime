import { check } from "submilli:security";

/**
 * Approves the send.
 * @param channelId Conversation to post in.
 * @capability test.com/send { channelId: string }
 */
export function guard(channelId: string): void {
  check("test.com/send", { channelId: channelId });
}
