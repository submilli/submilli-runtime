// Every request the page makes goes through here, to the control listener it was served
// from. The session token travels only in the `Authorization` header, never in a URL.

import type { ActionAnswer } from "./types";

/** Where the page keeps its browser session token between loads. */
export const SESSION_KEY = "submilli-playground-session";

/** What the page says when the playground doesn't answer at all. */
export const UNREACHABLE = "The playground didn’t answer. If it stopped or restarted, start it and run `submilli playground open` for a new link.";

/** The kind of an action the page stopped waiting for, with Stop. */
export const STOPPED = "stopped";

/** What an action route said: accepted with its result, or refused with the CLI's words. */
export type ActionOutcome<T> =
  | { ok: true; result: T; text: string }
  | { ok: false; kind: string; message: string };

/** The token was refused: the page is signed out until the next login link. */
export class SignedOutError extends Error {
  constructor() {
    super("signed out");
  }
}

/** A read route answered with an error: `code` and `message` are the server's. */
export class ReadError extends Error {
  constructor(
    readonly status: number,
    readonly code: string,
    message: string,
  ) {
    super(message);
  }
}

export interface ClientOptions {
  /** Defaults to the global `fetch`, looked up on each request. */
  fetch?: typeof fetch;
  /** Called once per refused token, after it was dropped. */
  onSignedOut?: () => void;
}

export class Client {
  constructor(private readonly options: ClientOptions = {}) {}

  /** A read route's JSON. */
  async read<T>(path: string): Promise<T> {
    const response = await this.request(path, {});
    if (!response.ok) {
      const body = await errorBody(response);
      throw new ReadError(response.status, body.code, body.message);
    }
    return (await response.json()) as T;
  }

  /**
   * An action route. These answer HTTP 200 with `{exit, result, text}`. A refusal has
   * `result.error`, with the message the CLI would print. A run that was made and ended
   * denied, stopped or failed exits non-zero too, as the CLI does, but it is a result.
   */
  async act<T>(path: string, body: unknown, signal?: AbortSignal): Promise<ActionOutcome<T>> {
    let response: Response;
    try {
      response = await this.request(path, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify(body),
        signal,
      });
    } catch (error) {
      if (error instanceof SignedOutError) throw error;
      // Given up on by the page: the playground stops what the request started.
      if (signal?.aborted) return { ok: false, kind: STOPPED, message: "stopped" };
      return { ok: false, kind: "unreachable", message: UNREACHABLE };
    }
    if (!response.ok) {
      const { code, message } = await errorBody(response);
      return { ok: false, kind: code, message };
    }
    const answer = (await response.json()) as ActionAnswer<T>;
    const error = answer.result.error;
    if (answer.exit !== 0 && error) return { ok: false, kind: error.kind, message: error.message };
    if (answer.exit !== 0 && typeof (answer.result as { run?: unknown }).run !== "number") {
      return { ok: false, kind: "failed", message: answer.text };
    }
    return { ok: true, result: answer.result, text: answer.text };
  }

  /** A raw request with the token attached, for the session feed's stream. */
  async request(path: string, init: RequestInit): Promise<Response> {
    const headers = new Headers(init.headers);
    const token = localStorage.getItem(SESSION_KEY);
    if (token) headers.set("authorization", `Bearer ${token}`);
    const fetchFn = this.options.fetch ?? globalThis.fetch;
    const response = await fetchFn(path, { ...init, headers });
    if (response.status === 401) {
      localStorage.removeItem(SESSION_KEY);
      this.options.onSignedOut?.();
      throw new SignedOutError();
    }
    return response;
  }
}

async function errorBody(response: Response): Promise<{ code: string; message: string }> {
  const fallback = { code: `http-${response.status}`, message: `the playground answered ${response.status}` };
  try {
    const body = (await response.json()) as { error?: unknown; message?: unknown };
    return {
      code: typeof body.error === "string" ? body.error : fallback.code,
      message: typeof body.message === "string" ? body.message : fallback.message,
    };
  } catch {
    return fallback;
  }
}
