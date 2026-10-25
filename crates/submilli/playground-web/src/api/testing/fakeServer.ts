// A stand-in for the playground's control listener, answering from the recorded
// fixtures. Tests only; nothing in the page imports it.

const raw = import.meta.glob("../../../test/fixtures/*", {
  query: "?raw",
  import: "default",
  eager: true,
}) as Record<string, string>;

export function fixture(name: string): string {
  const text = raw[`../../../test/fixtures/${name}`];
  if (text === undefined) throw new Error(`no fixture ${name}`);
  return text;
}

export function fixtureJson<T = unknown>(name: string): T {
  return JSON.parse(fixture(name)) as T;
}

export function hasFixture(name: string): boolean {
  return raw[`../../../test/fixtures/${name}`] !== undefined;
}

export const SESSION = (n: number) => `5e550000-0000-4000-8000-00000000000${n}`;

export interface Call {
  url: string;
  method: string;
  headers: Headers;
  body: unknown;
  /** The request's signal, so a handler can answer a request the page gives up. */
  signal?: AbortSignal;
}

/** One way to answer a feed request: the text to send, and whether to close after it. */
export interface FeedReply {
  text: string;
  /** Close the stream after the text; otherwise it stays open until the reader stops. */
  close: boolean;
  /** Bytes per chunk, so events arrive split across reads. */
  chunk?: number;
}

export class FakeServer {
  token = "tok-secret";
  calls: Call[] = [];
  sessions: unknown = fixtureJson("sessions.json");
  /** Answers a request before the recorded routes do, when it returns one. */
  handle: (call: Call) => Response | Promise<Response> | undefined = () => undefined;
  /** How a session's feed answers; by default, its recording. */
  feed: (session: string, lastEventId: string | null, attempt: number) => FeedReply = (
    session,
    lastEventId,
  ) => {
    const n = session.slice(-1);
    if (lastEventId !== null && hasFixture(`feed-resume-session-${n}.sse`)) {
      return { text: fixture(`feed-resume-session-${n}.sse`), close: false };
    }
    // A session with no recording has sent nothing yet.
    const name = `feed-session-${n}.sse`;
    return { text: hasFixture(name) ? fixture(name) : "", close: false };
  };
  private feedAttempts: Record<string, number> = {};

  fetch = async (input: RequestInfo | URL, init: RequestInit = {}): Promise<Response> => {
    const url = String(input);
    const headers = new Headers(init.headers);
    const body = typeof init.body === "string" ? JSON.parse(init.body) : undefined;
    const call = { url, method: init.method ?? "GET", headers, body, signal: init.signal ?? undefined };
    this.calls.push(call);
    const custom = await this.handle(call);
    if (custom) return custom;
    if (url === "/api/login") {
      return (body as { code: string }).code === "good"
        ? Response.json({ session_token: this.token })
        : Response.json({ error: "login_expired", message: "that link has expired" }, { status: 401 });
    }
    if (headers.get("authorization") !== `Bearer ${this.token}`) {
      return Response.json({ error: "unauthorized", message: "no session" }, { status: 401 });
    }
    return this.route(call, init.signal ?? undefined);
  };

  /** The requests made so far, as `METHOD url`. */
  requests(): string[] {
    return this.calls.map((c) => `${c.method} ${c.url}`);
  }

  private route(call: Call, signal: AbortSignal | undefined): Response {
    const { url, method, body } = call;
    const path = url.split("?")[0];
    const query = new URLSearchParams(url.split("?")[1] ?? "");
    const recorded = (name: string) =>
      hasFixture(name)
        ? new Response(fixture(name), { headers: { "content-type": "application/json" } })
        : Response.json({ error: "not_found", message: `nothing at ${url}` }, { status: 404 });
    let m: RegExpMatchArray | null;
    if (method === "GET") {
      if (path === "/api/status") return recorded("status.json");
      if (path === "/api/sessions") return Response.json(this.sessions);
      if ((m = path.match(/^\/api\/sessions\/([^/]+)\/events$/))) {
        const session = decodeURIComponent(m[1]);
        const attempt = (this.feedAttempts[session] = (this.feedAttempts[session] ?? -1) + 1);
        return this.stream(this.feed(session, call.headers.get("last-event-id"), attempt), signal);
      }
      if ((m = path.match(/^\/api\/runs\/(\d+)$/))) return recorded(`show-${m[1]}.json`);
      if ((m = path.match(/^\/api\/decisions\/([\d.]+)$/))) return recorded(`explain-${m[1]}.json`);
      if ((m = path.match(/^\/api\/compare\/(\d+)\/(\d+)$/))) return recorded(`compare-${m[1]}-${m[2]}.json`);
      if (path === "/api/changes") return recorded(query.has("version") ? `changes-v${query.get("version")}.json` : "changes.json");
      if (path === "/api/packages/docs") {
        if (query.get("package") === "@pied-piper/billing") {
          const markdown =
            "# @pied-piper/billing\n\n`listCharges(customerId)` returns the charges on one customer's account.\n\n" +
            "The starter data has two charges for `pied_piper` and one for `hooli`.\n\n" +
            '```ts\nimport { listCharges } from "@pied-piper/billing";\n\nconst charges = listCharges("pied_piper");\n```\n';
          return Response.json({ package: "@pied-piper/billing", bytes: markdown.length, truncated: false, note: null, untrusted: { markdown } });
        }
        return Response.json({ error: "unknown_package", message: "package is not in the blueprint" }, { status: 404 });
      }
      if (path === "/api/packages/source") {
        if (query.get("package") === "@pied-piper/billing" && query.get("module") === "lib") {
          return Response.json({
            package: "@pied-piper/billing",
            module: "lib",
            bytes: 30,
            truncated: false,
            note: null,
            untrusted: { text: "export function listCharges() {}" },
          });
        }
        return Response.json(
          { error: "unknown_module", message: "package `@pied-piper/billing` has no module `nope`" },
          { status: 404 },
        );
      }
    }
    if (method === "POST") {
      const b = body as { run?: number; mode?: string; session?: string };
      if (path === "/api/test") {
        if (b.run === 999999) return recorded("test-unknown-run.json");
        return recorded(b.mode === "reads-live" ? "test-reads-live.json" : "test.json");
      }
      if (path === "/api/rerun") return recorded("rerun.json");
      if (path === "/api/cancel") return recorded("cancel-not-in-flight.json");
      if (path === "/api/draft-rule") return recorded("draft-rule.json");
      if (path === "/api/recheck") return recorded("recheck.json");
      if (path === "/api/agent") return recorded(b.session ? "agent-continue.json" : "agent-new.json");
    }
    return Response.json({ error: "not_found", message: `nothing at ${url}` }, { status: 404 });
  }

  private stream(reply: FeedReply, signal: AbortSignal | undefined): Response {
    const bytes = new TextEncoder().encode(reply.text);
    const size = reply.chunk ?? 700;
    let at = 0;
    const body = new ReadableStream<Uint8Array>({
      start(controller) {
        signal?.addEventListener("abort", () => {
          try {
            controller.error(new DOMException("aborted", "AbortError"));
          } catch {
            // already closed
          }
        });
      },
      pull(controller) {
        if (at < bytes.length) {
          controller.enqueue(bytes.slice(at, at + size));
          at += size;
          return;
        }
        if (reply.close) controller.close();
        // Otherwise stay open, as a live session's feed does.
        return new Promise(() => {});
      },
    });
    return new Response(body, { headers: { "content-type": "text/event-stream" } });
  }
}
