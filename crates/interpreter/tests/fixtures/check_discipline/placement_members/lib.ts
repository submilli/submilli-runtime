// expect-warning: `check()` is called in `Client.guard`, which is not part of the package's public API
// expect-warning: `check()` is called in `Hidden.send`, which is not part of the package's public API
// expect-error-count: 2
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

/** Shared by every client. */
class Base {
  /**
   * Archives a conversation.
   * @param channelId Conversation to target.
   * @capability test.com/archive { channelId: string }
   */
  archive(channelId: string): void {
    check("test.com/archive", { channelId: channelId });
  }
}

/** Sends messages. */
export class Client extends Base {
  private token: string;

  constructor(token: string) {
    super();
    check("test.com/create", {});
    this.token = token;
  }

  /**
   * Sends the message.
   * @param channelId Conversation to post in.
   * @param text Message text.
   * @capability test.com/send { channelId: string }
   */
  send(channelId: string, text: string): void {
    check("test.com/send", { channelId: channelId });
    post(channelId, text);
  }

  /** The token in use. */
  get secret(): string {
    check("test.com/secret", {});
    return this.token;
  }

  /**
   * Approves the send.
   * @param channelId Conversation to target.
   * @capability test.com/guard { channelId: string }
   */
  private guard(channelId: string): void {
    check("test.com/guard", { channelId: channelId });
  }
}

class Hidden {
  /**
   * Sends without a client.
   * @param channelId Conversation to target.
   * @capability test.com/hidden { channelId: string }
   */
  send(channelId: string): void {
    check("test.com/hidden", { channelId: channelId });
  }
}
