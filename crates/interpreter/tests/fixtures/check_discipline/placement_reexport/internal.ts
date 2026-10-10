import { check } from "submilli:security";

/**
 * Sends the message.
 * @param channelId Conversation to post in.
 * @capability test.com/send { channelId: string }
 */
export function send(channelId: string): void {
  check("test.com/send", { channelId: channelId });
}

/** Sends messages. */
export class Client {
  /**
   * Opens a client.
   * @param channelId Conversation to post in.
   * @returns The opened client.
   * @capability test.com/open { channelId: string }
   */
  static open(channelId: string): Client {
    check("test.com/open", { channelId: channelId });
    return new Client();
  }
}
