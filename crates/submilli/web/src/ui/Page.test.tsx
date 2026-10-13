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
  expect(await within(logPane()).findByText("Run 7 · the agent’s 1st program")).toBeInTheDocument();
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

  log.scrollTop = 300;
  fireEvent.scroll(log);
  expect(screen.getByRole("button", { name: "Back to latest" })).toBeInTheDocument();
  height = 1500;
  await feed.send(...all.slice(20, 26));
  await screen.findByText("Run 3 · the agent’s 3rd program");
  expect(log.scrollTop).toBe(300);

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
});

test("a refused action shows the server's message in the toast", async () => {
  const server = new FakeServer();
  server.handle = (call) => (call.url === "/api/test" ? new Response(fixture("test-unknown-run.json")) : undefined);
  const { user } = await openPage(server);
  await runsLoaded();
  await user.click(rowHead("r1"));
  await user.click(within(row("r1")).getByRole("button", { name: "Test against blueprint v2" }));
  const message = fixtureJson<{ result: { error: { message: string } } }>("test-unknown-run.json").result.error.message;
  expect(within(await screen.findByRole("status")).getByText(message)).toBeInTheDocument();
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
  expect(within(bar).getByText("Live")).toBeInTheDocument();
  expect(posts(server)).toEqual([]);

  await user.type(screen.getByRole("textbox", { name: "Message the agent" }), "Hello{Enter}");
  const agent = server.calls.find((c) => c.url === "/api/agent");
  expect(agent?.body).toEqual({ task: "Hello", model: "anthropic:claude-haiku-5-5", variables: { customerId: "cus_globex" } });
  await expect.poll(() => server.requests()).toContain(`GET /api/sessions/${SESSION(8)}/events`);
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
    "Submilli playground run 2 in project billing-demo (chat, denied, blueprint billing v1). Details: submilli playground show 2";
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
