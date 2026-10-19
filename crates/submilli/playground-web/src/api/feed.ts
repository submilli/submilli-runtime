// A session's live event feed. It is read with `fetch` and a stream reader rather than
// `EventSource`, which can't send the bearer header, and the token must stay out of URLs.

import type { FeedSignal, StoredEvent } from "./types";
import { Client, SignedOutError } from "./client";

export interface SseMessage {
  id: string | null;
  event: string;
  data: string;
}

/** A server-sent-events parser fed text as it arrives, in chunks of any size. */
export function createSseParser(onMessage: (message: SseMessage) => void) {
  let buffer = "";
  let id: string | null = null;
  let event = "";
  let data: string[] = [];

  const line = (text: string) => {
    if (text === "") {
      if (data.length > 0) onMessage({ id, event: event || "message", data: data.join("\n") });
      id = null;
      event = "";
      data = [];
      return;
    }
    if (text.startsWith(":")) return; // a comment, such as a keep-alive
    const colon = text.indexOf(":");
    const field = colon < 0 ? text : text.slice(0, colon);
    let value = colon < 0 ? "" : text.slice(colon + 1);
    if (value.startsWith(" ")) value = value.slice(1);
    if (field === "data") data.push(value);
    else if (field === "event") event = value;
    else if (field === "id") id = value;
  };

  return {
    push(text: string) {
      buffer += text;
      for (;;) {
        const end = buffer.search(/\r\n|\r|\n/);
        if (end < 0) return;
        // A `\r` at the very end may be the first half of `\r\n`.
        if (buffer[end] === "\r" && end === buffer.length - 1) return;
        const width = buffer.startsWith("\r\n", end) ? 2 : 1;
        line(buffer.slice(0, end));
        buffer = buffer.slice(end + width);
      }
    },
    /** Drops a half-received event, as when the connection closes partway through it. */
    reset() {
      buffer = "";
      id = null;
      event = "";
      data = [];
    },
  };
}

export type FeedState = "connecting" | "open" | "reconnecting" | "ended";

export interface FeedOptions {
  client: Client;
  session: string;
  onEvent: (event: StoredEvent) => void;
  /** `run-idle`, and the `session-ended` or `feed-error` that ends the feed. */
  onSignal: (signal: FeedSignal) => void;
  onState: (state: FeedState) => void;
  /** How long to wait before the `attempt`th reconnect, in milliseconds. */
  retryDelay?: (attempt: number) => number;
}

const STORED = new Set(["event", "gap", "chat", "chat-replaced", "chat-tools"]);

const backoff = (attempt: number) => Math.min(500 * 2 ** (attempt - 1), 5000);

/**
 * Follows a session's feed until it ends or `stop` is called. A dropped connection is
 * opened again with `Last-Event-ID` set to the last `session_seq` received, so the server
 * sends only what came after.
 */
export function openFeed(options: FeedOptions): { stop(): void } {
  const { client, session, onEvent, onSignal, onState } = options;
  const retryDelay = options.retryDelay ?? backoff;
  const abort = new AbortController();
  let lastSeq = 0;
  let ended = false;

  const end = (signal: FeedSignal) => {
    ended = true;
    onSignal(signal);
    onState("ended");
  };

  const parser = createSseParser((message) => {
    if (ended) return;
    let data: unknown;
    try {
      data = JSON.parse(message.data);
    } catch {
      return;
    }
    if (STORED.has(message.event)) {
      const stored = data as StoredEvent;
      if (stored.session_seq <= lastSeq) return;
      lastSeq = stored.session_seq;
      onEvent(stored);
    } else if (message.event === "run-idle") {
      onSignal(data as FeedSignal);
    } else if (message.event === "session-ended" || message.event === "feed-error") {
      end(data as FeedSignal);
    }
  });

  const read = async (): Promise<"dropped" | "done"> => {
    const headers: Record<string, string> = { accept: "text/event-stream" };
    if (lastSeq > 0) headers["last-event-id"] = String(lastSeq);
    const response = await client.request(`/api/sessions/${encodeURIComponent(session)}/events`, {
      headers,
      signal: abort.signal,
    });
    // A server that is busy or restarting answers 5xx for a moment; that is tried again.
    if (response.status >= 500) throw new Error(`the feed answered ${response.status}`);
    if (!response.ok || !response.body) {
      // The session can't be followed at all (unknown, or not readable): say why once.
      const body = (await response.json().catch(() => ({}))) as { message?: unknown };
      const message =
        typeof body.message === "string" ? body.message : `the feed answered ${response.status}`;
      end({ kind: "feed-error", session, message });
      return "done";
    }
    onState("open");
    const reader = response.body.getReader();
    const decoder = new TextDecoder();
    for (;;) {
      const { done, value } = await reader.read();
      if (done) break;
      parser.push(decoder.decode(value, { stream: true }));
      if (ended) {
        await reader.cancel().catch(() => {});
        return "done";
      }
    }
    parser.reset();
    return "dropped";
  };

  const follow = async () => {
    onState("connecting");
    let attempt = 0;
    while (!abort.signal.aborted && !ended) {
      const seqBefore = lastSeq;
      try {
        if ((await read()) === "done") return;
      } catch (error) {
        if (abort.signal.aborted || error instanceof SignedOutError) return;
        parser.reset();
      }
      if (abort.signal.aborted) return;
      // A connection that brought something in starts the backoff over.
      attempt = lastSeq > seqBefore ? 1 : attempt + 1;
      onState("reconnecting");
      await new Promise((resolve) => setTimeout(resolve, retryDelay(attempt)));
    }
  };

  void follow();
  return { stop: () => abort.abort() };
}
