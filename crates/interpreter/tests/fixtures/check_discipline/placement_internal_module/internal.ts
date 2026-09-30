import { check } from "submilli:security";

/**
 * Approves the send.
 * @capability test.com/send { channelId: string }
 */
export function guard(channelId: string): void {
  check("test.com/send", { channelId: channelId });
}
