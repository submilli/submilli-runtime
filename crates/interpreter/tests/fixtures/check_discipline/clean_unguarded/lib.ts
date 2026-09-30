// expect-error-count: 0

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

/** Sends the message without a check of its own. */
export function send(input: Input): Input {
  post(input.channelId, input.text);
  post(input.channelId, input.tags.join(","));
  const deliver = (): void => post(input.channelId, input.text);
  deliver();
  input.text = "sent";
  return input;
}
