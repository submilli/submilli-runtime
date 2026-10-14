// The page over a real store and the recorded answers: what the developer sees and
// what each control sends.
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, expect, test, vi } from "vitest";

import { App } from "../App";
import { SESSION_KEY } from "../api/client";
import { FakeServer, SESSION, fixture, fixtureJson, type Call } from "../api/testing/fakeServer";
import type { SessionList, Show } from "../api/types";
import { STEPPED_BACK } from "../model/sessions";
import { createStore, type PageStore } from "../store/store";

let store: PageStore | null = null;
let scrolled: { el: Element; options: ScrollIntoViewOptions | boolean | undefined }[] = [];

beforeEach(() => {
  scrolled = [];
  // jsdom lays nothing out, so it has no scrollIntoView; record what was asked instead.
  Element.prototype.scrollIntoView = function (this: Element, options?: ScrollIntoViewOptions | boolean) {
    scrolled.push({ el: this, options });
  };
});

afterEach(() => {
  cleanup();
  store?.dispose();
  store = null;
  localStorage.clear();
  sessionStorage.clear();
  history.replaceState(null, "", "/");
});

const json = (body: unknown) => Response.json(body);

async function openPage(server = new FakeServer()) {
  localStorage.setItem(SESSION_KEY, server.token);
  store = createStore({ fetch: server.fetch, frame: (cb) => setTimeout(cb, 0), pollMs: 5, retryDelay: () => 0 });
  const user = userEvent.setup();
  await act(async () => {
    render(<App store={store as PageStore} />);
    await (store as PageStore).start();
  });
  return { server, store: store as PageStore, user };
}

/** The feed's recording in its server-sent-event blocks, each with its blank line. */
function blocks(name: string): string[] {
  return fixture(name)
    .split("\n\n")
    .filter((b) => b.trim())
    .map((b) => b + "\n\n");
}

/** A session feed the test sends on, one block at a time, as a live session's would. */
function liveFeed(server: FakeServer, session: string) {
  let controller: ReadableStreamDefaultController<Uint8Array> | null = null;
  const pending: string[] = [];
  const previous = server.handle;
  server.handle = (call: Call) => {
    if (call.url === `/api/sessions/${session}/events`) {
      const body = new ReadableStream<Uint8Array>({
        start(c) {
          controller = c;
          for (const text of pending.splice(0)) c.enqueue(new TextEncoder().encode(text));
        },
      });
      return new Response(body, { headers: { "content-type": "text/event-stream" } });
    }
    return previous(call);
  };
  return {
    async send(...texts: string[]) {
      await act(async () => {
        for (const text of texts) {
          if (controller) controller.enqueue(new TextEncoder().encode(text));
          else pending.push(text);
        }
        await new Promise((r) => setTimeout(r, 20));
      });
    },
  };
}

const posts = (server: FakeServer) => server.calls.filter((c) => c.method === "POST");
const logPane = () => screen.getByRole("region", { name: "Log" });
const codeRegion = () => screen.getAllByRole("region", { name: "Code" })[0];
const row = (id: string) => {
  const el = document.getElementById("pgrow-" + id);
  if (!el) throw new Error("no row " + id);
  return el as HTMLElement;
};
const rowHead = (id: string) => row(id).querySelector(".pg-event-head") as HTMLElement;
const runsLoaded = () => screen.findByText("Run 4 · the agent’s 4th program", {}, { timeout: 3000 });

test("stepping through a finished session sends no action request", async () => {
  const server = new FakeServer();
  const ended = 'event: session-ended\ndata: {"kind":"session-ended","session":"' + SESSION(4) + '"}\n\n';
  server.feed = () => ({ text: fixture("feed-session-4.sse") + ended, close: true });
  const { user } = await openPage(server);
  await runsLoaded();

  await user.click(screen.getByRole("button", { name: "From the start" }));
  expect(screen.getByText(/^0 \/ \d+$/)).toBeInTheDocument();
  await user.keyboard("{ArrowRight}{ArrowRight}{ArrowRight}");
  expect(screen.getByText(/^3 \/ \d+$/)).toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: "Previous event" }));
  await user.click(screen.getByRole("button", { name: "Next event" }));
  await user.click(screen.getByRole("button", { name: "Next event" }));
  expect(screen.getByText(/^4 \/ \d+$/)).toBeInTheDocument();
  expect(screen.getByText("Stepping through · read-only")).toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: "Back to latest" }));
  expect(screen.getByText("At the latest event")).toBeInTheDocument();

  expect(posts(server)).toEqual([]);
});

test("stepping into a package call shows the package's source with the active line centred", async () => {
  const server = new FakeServer();
  server.handle = (call) => (call.url.startsWith("/api/packages/source") ? json(fixtureJson("model-package-source.json")) : undefined);
  const { user } = await openPage(server);
  await runsLoaded();

  await user.click(screen.getByRole("button", { name: "From the start" }));
  // The program's line that calls into the package, then the package's own line.
  await user.keyboard("{ArrowRight}{ArrowRight}{ArrowRight}");
  expect(within(codeRegion()).getByText("Line 4 calls into @acme/billing (listCharges). Step → to follow it into the package’s code.")).toBeInTheDocument();
  await user.keyboard("{ArrowRight}");

  const code = codeRegion();
  await within(code).findAllByText('check("acme.com/charges.list", { customerId });');
  const active = code.querySelector(".pg-line.active") as HTMLElement;
  expect(active.querySelector(".num")?.textContent).toBe("28");
  expect(active.querySelector(".txt")?.textContent).toBe('    check("acme.com/charges.list", { customerId });');
  expect(within(code).getByText("@acme/billing · lib · called from program 1, line 4")).toBeInTheDocument();
  expect(within(code).getByText(/as installed now/)).toBeInTheDocument();
  expect(server.requests()).toContain("GET /api/packages/source?package=%40acme%2Fbilling&module=lib");
  const centred = scrolled.filter((s) => s.el === active);
  expect(centred.at(-1)?.options).toEqual({ block: "center" });
});

test("a missing package source says so in the code pane, in the server's words", async () => {
  const server = new FakeServer();
  server.handle = (call) =>
    call.url.startsWith("/api/packages/source")
      ? Response.json({ error: "unknown_module", message: "package `@acme/billing` has no module `lib`" }, { status: 404 })
      : undefined;
  const { user } = await openPage(server);
  await runsLoaded();
  await user.click(screen.getByRole("button", { name: "From the start" }));
  await user.keyboard("{ArrowRight}{ArrowRight}{ArrowRight}{ArrowRight}");
  expect(await within(codeRegion()).findByText("@acme/billing · lib: its source isn’t available.")).toBeInTheDocument();
  expect(within(codeRegion()).getByText("package @acme/billing has no module lib")).toBeInTheDocument();
});

test("while stepped back, test, rerun, draft and chat say why they're off, and Cancel still cancels", async () => {
  const server = new FakeServer();
  const feed = liveFeed(server, SESSION(4));
  const all = blocks("feed-session-4.sse");
  // Run 4 has started and is making its calls; it hasn't finished.
  const finish4 = all.findIndex((b) => b.includes('"run":4') && b.includes("run-finished"));
  server.handle = ((inner) => (call: Call) => {
    if (call.url.startsWith("/api/runs/4")) return Response.json({ error: "not_found", message: "run 4 is still running" }, { status: 404 });
    if (call.url === "/api/cancel") return json({ exit: 0, result: { run: 4 }, text: "" });
    return inner(call);
  })(server.handle);
  const { user } = await openPage(server);
  await feed.send(...all.slice(0, finish4));
  await screen.findByText("Run 3 · the agent’s 3rd program");

  await user.click(screen.getByRole("button", { name: "Previous event" }));
  await user.click(rowHead("r2"));
  for (const name of ["Draft a rule for check 2.1", "Test against blueprint v2", "Rerun for real…"]) {
    const button = within(row("r2")).getByRole("button", { name });
    expect(button).toHaveAttribute("aria-disabled", "true");
    expect(button).toHaveAttribute("title", STEPPED_BACK);
    expect(button).toHaveAccessibleDescription(STEPPED_BACK);
    await user.click(button);
  }
  const ask = screen.getByRole("textbox", { name: "Message the agent" });
  expect(ask).toHaveAttribute("aria-disabled", "true");
  const send = screen.getByRole("button", { name: "Send" });
  expect(send).toHaveAttribute("aria-disabled", "true");
  expect(send).toHaveAttribute("title", STEPPED_BACK);
  expect(posts(server)).toEqual([]);

  const cancel = within(screen.getByRole("region", { name: "Session" })).getByRole("button", { name: "Cancel run 4" });
  expect(cancel).not.toHaveAttribute("aria-disabled");
  await user.click(cancel);
  expect(posts(server).map((c) => [c.url, c.body])).toEqual([["/api/cancel", { run: 4 }]]);
});

test("live test and rerun ask first and send nothing until confirmed; real reads goes at once", async () => {
  const server = new FakeServer();
  const show2 = fixtureJson<Show>("show-2.json");
  show2.reruns.push({ run: 8, kind: "test", blueprint_version: "2", outcome: { kind: "stopped" } });
  server.handle = (call) => {
    if (call.url.startsWith("/api/runs/2?")) return json(show2);
    if (call.url.startsWith("/api/runs/8?")) return json(fixtureJson("model-show-8-stopped.json"));
    if (call.url === "/api/compare/2/8") return json(fixtureJson("model-compare-2-8.json"));
    return undefined;
  };
  const { user } = await openPage(server);
  await runsLoaded();

  await user.click(rowHead("r2"));
  await user.click(within(row("r2")).getByRole("button", { name: "Rerun for real…" }));
  const dialog = screen.getByRole("dialog", { name: "Rerun run 2 for real?" });
  expect(posts(server)).toEqual([]);
  await user.click(within(dialog).getByRole("button", { name: "Rerun for real" }));
  expect(posts(server).map((c) => [c.url, c.body])).toEqual([["/api/rerun", { run: 2 }]]);

  // The rerun shows under run 2, so its follow-ups are open now.
  await expect.poll(() => document.getElementById("pgrow-f8")).not.toBeNull();
  expect(within(row("r2")).getByText("Reruns by you (3)")).toBeInTheDocument();
  await user.click(rowHead("f8"));
  await user.click(within(row("f8")).getByRole("button", { name: "Test it live…" }));
  const live = screen.getByRole("dialog", { name: "Test run 2 live?" });
  expect(posts(server)).toHaveLength(1);
  await user.click(within(live).getByRole("button", { name: "Don’t run" }));
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  expect(posts(server)).toHaveLength(1);
  await user.click(within(row("f8")).getByRole("button", { name: "Test it live…" }));
  await user.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Test it live" }));
  expect(posts(server).at(-1)?.body).toEqual({ run: 2, mode: "live" });

  await user.click(within(row("f8")).getByRole("button", { name: "Test again with real reads" }));
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  expect(posts(server).at(-1)?.body).toEqual({ run: 2, mode: "reads-live" });
});

test("the sessions dialog lists sessions newest first, and Open loads one into the panes", async () => {
  const server = new FakeServer();
  const list = fixtureJson<SessionList>("sessions.json");
  // Session 1 as an earlier chat of its own, so the dialog lists it.
  const earlier = list.sessions.shift();
  list.sessions.push({ ...earlier!, sources: ["chat"], started_at: "2026-10-13T13:20:00Z", started_at_micros: 1791897600000000 });
  server.sessions = list;
  const { user } = await openPage(server);
  await runsLoaded();

  await user.click(screen.getByRole("button", { name: "Sessions" }));
  const dialog = screen.getByRole("dialog", { name: "Sessions" });
  const rows = within(dialog).getAllByRole("row").slice(1);
  expect(rows).toHaveLength(2);
  expect(within(rows[0]).getByText(/^4 runs/)).toBeInTheDocument();
  expect(within(rows[0]).getByText("Open now")).toBeInTheDocument();
  expect(within(rows[1]).getByText("1 run")).toBeInTheDocument();
  expect(within(rows[1]).getByText("Oct 13, 13:20 UTC")).toBeInTheDocument();

  await user.click(within(rows[1]).getByRole("button", { name: "Open" }));
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  expect(await within(logPane()).findByText("Run 7 · your rerun of run 1")).toBeInTheDocument();
  expect(within(logPane()).queryByText("Run 4 · the agent’s 4th program")).not.toBeInTheDocument();
  expect(within(codeRegion()).getByText("Run 7 ran this same program, from run 1.")).toBeInTheDocument();
  expect(server.requests()).toContain(`GET /api/sessions/${SESSION(1)}/events`);
});

test("focus view hides the session bar and summary, and Escape leaves it after closing a dialog", async () => {
  const { user } = await openPage();
  await runsLoaded();
  await user.click(screen.getByRole("button", { name: "More" }));
  await user.click(within(screen.getByRole("dialog", { name: "More" })).getByRole("button", { name: "Focus view" }));
  expect(screen.queryByRole("region", { name: "Session" })).not.toBeInTheDocument();
  expect(screen.queryByRole("region", { name: "Session summary" })).not.toBeInTheDocument();
  expect(screen.getByRole("region", { name: "Chat" })).toBeInTheDocument();

  const sessions = screen.getByRole("button", { name: "Sessions" });
  await user.click(sessions);
  expect(screen.getByRole("dialog", { name: "Sessions" })).toBeInTheDocument();
  await user.keyboard("{Escape}");
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  expect(sessions).toHaveFocus();
  expect(screen.getByRole("button", { name: "Exit focus view" })).toBeInTheDocument();
  await user.keyboard("{Escape}");
  expect(screen.getByRole("region", { name: "Session" })).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Exit focus view" })).not.toBeInTheDocument();
});

test("a reader at the latest event stays there; one scrolled up isn't moved and gets Back to latest", async () => {
  const server = new FakeServer();
  const feed = liveFeed(server, SESSION(4));
  const all = blocks("feed-session-4.sse");
  const { user } = await openPage(server);
  await feed.send(...all.slice(0, 12));
  await screen.findByText("Run 1 · the agent’s 1st program");

  const log = logPane().querySelector(".pg-log") as HTMLElement;
  let height = 1000;
  Object.defineProperty(log, "scrollHeight", { configurable: true, get: () => height });
  Object.defineProperty(log, "clientHeight", { configurable: true, get: () => 100 });
  height = 1200;
  await feed.send(...all.slice(12, 20));
  await screen.findByText("Run 2 · the agent’s 2nd program");
  expect(log.scrollTop).toBe(1200);
  expect(screen.queryByRole("button", { name: "Back to latest" })).not.toBeInTheDocument();

  // Scrolling up to read isn't being behind; something new arriving meanwhile is.
  log.scrollTop = 300;
  fireEvent.scroll(log);
  expect(screen.queryByRole("button", { name: "Back to latest" })).not.toBeInTheDocument();
  height = 1500;
  await feed.send(...all.slice(20, 26));
  await screen.findByText("Run 3 · the agent’s 3rd program");
  expect(log.scrollTop).toBe(300);
  expect(screen.getByRole("button", { name: "Back to latest" })).toBeInTheDocument();

  await user.click(screen.getByRole("button", { name: "Back to latest" }));
  expect(log.scrollTop).toBe(1500);
  expect(screen.queryByRole("button", { name: "Back to latest" })).not.toBeInTheDocument();
});

test("test from a refusal row starts a test run, which shows under its source run when it finishes", async () => {
  const server = new FakeServer();
  let tested = false;
  server.handle = (call) => {
    if (!call.url.startsWith("/api/runs/2?")) return undefined;
    const show = fixtureJson<Show>("show-2.json");
    // Until the test is done, run 2 has no follow-ups; then it lists the one the test made.
    show.reruns = tested ? show.reruns.filter((r) => r.run === 5) : [];
    return json(show);
  };
  const inner = server.handle;
  server.handle = (call) => {
    if (call.url === "/api/test") tested = true;
    return inner(call);
  };
  const { user } = await openPage(server);
  await runsLoaded();
  await user.click(rowHead("r2"));
  expect(within(row("r2")).queryByText(/^Reruns by you \(/)).not.toBeInTheDocument();

  await user.click(within(row("r2")).getByRole("button", { name: "Test against blueprint v2" }));
  expect(posts(server).map((c) => [c.url, c.body])).toEqual([["/api/test", { run: 2 }]]);
  expect(await within(row("r2")).findByText("Reruns by you (1)")).toBeInTheDocument();
  expect(within(row("f5")).getByText("· Test of blueprint v2")).toBeInTheDocument();
  expect(within(row("f5")).getByText("Completed")).toBeInTheDocument();
  // Focus moves from the pressed button on to the new row.
  await vi.waitFor(() => expect(rowHead("f5")).toHaveFocus());
});

test("a rerun's row opens its own session to step through, and Back returns to the row", async () => {
  const { server, user } = await openPage();
  await runsLoaded();
  await user.click(rowHead("r1"));
  await user.click(within(row("r1")).getByText(/^Reruns by you \(/));
  await user.click(rowHead("f7"));
  await user.click(within(row("f7")).getByRole("button", { name: "Step through this rerun" }));

  await expect.poll(() => server.requests()).toContain(`GET /api/sessions/${SESSION(1)}/events`);
  const chat = screen.getByRole("region", { name: "Chat" });
  expect(await within(chat).findByText(/No chat: this session is run 7, a rerun of run 1 that you started\. The agent never saw it\./)).toBeInTheDocument();
  expect(within(chat).getByText("A rerun takes no messages.")).toBeInTheDocument();
  // Opened at the rerun.
  expect(row("r7").className).toContain("selected");

  await user.click(within(chat).getByRole("button", { name: "Back to session " + SESSION(4).slice(0, 8) }));
  await expect.poll(() => document.getElementById("pgrow-f7")?.className ?? "").toContain("selected");
  expect(within(chat).queryByRole("button", { name: /^Back to session/ })).not.toBeInTheDocument();
});

test("a refused action shows the server's message in the toast", async () => {
  const server = new FakeServer();
  server.handle = (call) => (call.url === "/api/test" ? new Response(fixture("test-unknown-run.json")) : undefined);
  const { user } = await openPage(server);
  await runsLoaded();
  await user.click(rowHead("r1"));
  await user.click(within(row("r1")).getByRole("button", { name: "Test against blueprint v2" }));
  const message = fixtureJson<{ result: { error: { message: string } } }>("test-unknown-run.json").result.error.message;
  // The server's own words, as a sentence, with its commands shown as code.
  const shown = (message.charAt(0).toUpperCase() + message.slice(1)).replaceAll("`", "");
  expect((await screen.findByRole("status")).textContent).toContain(shown);
});

test("draft a rule shows the drafted rule, where it would go, and its copy lines, with no save", async () => {
  const { server, user } = await openPage();
  await runsLoaded();
  await user.click(rowHead("r2"));
  await user.click(within(row("r2")).getByRole("button", { name: "Draft a rule for check 2.1" }));
  expect(posts(server).map((c) => [c.url, c.body])).toEqual([["/api/draft-rule", { decision: "2.1" }]]);
  const dialog = await screen.findByRole("dialog", { name: "Draft a rule for check 2.1" });
  expect(within(dialog).getByText(/filter: customerId == "cus_globex"/)).toBeInTheDocument();
  expect(within(dialog).getByText(/submilli\/blueprints\/billing\.yaml, lines 39–41/)).toBeInTheDocument();
  expect(within(dialog).getByRole("button", { name: "Copy for assistant" })).toBeInTheDocument();
  await user.click(within(dialog).getByRole("button", { name: /Copy CLI command/ }));
  expect(await navigator.clipboard.readText()).toBe("submilli playground draft-rule 2.1 --write");
  expect(within(dialog).queryByRole("button", { name: /save/i })).not.toBeInTheDocument();
});

test("Cancel on a running run's row ends it as cancelled once the server confirms", async () => {
  const server = new FakeServer();
  const feed = liveFeed(server, SESSION(4));
  const all = blocks("feed-session-4.sse");
  const finish4 = all.findIndex((b) => b.includes('"run":4') && b.includes("run-finished"));
  let cancelled = false;
  server.handle = ((inner) => (call: Call) => {
    if (call.url === "/api/cancel") {
      cancelled = true;
      return json({ exit: 0, result: { run: 4 }, text: "" });
    }
    if (call.url.startsWith("/api/runs/4?")) {
      if (!cancelled) return Response.json({ error: "not_found", message: "still running" }, { status: 404 });
      const show = fixtureJson<Show>("show-4.json");
      show.outcome = { kind: "cancelled" };
      return json(show);
    }
    return inner(call);
  })(server.handle);
  const { user } = await openPage(server);
  await feed.send(...all.slice(0, finish4));
  const note = await within(logPane()).findByText(/Run 4, the agent’s 4th program, is running/);
  await user.click(within(note.closest(".pg-running") as HTMLElement).getByRole("button", { name: "Cancel run 4" }));
  expect(posts(server).map((c) => [c.url, c.body])).toEqual([["/api/cancel", { run: 4 }]]);

  await feed.send(all[finish4]);
  expect(await within(row("r4")).findByText("Cancelled")).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Cancel run 4" })).not.toBeInTheDocument();
});

test("New session with variables and a model starts a browser chat, shown in the session bar", async () => {
  const server = new FakeServer();
  let sent = false;
  server.handle = (call) => {
    if (call.url === "/api/sessions") {
      const list = fixtureJson<SessionList>("sessions.json");
      if (sent) list.sessions.unshift({ ...list.sessions[3], session: SESSION(8), sources: ["browser-chat"], runs: [], run_count: 0 });
      return json(list);
    }
    if (call.url === "/api/agent") {
      sent = true;
      return new Response(fixture("agent-new.json").replaceAll(SESSION(4), SESSION(8)));
    }
    return undefined;
  };
  const { user } = await openPage(server);
  await runsLoaded();

  await user.click(screen.getByRole("button", { name: "New session" }));
  const field = screen.getByRole("textbox", { name: /customerId/ });
  await user.clear(field);
  await user.type(field, "cus_globex");
  const model = screen.getByRole("textbox", { name: /^Model/ });
  await user.clear(model);
  await user.type(model, "claude-haiku-5-5");
  await user.click(screen.getByRole("button", { name: "Start session" }));

  const bar = screen.getByRole("region", { name: "Session" });
  expect(within(bar).getByText("cus_globex")).toBeInTheDocument();
  expect(within(bar).getByText("Agent Claude · claude-haiku-5-5")).toBeInTheDocument();
  expect(within(bar).getByText("Not started")).toBeInTheDocument();
  expect(posts(server)).toEqual([]);

  await user.type(screen.getByRole("textbox", { name: "Message the agent" }), "Hello{Enter}");
  const agent = server.calls.find((c) => c.url === "/api/agent");
  expect(agent?.body).toEqual({ task: "Hello", model: "anthropic:claude-haiku-5-5", variables: { customerId: "cus_globex" } });
  await expect.poll(() => server.requests()).toContain(`GET /api/sessions/${SESSION(8)}/events`);
});

test("New session starts with the latest chat session's agent and settings, Advanced open when it has some", async () => {
  const server = new FakeServer();
  const list = fixtureJson<SessionList>("sessions.json");
  // The latest chat session on OpenAI with settings, and an older one on Claude.
  const chat = list.sessions[3];
  list.sessions[3] = { ...chat, model: "openai:gpt-6-luna", settings: { effort: "low", max_output: 2000, provider: { temperature: 0.2 } } };
  list.sessions.push({ ...chat, session: SESSION(9), started_at_micros: chat.started_at_micros - 1 });
  server.handle = (call) => (call.url === "/api/sessions" ? json(list) : undefined);
  const { user } = await openPage(server);
  await runsLoaded();
  await user.click(screen.getByRole("button", { name: "New session" }));
  expect(screen.getByRole("button", { name: /^OpenAI/ })).toHaveAttribute("aria-pressed", "true");
  expect(screen.getByRole("textbox", { name: /^Model/ })).toHaveValue("gpt-6-luna");
  expect(screen.getByRole("button", { name: "Low" })).toHaveAttribute("aria-pressed", "true");
  expect(screen.getByRole("textbox", { name: /^Max output/ })).toHaveValue("2000");
  const settings = screen.getByRole("textbox", { name: "Provider settings as JSON" });
  expect(settings).toHaveValue('{"temperature":0.2}');
  expect(settings).toBeVisible();
});

test("a rerun running from the page can be cancelled from its notice", async () => {
  const server = new FakeServer();
  let cancelled: (() => void) | null = null;
  const made = { ...fixtureJson<Show>("model-show-8-stopped.json"), outcome: { kind: "cancelled" } };
  server.handle = (call) => {
    if (call.url === "/api/rerun") return new Promise<Response>((resolve) => (cancelled = () => resolve(json({ exit: 1, result: made, text: "" }))));
    if (call.url === "/api/runs?limit=1") return json({ runs: [], running: [{ run: 8, source: "rerun", started_at_micros: Date.now() * 1000, session: "throwaway" }] });
    if (call.url === "/api/cancel") {
      cancelled?.();
      return json({ exit: 0, result: { kind: "cancel", run: 8 }, text: "" });
    }
    return undefined;
  };
  const { user } = await openPage(server);
  await runsLoaded();
  await user.click(rowHead("r2"));
  await user.click(within(row("r2")).getByRole("button", { name: "Rerun for real…" }));
  await user.click(within(screen.getByRole("dialog", { name: "Rerun run 2 for real?" })).getByRole("button", { name: "Rerun for real" }));
  const notice = (await screen.findByText("Running:")).closest(".pg-toast") as HTMLElement;
  await user.click(within(notice).getByRole("button", { name: "Cancel" }));
  await expect.poll(() => posts(server).map((c) => [c.url, c.body])).toContainEqual(["/api/cancel", { run: 8 }]);
  // The rerun ends, cancelled, and says so as any other.
  expect(await screen.findByText(/^Run 8 cancelled\. It’s under run 2/)).toBeInTheDocument();
});

test("Start session is blocked while a required variable is empty", async () => {
  const { server, user } = await openPage();
  await runsLoaded();
  await user.click(screen.getByRole("button", { name: "New session" }));
  await user.clear(screen.getByRole("textbox", { name: /customerId/ }));
  const start = screen.getByRole("button", { name: "Start session" });
  expect(start).toHaveAttribute("aria-disabled", "true");
  expect(screen.getByText("Still needed: customerId.")).toBeInTheDocument();
  await user.click(start);
  expect(screen.getByRole("button", { name: "Start session" })).toBeInTheDocument();
  expect(screen.getByText("Nothing is started until you press Start session.")).toBeInTheDocument();
  expect(posts(server)).toEqual([]);
});

test("the blueprint dialog shows the version a decision used, its rule lit, and that it isn't the one in force", async () => {
  const { server, user } = await openPage();
  await runsLoaded();
  await user.click(rowHead("r2"));
  await user.click(row("d2.1").querySelector(".pg-event-head") as HTMLElement);
  const inspect = await within(row("d2.1")).findByRole("button", { name: /^Blueprint v1, line \d+$/ });
  const line = Number(/line (\d+)/.exec(inspect.textContent || "")?.[1]);
  await user.click(inspect);

  const dialog = screen.getByRole("dialog", { name: "Blueprint billing · v1" });
  expect(await within(dialog).findByText(/^Highlighted: line \d+/)).toBeInTheDocument();
  const lit = dialog.querySelector(".pg-bp .pg-line.active");
  expect(lit?.querySelector(".num")?.textContent).toBe(String(line));
  expect(within(dialog).getByText("Not the version in force: v2 is in force now.")).toBeInTheDocument();
  expect(within(dialog).getByRole("button", { name: "v2 · in force" })).toBeInTheDocument();
  expect(server.requests()).toContain("GET /api/changes?version=1");
});

test("recorded text with HTML markup shows as literal text", async () => {
  const server = new FakeServer();
  const markup = "<img src=x onerror=alert(1)>";
  server.feed = () => ({ text: fixture("feed-session-4.sse").replace("Let me total your charges.", markup + " Let me total"), close: false });
  server.handle = (call) =>
    call.url.startsWith("/api/packages/source")
      ? json({ package: "@acme/billing", module: "lib", bytes: 40, truncated: false, note: null, untrusted: { text: "<script>alert(1)</script>" } })
      : undefined;
  const { user } = await openPage(server);
  await runsLoaded();

  expect(within(screen.getByRole("region", { name: "Chat" })).getByText(markup + " Let me total", { exact: false })).toBeInTheDocument();
  await user.click(rowHead("r4"));
  expect(within(row("r4")).getByText(/ch_0153 <b>12997<\/b>/)).toBeInTheDocument();
  await user.click(rowHead("r3"));
  expect(within(row("r3")).getByText(/--> <execute>/)).toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: "From the start" }));
  await user.keyboard("{ArrowRight}{ArrowRight}{ArrowRight}{ArrowRight}");
  expect(await within(codeRegion()).findByText("<script>alert(1)</script>")).toBeInTheDocument();

  expect(document.querySelector(".pg img, .pg script, .pg b")).toBeNull();
});

test("Copy for assistant puts the reference on the clipboard and shows the toast", async () => {
  const { user } = await openPage();
  await runsLoaded();
  await user.click(rowHead("r2"));
  await user.click(within(row("r2")).getAllByRole("button", { name: "Copy for assistant" })[0]);
  const line =
    "Submilli playground, run 2 (details - project: `billing-demo`, blueprint: `billing v1`, outcome: `denied at check 2.1`, see_with: `submilli playground show 2`)";
  expect(await navigator.clipboard.readText()).toBe(line);
  const toast = screen.getByRole("status");
  expect(within(toast).getByText("Copied line for your assistant:")).toBeInTheDocument();
  expect(within(toast).getByText(line)).toBeInTheDocument();
});

test("a failed copy keeps the toast up with the text to copy by hand", async () => {
  const { user } = await openPage();
  await runsLoaded();
  vi.spyOn(navigator.clipboard, "writeText").mockRejectedValue(new Error("denied"));
  await user.click(rowHead("r1"));
  await user.click(within(row("r1")).getAllByRole("button", { name: "Copy for assistant" })[0]);
  const toast = await screen.findByRole("status");
  expect(within(toast).getByText("Copy this line for your assistant:")).toBeInTheDocument();
});

// ---- what trying the page live found

test("a run's link selects and opens that run once its session shows", async () => {
  history.replaceState(null, "", "/#run=2");
  await openPage();
  await runsLoaded();
  await vi.waitFor(() => expect(row("r2").classList.contains("selected")).toBe(true));
  expect(rowHead("r2").getAttribute("aria-expanded")).toBe("true");
});

test("a link to a run that isn't recorded says so", async () => {
  history.replaceState(null, "", "/#run=999");
  await openPage();
  expect(await screen.findByText("Run 999 isn’t recorded in this playground.")).toBeInTheDocument();
});

test("a double click on Test sends one test", async () => {
  const server = new FakeServer();
  // A test takes a while; the second click comes while the first is still out.
  server.handle = (call) =>
    call.url === "/api/test" ? new Promise<Response>((r) => setTimeout(() => r(new Response(fixture("test.json"))), 100)) : undefined;
  const { user } = await openPage(server);
  await runsLoaded();
  await user.click(rowHead("r2"));
  await user.dblClick(within(row("r2")).getByRole("button", { name: "Test against blueprint v2" }));
  await vi.waitFor(() => expect(posts(server).map((c) => c.url)).toEqual(["/api/test"]));
});

test("New session from focus view shows the form", async () => {
  const { user } = await openPage();
  await runsLoaded();
  await user.click(screen.getByRole("button", { name: "More" }));
  await user.click(screen.getByRole("button", { name: "Focus view" }));
  await user.click(screen.getByRole("button", { name: "New session" }));
  expect(screen.getByRole("button", { name: "Start session" })).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Exit focus view" })).not.toBeInTheDocument();
});

test("Tab stays inside an open dialog", async () => {
  const { user } = await openPage();
  await runsLoaded();
  await user.click(screen.getByRole("button", { name: "Sessions" }));
  const dialog = screen.getByRole("dialog", { name: "Sessions" });
  for (let i = 0; i < 12; i++) {
    await user.tab();
    expect(dialog.contains(document.activeElement)).toBe(true);
  }
  await user.tab({ shift: true });
  expect(dialog.contains(document.activeElement)).toBe(true);
});

test("a form just opened shows no field as wrong until it's typed in or Start session is pressed", async () => {
  const server = new FakeServer();
  const status = fixtureJson<Record<string, unknown> & { binding: Record<string, unknown> }>("status.json");
  status.binding = { ...status.binding, variables: {} };
  server.handle = (call) => (call.url === "/api/status" ? json(status) : undefined);
  const { user } = await openPage(server);
  await runsLoaded();
  await user.click(screen.getByRole("button", { name: "New session" }));
  const field = screen.getByRole("textbox", { name: /customerId/ });
  expect(field).toHaveValue("");
  expect(screen.queryByText("Required.")).not.toBeInTheDocument();
  expect(field).not.toHaveAttribute("aria-invalid");
  await user.click(screen.getByRole("button", { name: "Start session" }));
  expect(screen.getByText("Required.")).toBeInTheDocument();
  expect(field).toHaveAttribute("aria-invalid", "true");
});

test("with no model key, every provider can be picked, and the form says how to add a key", async () => {
  const server = new FakeServer();
  const status = fixtureJson<Record<string, unknown>>("status.json");
  status.chat_models = [];
  status.model_keys = [
    { provider: "anthropic", variable: "ANTHROPIC_API_KEY", default_model: "claude-sonnet-5-5", from: null },
    { provider: "openai", variable: "OPENAI_API_KEY", default_model: "gpt-5.5", from: null },
    { provider: "google", variable: "GEMINI_API_KEY", default_model: "gemini-flash-latest", from: null },
  ];
  server.handle = (call) => (call.url === "/api/status" ? json(status) : undefined);
  const { user } = await openPage(server);
  await runsLoaded();
  await user.click(screen.getByRole("button", { name: "New session" }));
  expect(screen.getByText(/No model key yet\. Add one with Add a key, put it in the project’s \.env/)).toBeInTheDocument();
  const agent = within(screen.getByRole("group", { name: "Provider" }));
  await user.click(agent.getByRole("button", { name: /^Gemini/ }));
  expect(agent.getByRole("button", { name: /^Gemini/ })).toHaveAttribute("aria-pressed", "true");
  expect(screen.getByRole("textbox", { name: /^Model/ })).toHaveValue("gemini-flash-latest");
  expect(screen.getByText("Gemini needs a key first: use Add a key under Agent.")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Start session" })).toHaveAttribute("aria-disabled", "true");
});

test("a model key typed in the form's key dialog is saved, masked, and the provider shows", async () => {
  const server = new FakeServer();
  let saved = false;
  const status = () => {
    const s = fixtureJson<Record<string, unknown>>("status.json");
    s.chat_models = saved ? ["openai:gpt-5.5"] : [];
    s.model_keys = [
      { provider: "anthropic", variable: "ANTHROPIC_API_KEY", from: null },
      { provider: "openai", variable: "OPENAI_API_KEY", from: saved ? "secret_store" : null },
      { provider: "google", variable: "GEMINI_API_KEY", from: null },
    ];
    return s;
  };
  server.handle = (call) => {
    if (call.url === "/api/status") return json(status());
    if (call.url === "/api/model-key") {
      saved = true;
      return json({ exit: 0, result: { variable: "OPENAI_API_KEY", from: "secret_store" }, text: "" });
    }
    return undefined;
  };
  const { user } = await openPage(server);
  await runsLoaded();
  await user.click(screen.getByRole("button", { name: "New session" }));
  await user.click(screen.getByRole("button", { name: "Add a key…" }));
  const dialog = within(screen.getByRole("dialog", { name: "Model key" }));
  await user.click(dialog.getByRole("button", { name: "OpenAI" }));
  expect(dialog.getByText("No OpenAI key yet.")).toBeInTheDocument();
  const field = dialog.getByLabelText("OpenAI API key");
  expect(field).toHaveAttribute("type", "password");
  expect(field).toHaveAttribute("autocomplete", "off");
  await user.type(field, "sk-test-123");
  await user.click(dialog.getByRole("button", { name: "Save key" }));
  expect(server.calls.find((c) => c.url === "/api/model-key")?.body).toEqual({ provider: "openai", key: "sk-test-123" });
  expect(await screen.findByText("Saved the OpenAI key.")).toBeInTheDocument();
  expect(screen.queryByRole("dialog", { name: "Model key" })).not.toBeInTheDocument();
  // The provider is picked now, and the key is nowhere on the page.
  expect(await screen.findByRole("textbox", { name: /^Model/ })).toHaveValue("gpt-5.5");
  expect(document.body.textContent).not.toContain("sk-test-123");
  expect([...document.querySelectorAll("input")].some((i) => i.value === "sk-test-123")).toBe(false);
});

test("removing a saved model key asks once more before it goes", async () => {
  const server = new FakeServer();
  const status = fixtureJson<Record<string, unknown>>("status.json");
  status.chat_models = ["openai:gpt-5.5"];
  status.model_keys = [
    { provider: "anthropic", variable: "ANTHROPIC_API_KEY", default_model: "claude-sonnet-5-5", from: null },
    { provider: "openai", variable: "OPENAI_API_KEY", default_model: "gpt-5.5", from: "secret_store" },
    { provider: "google", variable: "GEMINI_API_KEY", default_model: "gemini-flash-latest", from: null },
  ];
  server.handle = (call) => {
    if (call.url === "/api/status") return json(status);
    if (call.url === "/api/model-key") return json({ exit: 0, result: { variable: "OPENAI_API_KEY", from: null }, text: "" });
    return undefined;
  };
  const { user } = await openPage(server);
  await runsLoaded();
  await user.click(screen.getByRole("button", { name: "New session" }));
  // The form carries on the last session's agent, Claude here; the key is OpenAI's.
  await user.click(screen.getByRole("button", { name: "OpenAI" }));
  await user.click(screen.getByRole("button", { name: "Replace or remove the OpenAI key…" }));
  const dialog = within(screen.getByRole("dialog", { name: "Model key" }));
  expect(dialog.getByText("A saved OpenAI key is in use. Saving replaces it.")).toBeInTheDocument();
  await user.click(dialog.getByRole("button", { name: "Remove saved key…" }));
  expect(dialog.getByText("Remove the saved OpenAI key? Chat can’t use OpenAI until you add a key again.")).toBeInTheDocument();
  // Keep it goes back, with nothing sent.
  await user.click(dialog.getByRole("button", { name: "Keep it" }));
  expect(server.calls.some((c) => c.url === "/api/model-key")).toBe(false);
  await user.click(dialog.getByRole("button", { name: "Remove saved key…" }));
  await user.click(dialog.getByRole("button", { name: "Remove key" }));
  expect(server.calls.find((c) => c.url === "/api/model-key")?.body).toEqual({ provider: "openai", key: null });
  expect(await screen.findByText("Removed the saved OpenAI key.")).toBeInTheDocument();
});

test("a blueprint edit the playground refused is said, with the version that stays", async () => {
  const server = new FakeServer();
  const status = fixtureJson<Record<string, unknown> & { blueprint_status: Record<string, unknown> }>("status.json");
  status.blueprint_status = { ...status.blueprint_status, refused: { code: "parse_error", message: "duplicate field `permissions`", line: 30 } };
  server.handle = (call) => (call.url === "/api/status" ? json(status) : undefined);
  await openPage(server);
  const note = await screen.findByText((_, el) => el?.getAttribute("role") === "note" && /wasn’t taken/.test(el.textContent ?? ""));
  expect(note.textContent).toContain("duplicate field permissions (line 30). Version 2 stays in force, and runs use it.");
});

test("Shift+Enter starts a new line in the message; Enter sends it, lines and all", async () => {
  const server = new FakeServer();
  server.handle = (call) => (call.url === "/api/agent" ? json({ exit: 1, result: { error: { kind: "failure", message: "no" } }, text: "" }) : undefined);
  const { user } = await openPage(server);
  await runsLoaded();
  await user.click(screen.getByRole("button", { name: "New session" }));
  await user.click(screen.getByRole("button", { name: "Start session" }));
  const box = screen.getByRole("textbox", { name: "Message the agent" });
  await user.type(box, "First line{Shift>}{Enter}{/Shift}second line");
  expect(box).toHaveValue("First line\nsecond line");
  expect(server.calls.some((c) => c.url === "/api/agent")).toBe(false);
  await user.type(box, "{Enter}");
  await vi.waitFor(() => expect(server.calls.find((c) => c.url === "/api/agent")?.body).toMatchObject({ task: "First line\nsecond line" }));
});

test("a first message the agent couldn't answer shows in the chat, with what to do", async () => {
  const server = new FakeServer();
  server.handle = (call) =>
    call.url === "/api/agent"
      ? json({ exit: 1, result: { error: { kind: "failure", message: "openai refused the request (HTTP 404): The model `nope` does not exist" } }, text: "" })
      : undefined;
  const { user } = await openPage(server);
  await runsLoaded();
  await user.click(screen.getByRole("button", { name: "New session" }));
  await user.click(screen.getByRole("button", { name: "Start session" }));
  await user.type(screen.getByRole("textbox", { name: "Message the agent" }), "Hello{Enter}");
  const failed = await screen.findByRole("alert");
  expect(failed.textContent).toContain("The agent couldn’t answer: openai refused the request (HTTP 404): The model nope does not exist.");
  expect(failed.textContent).toContain("Nothing was started.");
  // The message is kept, to send again.
  expect(screen.getByRole("textbox", { name: "Message the agent" })).toHaveValue("Hello");
});

test("Stop gives up the message the agent is answering, and says so", async () => {
  const server = new FakeServer();
  // The answer never comes; the request ends only when the page gives it up.
  server.handle = (call) =>
    call.url === "/api/agent"
      ? new Promise<Response>((_, reject) => call.signal?.addEventListener("abort", () => reject(new DOMException("aborted", "AbortError"))))
      : undefined;
  const { user } = await openPage(server);
  await runsLoaded();
  await user.type(screen.getByRole("textbox", { name: "Message the agent" }), "Take your time{Enter}");
  expect(screen.queryByRole("button", { name: "Send" })).not.toBeInTheDocument();
  await user.click(await screen.findByRole("button", { name: "Stop" }));
  expect(await screen.findByText(/You stopped the agent\. It stops after the step it’s on/)).toBeInTheDocument();
  expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  // Sent, so it's in the session already: the box doesn't get it back.
  expect(screen.getByRole("textbox", { name: "Message the agent" })).toHaveValue("");
  expect(screen.getByRole("button", { name: "Send" })).toBeInTheDocument();
});

test("Stop on a new session's first message leaves it in the session it started, not in the box", async () => {
  const server = new FakeServer();
  let sent = false;
  server.handle = (call) => {
    if (call.url === "/api/sessions") {
      const list = fixtureJson<SessionList>("sessions.json");
      if (sent) list.sessions.unshift({ ...list.sessions[3], session: SESSION(8), sources: ["browser-chat"], runs: [], run_count: 0 });
      return json(list);
    }
    if (call.url === "/api/agent") {
      sent = true;
      return new Promise<Response>((_, reject) => call.signal?.addEventListener("abort", () => reject(new DOMException("aborted", "AbortError"))));
    }
    return undefined;
  };
  const { user } = await openPage(server);
  await runsLoaded();
  await user.click(screen.getByRole("button", { name: "New session" }));
  await user.click(screen.getByRole("button", { name: "Start session" }));
  await user.type(screen.getByRole("textbox", { name: "Message the agent" }), "Hello{Enter}");
  // The session it started is listed and opened while the agent answers.
  await expect.poll(() => server.requests()).toContain(`GET /api/sessions/${SESSION(8)}/events`);
  await user.click(await screen.findByRole("button", { name: "Stop" }));
  expect(await screen.findByText(/You stopped the agent\. It stops after the step it’s on/)).toBeInTheDocument();
  expect(screen.getByRole("textbox", { name: "Message the agent" })).toHaveValue("");
});

test("a message the agent couldn't answer still says so after a reload", async () => {
  const server = new FakeServer();
  server.handle = (call) =>
    call.url === "/api/agent" ? json({ exit: 1, result: { error: { kind: "failure", message: "the provider timed out" } }, text: "" }) : undefined;
  const { user } = await openPage(server);
  await runsLoaded();
  await user.type(screen.getByRole("textbox", { name: "Message the agent" }), "Again?{Enter}");
  expect((await screen.findByRole("alert")).textContent).toContain("Send again");
  cleanup();
  store?.dispose();
  await openPage(new FakeServer());
  await runsLoaded();
  expect((await screen.findByRole("alert")).textContent).toContain("The agent couldn’t answer: the provider timed out.");
});

test("a link to a repeated check opens the row it's listed under", async () => {
  const server = new FakeServer();
  const show = fixtureJson<Show>("show-1.json");
  show.decisions[0] = { ...show.decisions[0], refs: ["1.1", "1.3"] };
  server.handle = (call) => (call.url.startsWith("/api/runs/1?") ? json(show) : undefined);
  history.replaceState(null, "", "/#run=1&decision=3");
  await openPage(server);
  await runsLoaded();
  await vi.waitFor(() => expect(row("d1.1").classList.contains("selected")).toBe(true));
});

test("a click outside More closes it", async () => {
  const { user } = await openPage();
  await runsLoaded();
  await user.click(screen.getByRole("button", { name: "More" }));
  expect(screen.getByRole("dialog", { name: "More" })).toBeInTheDocument();
  await user.click(screen.getByRole("dialog", { name: "More" }));
  expect(screen.getByRole("dialog", { name: "More" })).toBeInTheDocument();
  await user.click(rowHead("r2"));
  expect(screen.queryByRole("dialog", { name: "More" })).not.toBeInTheDocument();
});

test("a session started but not yet sent to is still there after a reload", async () => {
  const { user } = await openPage();
  await runsLoaded();
  await user.click(screen.getByRole("button", { name: "New session" }));
  await user.click(screen.getByRole("button", { name: "Start session" }));
  expect(screen.getByText("New session · send its first message")).toBeInTheDocument();
  cleanup();
  store?.dispose();
  await openPage();
  expect(await screen.findByText("New session · send its first message")).toBeInTheDocument();
  expect(screen.getByRole("textbox", { name: "Message the agent" })).not.toHaveAttribute("readonly");
});

test("arrow keys step while focus is in the read-only ask box", async () => {
  const { user } = await openPage();
  await runsLoaded();
  await user.click(screen.getByRole("button", { name: "From the start" }));
  const box = screen.getByRole("textbox", { name: "Message the agent" });
  expect(box).toHaveAttribute("readonly");
  await user.click(box);
  await user.keyboard("{ArrowRight}{ArrowRight}");
  expect(screen.getByText(/^2 \/ \d+$/)).toBeInTheDocument();
});

test("dismissing the unknown-run notice puts focus in the ask box", async () => {
  history.replaceState(null, "", "/#run=999");
  const { user } = await openPage();
  await screen.findByText("Run 999 isn’t recorded in this playground.");
  const focus = vi.spyOn(HTMLElement.prototype, "focus");
  await user.click(screen.getByRole("button", { name: "Dismiss" }));
  await vi.waitFor(() => expect(document.activeElement).toBe(screen.getByRole("textbox", { name: "Message the agent" })));
  // Without moving the page: the reader may have scrolled away from the box.
  expect(focus).toHaveBeenLastCalledWith({ preventScroll: true });
  focus.mockRestore();
});

test("a run's link is dropped from the address once another session opens or a new one is set up", async () => {
  history.replaceState(null, "", "/#run=2");
  const { user } = await openPage();
  await runsLoaded();
  await vi.waitFor(() => expect(row("r2").classList.contains("selected")).toBe(true));
  await user.click(screen.getByRole("button", { name: "New session" }));
  expect(location.hash).toBe("");
});
