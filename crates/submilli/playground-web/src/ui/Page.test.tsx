// The page over a real store and the recorded answers: what the developer sees and
// what each control sends.
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, expect, test, vi } from "vitest";

import { App } from "../App";
import { SESSION_KEY } from "../api/client";
import { FakeServer, SESSION, fixture, fixtureJson, type Call } from "../api/testing/fakeServer";
import type { SessionList, Show, Status } from "../api/types";
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
const logPane = () => screen.getByRole("region", { name: "Execution" });
const codeRegion = () => screen.getAllByRole("region", { name: "Program" })[0];
const row = (id: string) => {
  const el = document.getElementById("pgrow-" + id);
  if (!el) throw new Error("no row " + id);
  return el as HTMLElement;
};
const rowHead = (id: string) => row(id).querySelector(".pg-event-head") as HTMLElement;
const runsLoaded = () => within(logPane()).findByText("Program 4 · Run 4", {}, { timeout: 3000 });

async function completeSetup(user: ReturnType<typeof userEvent.setup>) {
  const modelDone = screen.queryByRole("button", { name: "Done configuring model" });
  if (modelDone) await user.click(modelDone);
  const variablesDone = screen.queryByRole("button", { name: "Done preparing variables" });
  if (variablesDone) await user.click(variablesDone);
}

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

test("replay advances despite status refreshes, pauses, and stops at the end without actions", async () => {
  const { server } = await openPage();
  await runsLoaded();
  vi.useFakeTimers();
  try {
    fireEvent.click(screen.getByRole("button", { name: "Replay recording" }));
    expect(screen.getByText("0 / 20")).toBeInTheDocument();
    await act(async () => { await vi.advanceTimersByTimeAsync(500); });
    const statusReads = server.requests().filter((r) => r === "GET /api/status").length;
    await act(async () => { window.dispatchEvent(new Event("focus")); });
    expect(server.requests().filter((r) => r === "GET /api/status").length).toBeGreaterThan(statusReads);
    await act(async () => { await vi.advanceTimersByTimeAsync(400); });
    expect(screen.getByText("1 / 20")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Pause" }));
    await act(async () => { await vi.advanceTimersByTimeAsync(1800); });
    expect(screen.getByText("1 / 20")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Play recording" }));
    for (let i = 1; i < 20; i++) {
      await act(async () => { await vi.advanceTimersByTimeAsync(900); });
    }
    expect(screen.getByText("20 / 20")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Replay recording" })).toBeInTheDocument();
    await act(async () => { await vi.advanceTimersByTimeAsync(1800); });
    expect(screen.getByText("20 / 20")).toBeInTheDocument();
    expect(posts(server)).toEqual([]);
  } finally {
    vi.useRealTimers();
  }
});

test("opening another session stops playback", async () => {
  const { server, store: pageStore } = await openPage();
  await runsLoaded();
  vi.useFakeTimers();
  try {
    fireEvent.click(screen.getByRole("button", { name: "Replay recording" }));
    await act(async () => {
      await pageStore.openSession(SESSION(1));
      await vi.advanceTimersByTimeAsync(10);
    });
    expect(screen.queryByRole("button", { name: "Pause" })).not.toBeInTheDocument();
    const position = screen.getByText(/^\d+ \/ \d+$/).textContent;
    await act(async () => { await vi.advanceTimersByTimeAsync(1800); });
    expect(screen.getByText(position!)).toBeInTheDocument();
    expect(posts(server)).toEqual([]);
  } finally {
    vi.useRealTimers();
  }
});

test("the package dialog renders the package guide as readable Markdown", async () => {
  const { server, user } = await openPage();
  await runsLoaded();

  await user.click(screen.getByRole("button", { name: /Packages@pied-piper\/billing/ }));

  const dialog = screen.getByRole("dialog", { name: "Packages in billing v2" });
  expect(within(dialog).getByRole("heading", { name: "@pied-piper/billing" })).toBeInTheDocument();
  expect(within(dialog).getByText(/v0\.1\.0 · Project package · Programs can import it/)).toBeInTheDocument();
  expect(await within(dialog).findByText(/two charges for/)).toBeInTheDocument();
  expect(within(dialog).getByText("pied_piper")).toBeInTheDocument();
  expect(within(dialog).getByText('const charges = listCharges("pied_piper");', { exact: false, selector: 'pre' })).toBeInTheDocument();
  expect(within(dialog).getByText("@pied-piper/billing", { selector: ".pg-md-head" })).toBeInTheDocument();
  expect(within(dialog).queryByText("```ts")).not.toBeInTheDocument();
  expect(server.requests()).toContain("GET /api/packages/docs?package=%40pied-piper%2Fbilling&blueprint=billing");
});

test("stepping into a package call shows the package's source with the active line centred", async () => {
  const server = new FakeServer();
  server.handle = (call) => (call.url.startsWith("/api/packages/source") ? json(fixtureJson("model-package-source.json")) : undefined);
  const { user } = await openPage(server);
  await runsLoaded();

  await user.click(screen.getByRole("button", { name: "From the start" }));
  // The program's line that calls into the package, then the package's own line.
  await user.keyboard("{ArrowRight}{ArrowRight}{ArrowRight}");
  expect(within(codeRegion()).getByText("Line 4 calls into @pied-piper/billing (listCharges). Step → to follow it into the package’s code.")).toBeInTheDocument();
  await user.keyboard("{ArrowRight}");

  const code = codeRegion();
  await within(code).findAllByText('check("pied-piper.example/charges.list", { customerId });');
  const active = code.querySelector(".pg-line.active") as HTMLElement;
  expect(active.querySelector(".num")?.textContent).toBe("28");
  expect(active.querySelector(".txt")?.textContent).toBe('    check("pied-piper.example/charges.list", { customerId });');
  expect(within(code).getByText("@pied-piper/billing · lib · called from program 1, line 4")).toBeInTheDocument();
  expect(within(code).getByText(/as installed now/)).toBeInTheDocument();
  expect(server.requests()).toContain("GET /api/packages/source?package=%40pied-piper%2Fbilling&module=lib");
  const centred = scrolled.filter((s) => s.el === active);
  expect(centred.at(-1)?.options).toEqual({ block: "center" });
});

test("a missing package source says so in the code pane, in the server's words", async () => {
  const server = new FakeServer();
  server.handle = (call) =>
    call.url.startsWith("/api/packages/source")
      ? Response.json({ error: "unknown_module", message: "package `@pied-piper/billing` has no module `lib`" }, { status: 404 })
      : undefined;
  const { user } = await openPage(server);
  await runsLoaded();
  await user.click(screen.getByRole("button", { name: "From the start" }));
  await user.keyboard("{ArrowRight}{ArrowRight}{ArrowRight}{ArrowRight}");
  expect(await within(codeRegion()).findByText("@pied-piper/billing · lib: its source isn’t available.")).toBeInTheDocument();
  expect(within(codeRegion()).getByText("package @pied-piper/billing has no module lib")).toBeInTheDocument();
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
  await within(logPane()).findByText("Program 3 · Run 3");

  await user.click(screen.getByRole("button", { name: "Previous event" }));
  await user.click(rowHead("r2"));
  for (const name of ["Draft a rule for check 2.1", "Test with recorded results · v2", "Rerun live…"]) {
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
  await user.click(within(row("r2")).getByRole("button", { name: "Rerun live…" }));
  const dialog = screen.getByRole("dialog", { name: "Rerun run 2 for real?" });
  expect(posts(server)).toEqual([]);
  await user.click(within(dialog).getByRole("button", { name: "Rerun for real" }));
  expect(posts(server).map((c) => [c.url, c.body])).toEqual([["/api/rerun", { run: 2, variables: { customerId: "pied_piper" } }]]);

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
  expect(within(logPane()).queryByText("Program 4 · Run 4")).not.toBeInTheDocument();
  expect(within(codeRegion()).getByText("Used by Run 7. This is the original program from Run 1.")).toBeInTheDocument();
  expect(server.requests()).toContain(`GET /api/sessions/${SESSION(1)}/events`);
});

test("the summary chevron hides and restores only the summary metrics without losing focus", async () => {
  const { user } = await openPage();
  await runsLoaded();
  expect(screen.getByRole("region", { name: "Session" })).toBeInTheDocument();
  expect(screen.getByText("Fetched")).toBeInTheDocument();

  await user.click(screen.getByRole("button", { name: "Show summary" }));
  await user.click(screen.getByRole("button", { name: "Hide summary" }));
  const show = screen.getByRole("button", { name: "Show summary" });
  expect(show).toHaveAttribute("aria-expanded", "false");
  expect(show).toHaveFocus();
  expect(screen.getByText("Fetched")).not.toBeVisible();
  expect(screen.getByRole("region", { name: "Session" })).toBeInTheDocument();

  await user.keyboard("{Enter}");
  const hide = screen.getByRole("button", { name: "Hide summary" });
  expect(hide).toHaveAttribute("aria-expanded", "true");
  expect(hide).toHaveFocus();
  expect(screen.getByText("Fetched")).toBeVisible();
});

test("a reader at the latest event stays there; one scrolled up isn't moved and gets Back to latest", async () => {
  const server = new FakeServer();
  const feed = liveFeed(server, SESSION(4));
  const all = blocks("feed-session-4.sse");
  const { user } = await openPage(server);
  await feed.send(...all.slice(0, 12));
  await within(logPane()).findByText("Program 1 · Run 1");

  const log = logPane().querySelector(".pg-log") as HTMLElement;
  let height = 1000;
  Object.defineProperty(log, "scrollHeight", { configurable: true, get: () => height });
  Object.defineProperty(log, "clientHeight", { configurable: true, get: () => 100 });
  height = 1200;
  await feed.send(...all.slice(12, 20));
  await within(logPane()).findByText("Program 2 · Run 2");
  expect(log.scrollTop).toBe(1200);
  expect(screen.queryByRole("button", { name: "Back to latest" })).not.toBeInTheDocument();

  // Scrolling up to read isn't being behind; something new arriving meanwhile is.
  log.scrollTop = 300;
  fireEvent.scroll(log);
  expect(screen.queryByRole("button", { name: "Back to latest" })).not.toBeInTheDocument();
  height = 1500;
  await feed.send(...all.slice(20, 26));
  await within(logPane()).findByText("Program 3 · Run 3");
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

  await user.click(within(row("r2")).getByRole("button", { name: "Test with recorded results · v2" }));
  expect(posts(server).map((c) => [c.url, c.body])).toEqual([["/api/test", { run: 2 }]]);
  expect(await within(row("r2")).findByText("Reruns by you (1)")).toBeInTheDocument();
  expect(within(row("f5")).getByText("· Test of blueprint v2")).toBeInTheDocument();
  expect(within(row("f5")).getByText("Completed")).toBeInTheDocument();
  // The notice says it was a test, so it doesn't read as the last rerun's.
  expect(screen.getByText("Test of run 2 against blueprint v2 completed: run 5. It’s under run 2, in Reruns by you.")).toBeInTheDocument();
  // The original selection stays stable while its new comparison becomes available.
  expect(row("r2")).toHaveClass("selected");
  expect(row("f5")).not.toHaveClass("selected");
  expect(rowHead("f5")).toHaveAttribute("aria-expanded", "true");
  // Selecting the result opened by completion must not hide that evidence.
  await user.click(screen.getByRole("button", { name: "Run 5 · Test of blueprint v2: Completed" }));
  expect(row("f5")).toHaveClass("selected");
  expect(rowHead("f5")).toHaveAttribute("aria-expanded", "true");
  expect(within(row("f5")).getByText("33")).toBeVisible();
  expect(row("f5")).toHaveTextContent(fixtureJson<Show>("show-5.json").test!.local_state!);
  expect(screen.getByText("Viewing Run 5")).toBeVisible();
  expect(codeRegion()).toHaveTextContent("Source: Program 2 · Run 2");
  expect(codeRegion()).toHaveTextContent("Used by Run 5");
  expect(screen.getByRole("button", { name: /View details · Program 2 · Run 2.*Source for Run 5/ })).not.toHaveClass("selected");
  // Once selected, the same keyboard disclosure retains collapse/reopen capability.
  await user.keyboard("{Enter}");
  expect(rowHead("f5")).toHaveAttribute("aria-expanded", "false");
  await user.keyboard(" ");
  expect(rowHead("f5")).toHaveAttribute("aria-expanded", "true");
  expect(posts(server)).toHaveLength(1);
});

test("a test that kept the run's values names the first one bound differently, and opens the rest in place", async () => {
  const server = new FakeServer();
  const bound = { ...fixtureJson<Record<string, unknown>>("status.json"), binding: { variables: { customerId: "cus_globex", region: "us", tier: "gold" }, secrets: [] } };
  server.handle = (call) => (call.method === "GET" && call.url.split("?")[0] === "/api/status" ? json(bound) : undefined);
  const { user } = await openPage(server);
  await runsLoaded();
  await user.click(rowHead("r2"));
  await user.click(within(row("r2")).getByText(/by you \(/));
  await user.click(rowHead("f5"));
  const f5 = row("f5");
  expect(within(f5).getByText(/This test kept run 2’s values; the ones bound now differ: customerId pied_piper → cus_globex/)).toBeInTheDocument();
  expect(within(f5).queryByText("region not set → us")).not.toBeInTheDocument();
  await user.click(within(f5).getByRole("button", { name: "(+2 more)" }));
  expect(within(f5).getByText("region not set → us")).toBeInTheDocument();
  expect(within(f5).getByText("tier not set → gold")).toBeInTheDocument();
  await user.click(within(f5).getByRole("button", { name: "show less" }));
  expect(within(f5).queryByText("region not set → us")).not.toBeInTheDocument();
});

test("an open session that keeps changed values shows a compact notice and offers a new session", async () => {
  const server = new FakeServer();
  const bound = { ...fixtureJson<Record<string, unknown>>("status.json"), binding: { variables: { customerId: "cus_globex" }, secrets: [] } };
  server.handle = (call) => (call.method === "GET" && call.url.split("?")[0] === "/api/status" ? json(bound) : undefined);
  const { user } = await openPage(server);
  await runsLoaded();
  const note = await screen.findByText(/New sessions will use different settings\./);
  await user.click(within(note.closest(".pg-callout") as HTMLElement).getByRole("button", { name: "Show details" }));
  expect(within(note.closest(".pg-callout") as HTMLElement).getByText("customerId pied_piper → cus_globex")).toBeInTheDocument();
  await user.click(within(note.closest(".pg-callout") as HTMLElement).getByRole("button", { name: "Start a new session to use them" }));
  expect(await screen.findByRole("button", { name: "Configure model" })).toBeInTheDocument();
  // The form is for a new session, which takes the bound values: the note was about the old one.
  expect(screen.queryByText("New sessions will use different settings.")).not.toBeInTheDocument();
});

test("a rerun's row opens its own session to step through, and Back returns to the row", async () => {
  const { server, user } = await openPage();
  await runsLoaded();
  await user.click(rowHead("r1"));
  await user.click(within(row("r1")).getByText(/^Reruns by you \(/));
  await user.click(rowHead("f7"));
  await user.click(within(row("f7")).getByRole("button", { name: "Step through this rerun" }));

  await expect.poll(() => server.requests()).toContain(`GET /api/sessions/${SESSION(1)}/events`);
  const chat = screen.getByRole("region", { name: "Conversation" });
  expect(await within(chat).findByText(/No chat: this session is run 7, a rerun of run 1 that you started\. The agent never saw it\./)).toBeInTheDocument();
  expect(within(chat).getByText("A rerun takes no messages.")).toBeInTheDocument();
  // Opened at the rerun.
  expect(row("r7").className).toContain("selected");

  // The way back is said where the page's notices are, as well as in the chat.
  const notice = screen.getByText("You’re in run 7’s own session, a rerun of run 1 you started: the log shows only this run, and the agent never saw it.");
  expect(within(notice.closest(".pg-callout") as HTMLElement).getByRole("button", { name: "Back to session " + SESSION(4).slice(0, 8) })).toBeInTheDocument();
  // The code is the agent's program, from a session this one isn't: it has no number here.
  expect(within(screen.getAllByRole("region", { name: "Program" })[0]).getByText(/Source: Program · Run 1/)).toBeInTheDocument();

  await user.click(within(notice.closest(".pg-callout") as HTMLElement).getByRole("button", { name: "Back to session " + SESSION(4).slice(0, 8) }));
  await expect.poll(() => document.getElementById("pgrow-f7")?.className ?? "").toContain("selected");
  expect(screen.queryByRole("button", { name: /^Back to session/ })).not.toBeInTheDocument();
  expect(screen.queryByText(/own session, a rerun of run 1/)).not.toBeInTheDocument();
});

test("rerun starts from its recorded values and requires reviewed values before sending", async () => {
  const server = new FakeServer();
  const status = fixtureJson<Status>("status.json");
  status.binding.variables = {};
  status.declared_variables.push({ name: "accountId", required: true });
  server.handle = (call) => call.url === "/api/status" ? json(status) : undefined;
  const { user } = await openPage(server);
  await runsLoaded();
  await user.click(rowHead("r2"));
  await user.click(within(row("r2")).getByRole("button", { name: "Rerun live…" }));
  const dialog = screen.getByRole("dialog", { name: "Rerun run 2 for real?" });
  expect(within(dialog).getByLabelText("customerId (required)")).toHaveValue("pied_piper");
  const go = within(dialog).getByRole("button", { name: "Rerun for real" });
  expect(go).toBeDisabled();
  await user.click(go);
  expect(posts(server)).toEqual([]);
  await user.type(within(dialog).getByLabelText("accountId (required)"), "account-1");
  await user.clear(within(dialog).getByLabelText("customerId (required)"));
  expect(go).toBeDisabled();
  await user.type(within(dialog).getByLabelText("customerId (required)"), "reviewed");
  await user.click(go);
  expect(posts(server).map((call) => [call.url, call.body])).toEqual([
    ["/api/rerun", { run: 2, variables: { customerId: "reviewed", accountId: "account-1" } }],
  ]);
});

test("a refused rerun keeps the dialog and edited values with the server's actionable error", async () => {
  const server = new FakeServer();
  const status = fixtureJson<Status>("status.json");
  status.binding.variables.customerId = "bound-current";
  const message = "required secret API_KEY is missing";
  server.handle = (call) => {
    if (call.url === "/api/status") return json(status);
    if (call.url.startsWith("/api/runs/2?")) return json({ ...fixtureJson<Show>("show-2.json"), variables: { customerId: "recorded-override" } });
    if (call.url === "/api/rerun") return json({ exit: 2, result: { error: { kind: "missing-secret", message } }, text: "" });
    return undefined;
  };
  const { user } = await openPage(server);
  await runsLoaded();
  await user.click(rowHead("r2"));
  await user.click(within(row("r2")).getByRole("button", { name: "Rerun live…" }));
  const dialog = screen.getByRole("dialog", { name: "Rerun run 2 for real?" });
  const field = within(dialog).getByLabelText("customerId (required)");
  expect(field).toHaveValue("recorded-override");
  await user.clear(field);
  await user.type(field, "keep-this-value");
  await user.click(within(dialog).getByRole("button", { name: "Rerun for real" }));
  expect(await within(dialog).findByRole("alert")).toHaveTextContent(message);
  expect(field).toHaveValue("keep-this-value");
  expect(within(dialog).getByRole("button", { name: "Rerun for real" })).toBeEnabled();
  expect(screen.getByText("Couldn’t rerun run 2")).toBeInTheDocument();
  expect(screen.queryByText(/The playground didn’t answer/)).not.toBeInTheDocument();
  expect(logPane()).toBeInTheDocument();
  expect(posts(server)).toHaveLength(1);
});

test("finishing a rerun leaves a different dialog opened while it was running alone", async () => {
  const server = new FakeServer();
  let finish = () => {};
  server.handle = (call) => call.url === "/api/rerun"
    ? new Promise<Response>((resolve) => { finish = () => resolve(json(fixtureJson("rerun.json"))); })
    : undefined;
  const { user } = await openPage(server);
  await runsLoaded();
  await user.click(rowHead("r2"));
  await user.click(within(row("r2")).getByRole("button", { name: "Rerun live…" }));
  const dialog = screen.getByRole("dialog", { name: "Rerun run 2 for real?" });
  await user.click(within(dialog).getByRole("button", { name: "Rerun for real" }));
  expect(within(dialog).getByRole("button", { name: "Rerunning…" })).toBeDisabled();
  await user.click(within(dialog).getAllByRole("button", { name: "Close" }).at(-1)!);
  await user.click(screen.getByRole("button", { name: "Current blueprint billing · v2" }));
  const blueprint = screen.getByRole("dialog");
  await act(async () => { finish(); });
  expect(blueprint).toBeInTheDocument();
  expect(screen.getByRole("dialog")).toBe(blueprint);
  expect(posts(server).filter((call) => call.url === "/api/rerun")).toHaveLength(1);
});

test("a refused action shows the server's message in the toast", async () => {
  const server = new FakeServer();
  server.handle = (call) => (call.url === "/api/test" ? new Response(fixture("test-unknown-run.json")) : undefined);
  const { user } = await openPage(server);
  await runsLoaded();
  await user.click(rowHead("r1"));
  await user.click(within(row("r1")).getByRole("button", { name: "Test with recorded results · v2" }));
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

test("a refused file tool's call offers a recheck, which says what the blueprint in force decides and runs nothing", async () => {
  const server = new FakeServer();
  const asCall = { ...fixtureJson<Record<string, unknown>>("show-2.json"), entry: "mcp:submilli__files__list" };
  server.handle = (call) => (call.method === "GET" && call.url.split("?")[0] === "/api/runs/2" ? json(asCall) : undefined);
  const { user } = await openPage(server);
  await screen.findByText("Call · files.list · Run 2 · customerId = cus_globex", {}, { timeout: 3000 });
  await user.click(rowHead("r2"));
  expect(within(row("r2")).queryByRole("button", { name: /Test against/ })).not.toBeInTheDocument();
  await user.click(within(row("r2")).getByRole("button", { name: "Recheck policy · v2" }));
  expect(posts(server).map((c) => [c.url, c.body])).toEqual([["/api/recheck", { run: 2 }]]);
  const notice = (await screen.findByRole("status")).textContent;
  expect(notice).toContain("Check 2.1 is allowed now, under blueprint v2.");
  expect(notice).toContain("Nothing ran: to make the call again, ask the agent.");
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
    if (call.url.split("?")[0] === "/api/sessions") {
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
  const model = screen.getByRole("textbox", { name: /^Model/ });
  await user.clear(model);
  await user.type(model, "claude-haiku-5-5");
  await user.click(screen.getByRole("button", { name: "Done configuring model" }));
  const field = screen.getByRole("textbox", { name: /customerId/ });
  await user.clear(field);
  await user.type(field, "cus_globex");
  await completeSetup(user);
  await user.click(screen.getByRole("button", { name: "Start session" }));

  const bar = screen.getByRole("region", { name: "Session" });
  expect(within(bar).getByText("cus_globex")).toBeInTheDocument();
  expect(within(bar).getByText("Agent Claude · claude-haiku-5-5")).toBeInTheDocument();
  expect(within(bar).getByText("Not started")).toBeInTheDocument();
  expect(posts(server)).toEqual([]);

  await user.type(screen.getByRole("textbox", { name: "Message the agent" }), "Hello{Enter}");
  const agent = server.calls.find((c) => c.url === "/api/agent");
  expect(agent?.body).toEqual({ task: "Hello", blueprint: "billing", model: "anthropic:claude-haiku-5-5", variables: { customerId: "cus_globex" } });
  await expect.poll(() => server.requests()).toContain(`GET /api/sessions/${SESSION(8)}/events`);
});

test("New session starts with the latest chat session's agent and settings, with settings open when it has some", async () => {
  const server = new FakeServer();
  const list = fixtureJson<SessionList>("sessions.json");
  // The latest chat session on OpenAI with settings, and an older one on Claude.
  const chat = list.sessions[3];
  list.sessions[3] = { ...chat, model: "openai:gpt-6-luna", settings: { effort: "low", max_output: 2000, provider: { temperature: 0.2 } } };
  list.sessions.push({ ...chat, session: SESSION(9), started_at_micros: chat.started_at_micros - 1 });
  server.handle = (call) => (call.url.split("?")[0] === "/api/sessions" ? json(list) : undefined);
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
  await user.click(within(row("r2")).getByRole("button", { name: "Rerun live…" }));
  await user.click(within(screen.getByRole("dialog", { name: "Rerun run 2 for real?" })).getByRole("button", { name: "Rerun for real" }));
  const notice = (await screen.findByText("Running:")).closest(".pg-toast") as HTMLElement;
  await user.click(within(notice).getByRole("button", { name: "Cancel" }));
  await expect.poll(() => posts(server).map((c) => [c.url, c.body])).toContainEqual(["/api/cancel", { run: 8 }]);
  // The rerun ends, cancelled, and says so as any other.
  expect(await screen.findByText(/^Rerun of run 2 cancelled: run 8\. It’s under run 2/)).toBeInTheDocument();
});

test("Start session is blocked while a required variable is empty", async () => {
  const { server, user } = await openPage();
  await runsLoaded();
  await user.click(screen.getByRole("button", { name: "New session" }));
  await user.click(screen.getByRole("button", { name: "Done configuring model" }));
  await user.clear(screen.getByRole("textbox", { name: /customerId/ }));
  const start = screen.getByRole("button", { name: "Done preparing variables" });
  await user.click(start);
  expect(screen.queryByRole("button", { name: "Start session" })).not.toBeInTheDocument();
  expect(screen.getByText("Required.")).toBeInTheDocument();
  expect(posts(server)).toEqual([]);
});

test("a declared variable explains its purpose and offers its author-provided example", async () => {
  const server = new FakeServer();
  const status = fixtureJson<Record<string, unknown>>("status.json");
  status.declared_variables = [
    {
      name: "customerId",
      required: true,
      description: "Use the demo customer below, or find another customerId in submilli/volumes/billing/charges.json. In your application, this value comes from the signed-in customer's account record. This blueprint fixes the customer for the session and denies charge lookups for other customers.",
      example: "pied_piper",
    },
  ];
  server.handle = (call) => (call.url === "/api/status" ? json(status) : undefined);
  const { user } = await openPage(server);
  await runsLoaded();
  await user.click(screen.getByRole("button", { name: "New session" }));

  await user.click(screen.getByRole("button", { name: "Done configuring model" }));
  await user.click(screen.getByText("About customerId"));
  const field = screen.getByRole("textbox", { name: /customerId/ });
  await user.clear(field);
  await user.type(field, "Ehud");
  expect(document.getElementById("help-customerId")).toHaveTextContent(
    "submilli/volumes/billing/charges.json",
  );
  expect(field).toHaveAccessibleDescription("Required by blueprint billing to start this session.");
  expect(document.getElementById("help-customerId")).toHaveTextContent(/denies charge lookups for other customers/);
  expect(screen.getByText("pied_piper")).toBeInTheDocument();
  expect(field).toHaveValue("Ehud");

  await user.click(screen.getByRole("button", { name: "Use example" }));
  expect(field).toHaveValue("pied_piper");
});

test("session setup explains the agent first and application context second", async () => {
  const { user } = await openPage();
  await runsLoaded();
  await user.click(screen.getByRole("button", { name: "New session" }));

  const model = screen.getByRole("button", { name: "Configure model" });
  const variables = screen.getByRole("button", { name: "Bind application variables" });
  expect(model).toHaveAttribute("aria-expanded", "true");
  expect(variables).toBeDisabled();
  expect(screen.queryByRole("textbox", { name: /customerId/ })).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Start session" })).not.toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: "Done configuring model" }));
  expect(variables).toHaveAttribute("aria-expanded", "true");
  expect(screen.getByRole("textbox", { name: /customerId/ })).toBeVisible();
  expect(model).toHaveTextContent("✓ Done · Edit");
});

test("a Claude Code stand-in's session says, in place of the chat box, that its chat happens in Claude Code", async () => {
  const server = new FakeServer();
  server.handle = (call) => {
    if (call.url.split("?")[0] !== "/api/sessions") return undefined;
    const list = fixtureJson<SessionList>("sessions.json");
    list.sessions.unshift({ ...list.sessions[3], session: SESSION(8), sources: ["stand-in"], model: undefined, open: null, runs: [], run_count: 0 });
    return json(list);
  };
  await openPage(server);
  const chat = within(screen.getByRole("region", { name: "Conversation" }));
  expect(await chat.findByText("You chat with this session’s stand-in in Claude Code.")).toBeInTheDocument();
  expect(chat.queryByRole("button", { name: "New session" })).not.toBeInTheDocument();
  expect(within(screen.getByRole("banner")).getByRole("button", { name: "New session" })).toBeVisible();
  expect(chat.queryByRole("textbox")).not.toBeInTheDocument();
});

test("New session's way back sits in the top bar, beside Sessions, and leaves the form", async () => {
  const { user } = await openPage();
  await runsLoaded();
  await user.click(screen.getByRole("button", { name: "New session" }));
  const back = within(screen.getByRole("banner")).getByRole("button", { name: "← Back to the session you were viewing" });
  await user.click(back);
  expect(screen.queryByRole("button", { name: "Start session" })).not.toBeInTheDocument();
  expect(within(screen.getByRole("banner")).queryByRole("button", { name: /Back to the session/ })).not.toBeInTheDocument();
});

test("Open it on a session that started while New session is open leaves the form and shows that session", async () => {
  const server = new FakeServer();
  let started = false;
  server.handle = (call) => {
    if (call.url.split("?")[0] !== "/api/sessions" || !started) return undefined;
    const list = fixtureJson<SessionList>("sessions.json");
    list.sessions.unshift({ ...list.sessions[3], session: SESSION(8), sources: ["stand-in"], runs: [], run_count: 0 });
    return json(list);
  };
  const { user } = await openPage(server);
  await runsLoaded();
  await user.click(screen.getByRole("button", { name: "New session" }));
  // A stand-in starts on its task in Claude Code meanwhile.
  started = true;
  window.dispatchEvent(new Event("focus"));
  await screen.findByText(/^Another session started while you were watching/, {}, { timeout: 3000 });
  await user.click(screen.getByRole("button", { name: "Open it" }));
  await expect.poll(() => screen.queryByRole("button", { name: "Start session" })).toBeNull();
  await expect.poll(() => server.requests()).toContain(`GET /api/sessions/${SESSION(8)}/events`);
});

test("the blueprint dialog shows the version a decision used, its rule lit, and that it isn't the one in force", async () => {
  const { server, user } = await openPage();
  await runsLoaded();
  await user.click(rowHead("r2"));
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

test("the current blueprint opens without version pills, and history can select an older version", async () => {
  const { user } = await openPage();
  await runsLoaded();
  await user.click(screen.getByRole("button", { name: /Current blueprint billing · v2/ }));

  const dialog = screen.getByRole("dialog", { name: "Blueprint billing · v2" });
  expect(await within(dialog).findByText("kind: blueprint")).toBeInTheDocument();
  expect(within(dialog).getByRole("button", { name: "Project blueprint history (2)" })).toHaveAttribute("aria-expanded", "false");
  expect(within(dialog).queryByRole("button", { name: "v1" })).not.toBeInTheDocument();

  await user.click(within(dialog).getByRole("button", { name: "Project blueprint history (2)" }));
  expect(within(dialog).getByRole("button", { name: "v1" })).toBeInTheDocument();
  await user.click(within(dialog).getByRole("button", { name: "v1" }));
  expect(await within(dialog).findByText("kind: blueprint")).toBeInTheDocument();
  expect(within(dialog).getByText(/Not the version in force/)).toBeInTheDocument();
});

test("the blueprint dialog replaces a failed version load with an error and a working retry", async () => {
  const server = new FakeServer();
  let unavailable = true;
  server.handle = (call) =>
    call.url === "/api/changes?version=2" && unavailable
      ? Response.json({ error: "busy", message: "version text is temporarily unavailable" }, { status: 503 })
      : undefined;
  const { user } = await openPage(server);
  await runsLoaded();
  await user.click(screen.getByRole("button", { name: /Current blueprint billing · v2/ }));

  const dialog = screen.getByRole("dialog", { name: "Blueprint billing · v2" });
  expect(await within(dialog).findByText("version text is temporarily unavailable")).toBeInTheDocument();
  expect(within(dialog).queryByText("Loading this version’s text…")).not.toBeInTheDocument();

  unavailable = false;
  await user.click(within(dialog).getByRole("button", { name: "Try again" }));
  expect(await within(dialog).findByText("kind: blueprint")).toBeInTheDocument();
  expect(within(dialog).queryByRole("button", { name: "Try again" })).not.toBeInTheDocument();
});

test("recorded text with HTML markup shows as literal text", async () => {
  const server = new FakeServer();
  const markup = "<img src=x onerror=alert(1)>";
  server.feed = () => ({ text: fixture("feed-session-4.sse").replace("Let me total your charges.", markup + " Let me total"), close: false });
  server.handle = (call) =>
    call.url.startsWith("/api/packages/source")
      ? json({ package: "@pied-piper/billing", module: "lib", bytes: 40, truncated: false, note: null, untrusted: { text: "<script>alert(1)</script>" } })
      : undefined;
  const { user } = await openPage(server);
  await runsLoaded();

  expect(within(screen.getByRole("region", { name: "Conversation" })).getByText(markup + " Let me total", { exact: false })).toBeInTheDocument();
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
  await user.click(within(row("r2").querySelector(":scope > .pg-event-detail") as HTMLElement).getAllByRole("button", { name: "Copy for assistant" })[0]);
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
  await user.dblClick(within(row("r2")).getByRole("button", { name: "Test with recorded results · v2" }));
  await vi.waitFor(() => expect(posts(server).map((c) => c.url)).toEqual(["/api/test"]));
});

test("New session remains available while the summary is hidden", async () => {
  const { user } = await openPage();
  await runsLoaded();
  await user.click(screen.getByRole("button", { name: "Show summary" }));
  await user.click(screen.getByRole("button", { name: "Hide summary" }));
  await user.click(screen.getByRole("button", { name: "New session" }));
  expect(screen.getByRole("button", { name: "Configure model" })).toBeInTheDocument();
});

test("an empty playground has one New session CTA and hides inactive panes", async () => {
  const server = new FakeServer();
  server.sessions = { sessions: [] };
  const { user } = await openPage(server);
  expect(screen.getAllByRole("button", { name: "New session" })).toHaveLength(1);
  expect(within(screen.getByRole("banner")).getByRole("button", { name: "New session" })).toBeVisible();
  expect(screen.getByRole("heading", { name: "Start a playground session" })).toBeInTheDocument();
  expect(screen.queryByRole("region", { name: "Session summary" })).not.toBeInTheDocument();
  expect(screen.queryByRole("region", { name: "Conversation" })).not.toBeInTheDocument();
  expect(screen.queryByText(/No sessions yet|Start a session with New session/)).not.toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: "New session" }));
  expect(screen.getByRole("heading", { name: "Configure model" })).toBeInTheDocument();
  expect(screen.getByRole("heading", { name: "Bind application variables" })).toBeInTheDocument();
  expect(screen.queryAllByRole("button", { name: "New session" })).toHaveLength(0);
  expect(screen.queryByRole("region", { name: "Session summary" })).not.toBeInTheDocument();
  expect(screen.queryByRole("region", { name: "Conversation" })).not.toBeInTheDocument();
  expect(screen.queryByRole("region", { name: "Program" })).not.toBeInTheDocument();
  expect(screen.queryByRole("region", { name: "Execution" })).not.toBeInTheDocument();
  expect(screen.queryByText("New session · not started yet")).not.toBeInTheDocument();
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
  await user.click(screen.getByRole("button", { name: "Done configuring model" }));
  const field = screen.getByRole("textbox", { name: /customerId/ });
  expect(field).toHaveValue("");
  expect(screen.queryByText("Required.")).not.toBeInTheDocument();
  expect(field).not.toHaveAttribute("aria-invalid");
  await user.click(screen.getByRole("button", { name: "Done preparing variables" }));
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
  expect(screen.getByRole("status")).toHaveTextContent("No Claude key saved");
  const agent = within(screen.getByRole("group", { name: "Provider" }));
  await user.click(agent.getByRole("button", { name: /^Gemini/ }));
  expect(agent.getByRole("button", { name: /^Gemini/ })).toHaveAttribute("aria-pressed", "true");
  expect(agent.getByText("No Gemini key saved")).toBeInTheDocument();
  await user.click(agent.getByRole("button", { name: "Add Gemini key" }));
  expect(screen.getByRole("dialog", { name: "Model key" })).toBeInTheDocument();
  await user.click(within(screen.getByRole("dialog", { name: "Model key" })).getByRole("button", { name: "Close" }));
  expect(screen.getByRole("textbox", { name: /^Model/ })).toHaveValue("gemini-flash-latest");
  await user.click(screen.getByText("Advanced model options"));
  expect(screen.queryByRole("group", { name: "Reasoning effort" })).not.toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: "Done configuring model" }));
  expect(screen.getByRole("status")).toHaveTextContent("No Gemini key saved");
  expect(screen.queryByRole("button", { name: "Start session" })).not.toBeInTheDocument();
});

test("session settings and JSON variables keep entered values when their disclosures change", async () => {
  const { user } = await openPage();
  await runsLoaded();
  await user.click(screen.getByRole("button", { name: "New session" }));
  await user.click(screen.getByRole("button", { name: "Done configuring model" }));
  const customer = screen.getByRole("textbox", { name: /customerId/ });
  await user.clear(customer);
  await user.type(customer, "cus_kept");

  await user.click(screen.getByRole("button", { name: "Edit as JSON" }));
  expect((screen.getByRole("textbox", { name: "Session variables as JSON" }) as HTMLTextAreaElement).value).toContain("cus_kept");
  await user.click(screen.getByRole("button", { name: "Edit as fields" }));
  expect(screen.getByRole("textbox", { name: /customerId/ })).toHaveValue("cus_kept");

  const summary = screen.getByRole("button", { name: /Configure model/ });
  await user.click(summary);
  const model = screen.getByRole("textbox", { name: /^Model/ });
  await user.clear(model);
  await user.type(model, "claude-haiku-5-5");
  await user.click(screen.getByRole("button", { name: "Done configuring model" }));
  expect(summary).toHaveAttribute("aria-expanded", "false");
  expect(summary).toHaveTextContent("Claude · claude-haiku-5-5");
  await user.click(summary);
  expect(screen.getByRole("textbox", { name: /^Model/ })).toHaveValue("claude-haiku-5-5");
});

test("reasoning effort stays available for Claude and is omitted for Gemini", async () => {
  const { user } = await openPage();
  await runsLoaded();
  await user.click(screen.getByRole("button", { name: "New session" }));
  const agent = within(screen.getByRole("group", { name: "Provider" }));
  await user.click(agent.getByRole("button", { name: "Claude" }));
  await user.click(screen.getByText("Advanced model options"));
  expect(screen.getByRole("group", { name: "Reasoning effort" })).toBeInTheDocument();
  await user.click(agent.getByRole("button", { name: /^Gemini/ }));
  expect(screen.queryByRole("group", { name: "Reasoning effort" })).not.toBeInTheDocument();
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
  await user.click(screen.getByRole("button", { name: "Add Claude key" }));
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
  await user.click(screen.getByRole("button", { name: "Change OpenAI key" }));
  let dialog = within(screen.getByRole("dialog", { name: "Model key" }));
  expect(dialog.getByText("A saved OpenAI key is in use. Saving replaces it.")).toBeInTheDocument();
  await user.click(dialog.getByRole("button", { name: "Close" }));
  expect(screen.getByRole("button", { name: "Configure model" })).toHaveFocus();
  await user.click(screen.getByRole("button", { name: "Change OpenAI key" }));
  dialog = within(screen.getByRole("dialog", { name: "Model key" }));
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
  await completeSetup(user);
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
  await completeSetup(user);
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

test("the next message can be written while the agent answers, and sends only once it's done", async () => {
  const server = new FakeServer();
  let answer: (r: Response) => void = () => {};
  let asked = 0;
  server.handle = (call) => {
    if (call.url !== "/api/agent") return undefined;
    asked++;
    return new Promise<Response>((resolve) => (answer = resolve));
  };
  const { user } = await openPage(server);
  await runsLoaded();
  const ask = screen.getByRole("textbox", { name: "Message the agent" });
  await user.type(ask, "First{Enter}");
  await screen.findByRole("button", { name: "Stop" });
  expect(ask).not.toHaveAttribute("readonly");
  expect(screen.getByText(/You can write your next one meanwhile/)).toBeInTheDocument();
  await user.type(ask, "Second{Enter}");
  // Enter waits for the answer: the draft stays, and nothing more was sent.
  expect(ask).toHaveValue("Second");
  expect(asked).toBe(1);
  // A failed answer puts the first message back above the draft, not over it.
  answer(json({ exit: 1, result: { error: { kind: "failure", message: "the provider timed out" } }, text: "" }));
  expect(await screen.findByRole("alert")).toBeInTheDocument();
  expect(ask).toHaveValue("First\n\nSecond");
});

test("Stop on a new session's first message leaves it in the session it started, not in the box", async () => {
  const server = new FakeServer();
  let sent = false;
  server.handle = (call) => {
    if (call.url.split("?")[0] === "/api/sessions") {
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
  await completeSetup(user);
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

test("a session started but not yet sent to is still there after a reload", async () => {
  const { user } = await openPage();
  await runsLoaded();
  await user.click(screen.getByRole("button", { name: "New session" }));
  await completeSetup(user);
  await user.click(screen.getByRole("button", { name: "Start session" }));
  expect(screen.getByText("New session · send its first message")).toBeInTheDocument();
  cleanup();
  store?.dispose();
  await openPage();
  expect(await screen.findByText("New session · send its first message")).toBeInTheDocument();
  expect(screen.getByRole("textbox", { name: "Message the agent" })).not.toHaveAttribute("readonly");
});

test("arrow keys keep native caret focus in the read-only ask box without stepping", async () => {
  const { user, server } = await openPage();
  await runsLoaded();
  const box = screen.getByRole("textbox", { name: "Message the agent" }) as HTMLTextAreaElement;
  await user.type(box, "Keep this draft");
  await user.click(screen.getByRole("button", { name: "From the start" }));
  expect(box).toHaveAttribute("readonly");
  await user.click(box);
  box.setSelectionRange(0, 0);
  await user.keyboard("{ArrowRight}{ArrowRight}");
  expect(screen.getByText(/^0 \/ \d+$/)).toBeInTheDocument();
  expect(box).toHaveFocus();
  expect(box.selectionStart).toBe(2);
  expect(box).toHaveValue("Keep this draft");
  // Playback remains available on its explicit control.
  await user.click(screen.getByRole("button", { name: "Next event" }));
  expect(screen.getByText(/^1 \/ \d+$/)).toBeInTheDocument();
  expect(posts(server)).toEqual([]);
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


test("reload restores a chosen historical run, draft and layout without running anything", async () => {
  const server = new FakeServer();
  const { user } = await openPage(server);
  await runsLoaded();
  await user.type(screen.getByRole("textbox"), "Continue with this draft");
  await user.click(rowHead("r2"));
  await user.click(screen.getByRole("button", { name: "Focus one pane" }));
  await user.click(screen.getByRole("tab", { name: "Program" }));
  const selectedText = codeRegion().textContent;
  cleanup();
  store?.dispose();
  store = null;
  await openPage(server);
  await runsLoaded();
  expect(screen.getByRole("textbox")).toHaveValue("Continue with this draft");
  expect(row("r2")).toHaveClass("selected");
  expect(codeRegion().textContent).toBe(selectedText);
  expect(screen.getByRole("tab", { name: "Program" })).toHaveAttribute("aria-selected", "true");
  expect(screen.getByRole("button", { name: "Show all panes" })).toBeInTheDocument();
  expect(posts(server)).toEqual([]);
});

test("pane arrows change the pane without moving recorded playback", async () => {
  const { user, server } = await openPage();
  await runsLoaded();
  await user.click(screen.getByRole("button", { name: "From the start" }));
  await user.click(screen.getByRole("tab", { name: "Conversation" }));
  await user.keyboard("{ArrowRight}");
  expect(screen.getByRole("tab", { name: "Program" })).toHaveFocus();
  expect(screen.getByText("0 / 20")).toBeInTheDocument();
  expect(posts(server)).toEqual([]);
});

test("denial evidence comes before follow-up actions and the original blueprint is inspectable", async () => {
  const { user } = await openPage();
  await runsLoaded();
  await user.click(rowHead("r2"));
  const check = row("d2.1");
  const draft = within(row("r2")).getByRole("button", { name: "Draft a rule for check 2.1" });
  expect(check.compareDocumentPosition(draft) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  expect(screen.getByRole("button", { name: "Recorded with v1" })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Current blueprint billing · v2" })).toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: "Recorded with v1" }));
  expect(screen.getByRole("dialog", { name: "Blueprint billing · v1" })).toBeInTheDocument();
});


test("fresh and existing composers retain separate drafts when reopening the same session", async () => {
  const { user, server } = await openPage();
  await runsLoaded();
  await user.type(screen.getByRole("textbox"), "prior session draft");
  await user.click(screen.getByRole("button", { name: "New session" }));
  await completeSetup(user);
  await user.click(screen.getByRole("button", { name: "Start session" }));
  await user.type(screen.getByRole("textbox"), "fresh session draft");
  const fresh = JSON.parse(sessionStorage.getItem("submilli-playground-new-session") ?? "null");
  await user.click(screen.getByRole("button", { name: "Sessions" }));
  const dialog = screen.getByRole("dialog", { name: "Sessions" });
  const previous = within(dialog).getByText(/^What's my total/).closest("tr") as HTMLElement;
  await user.click(within(previous).getByRole("button", { name: "Open" }));
  expect(screen.getByRole("textbox")).toHaveValue("prior session draft");
  expect(JSON.parse(sessionStorage.getItem("submilli-playground-draft:" + SESSION(4)) ?? "null")).toBe("prior session draft");
  expect(JSON.parse(sessionStorage.getItem("submilli-playground-draft:#fresh " + fresh.started) ?? "null")).toBe("fresh session draft");
  expect(posts(server)).toEqual([]);
});

test("opening another session from a fresh composer restores only the destination draft", async () => {
  const server = new FakeServer();
  const list = fixtureJson<SessionList>("sessions.json");
  const original = list.sessions.find((session) => session.session === SESSION(4));
  if (!original) throw new Error("Missing browser session fixture");
  server.sessions = { ...list, sessions: [...list.sessions, { ...original, session: SESSION(9), runs: [], untrusted: { task: "Another browser task" } }] };
  sessionStorage.setItem("submilli-playground-shown-session", SESSION(4));
  sessionStorage.setItem("submilli-playground-draft:" + SESSION(9), JSON.stringify("destination draft"));
  const { user } = await openPage(server);
  await runsLoaded();
  await user.type(screen.getByRole("textbox"), "prior session draft");
  await user.click(screen.getByRole("button", { name: "New session" }));
  await completeSetup(user);
  await user.click(screen.getByRole("button", { name: "Start session" }));
  await user.type(screen.getByRole("textbox"), "fresh session draft");
  await user.click(screen.getByRole("button", { name: "Sessions" }));
  const dialog = screen.getByRole("dialog", { name: "Sessions" });
  const destination = within(dialog).getByText(/^Another browser task/).closest("tr") as HTMLElement;
  await user.click(within(destination).getByRole("button", { name: "Open" }));
  await vi.waitFor(() => expect(screen.getByRole("textbox")).toHaveValue("destination draft"));
  expect(JSON.parse(sessionStorage.getItem("submilli-playground-draft:" + SESSION(4)) ?? "null")).toBe("prior session draft");
  expect(posts(server)).toEqual([]);
});

test("a saved unsent session cannot hide an explicit recorded run link", async () => {
  const server = new FakeServer();
  const { user } = await openPage(server);
  await runsLoaded();
  await user.click(screen.getByRole("button", { name: "New session" }));
  await completeSetup(user);
  await user.click(screen.getByRole("button", { name: "Start session" }));
  await user.type(screen.getByRole("textbox"), "unsent linked-away draft");
  const savedFresh = sessionStorage.getItem("submilli-playground-new-session");
  cleanup();
  store?.dispose();
  store = null;
  history.replaceState(null, "", "/#run=2");
  await openPage(server);
  await vi.waitFor(() => expect(row("r2")).toHaveClass("selected"));
  expect(screen.queryByText("New session · ready for its first message")).not.toBeInTheDocument();
  expect(sessionStorage.getItem("submilli-playground-new-session")).toBe(savedFresh);
  expect(screen.getByRole("tab", { name: "Execution" })).toHaveAttribute("aria-selected", "true");
  expect(posts(server)).toEqual([]);
});

test("a denied check opened by default collapses on its first click", async () => {
  const { user } = await openPage();
  await runsLoaded();
  await user.click(rowHead("r2"));
  expect(rowHead("d2.1")).toHaveAttribute("aria-expanded", "true");
  await user.click(rowHead("d2.1"));
  expect(rowHead("d2.1")).toHaveAttribute("aria-expanded", "false");
  await user.click(rowHead("d2.1"));
  expect(rowHead("d2.1")).toHaveAttribute("aria-expanded", "true");
});


test("cross-pane links move focus to the revealed evidence without changing execution", async () => {
  const { user, server } = await openPage();
  await runsLoaded();
  // Reproduce focus layout's hiding in jsdom; viewport layout itself is browser QA.
  const hiding = document.createElement("style");
  hiding.textContent = `.pg-app[data-layout="focus"][data-pane="chat"] > :not(.chat),
    .pg-app[data-layout="focus"][data-pane="log"] > :not(.log),
    .pg-app[data-layout="focus"][data-pane="code"] > :not(.code) { display: none; }`;
  document.head.append(hiding);
  try {
    await user.click(screen.getByRole("button", { name: "Focus one pane" }));
    const conversation = screen.getByRole("region", { name: "Conversation" });
    const run = within(conversation).getByRole("button", { name: /2/ });
    run.focus();
    await user.keyboard("{Enter}");
    expect(conversation).not.toBeVisible();
    expect(rowHead("r2")).toHaveFocus();
    expect(row("r2")).toHaveClass("selected");

    const source = within(row("d2.1")).getByRole("button", { name: /^Show line/ });
    source.focus();
    await user.keyboard("{Enter}");
    expect(source).not.toBeVisible();
    expect(within(codeRegion()).getByRole("heading", { level: 2 })).toHaveFocus();
    expect(codeRegion().querySelector(".pg-line.active")).not.toBeNull();
    expect(posts(server)).toEqual([]);

    // A routine background refresh cannot request cross-pane focus again.
    const tab = screen.getByRole("tab", { name: "Conversation" });
    await user.click(tab);
    await act(async () => { window.dispatchEvent(new Event("focus")); });
    expect(tab).toHaveFocus();
  } finally {
    hiding.remove();
  }
});


test("session details preserve secondary controls while bound values remain visible", async () => {
  const { user, server } = await openPage();
  await runsLoaded();
  const context = screen.getByRole("region", { name: "Session" });
  expect(within(context).getByText("pied_piper")).toBeVisible();
  const details = within(context).getByText("Session details");
  expect(details.parentElement).not.toHaveAttribute("open");
  await user.click(details);
  expect(within(context).getByRole("button", { name: /Copy a line for your assistant/ })).toBeVisible();
  expect(within(context).getByText(/Agent Claude/)).toBeVisible();
  await user.click(details);
  expect(within(context).getByText("pied_piper")).toBeVisible();
  expect(posts(server)).toEqual([]);
});


test.each([
  ["recorded", "Recorded test"],
  ["reads-live", "Test with real reads"],
  ["live", "Live test"],
  [undefined, "Test · mode unavailable"],
  ["sandbox-v2", "Test · mode unavailable"],
])("selected historical test mode %s is labeled without inference", async (mode, label) => {
  const server = new FakeServer();
  // Read raw JSON so the backend's supported omitted mode and future runtime values
  // cross the same API boundary as historical records, despite the narrower TS type.
  const historic = fixtureJson<{ test: Record<string, unknown> }>("show-5.json");
  if (mode === undefined) delete historic.test.mode;
  else historic.test.mode = mode;
  server.handle = (call) => call.url.split("?")[0] === "/api/runs/5" ? json(historic) : undefined;
  history.replaceState(null, "", "/#run=5");
  await openPage(server);
  await screen.findByText("Viewing Run 5");
  const context = within(screen.getByLabelText("Selected execution context"));
  expect(context.getByText(label!)).toBeVisible();
  if (mode === undefined || mode === "sandbox-v2") {
    expect(context.queryByText("Recorded test")).not.toBeInTheDocument();
    expect(row("f5")).toHaveTextContent("The test mode was not recorded.");
    expect(within(row("f5")).getByText(/The test mode was not recorded/)).not.toHaveTextContent("No live external connector calls.");
  }
  expect(posts(server)).toEqual([]);
});


test("interactive disclosures and controls keep arrows out of recorded playback", async () => {
  const { user, server } = await openPage();
  await runsLoaded();
  await user.click(screen.getByRole("button", { name: "Previous event" }));
  await user.keyboard("{Enter}");
  expect(screen.getByText("18 / 20")).toBeInTheDocument();

  await user.click(screen.getByText("Session details"));
  await user.keyboard("{ArrowRight}");
  expect(screen.getByText("18 / 20")).toBeInTheDocument();
  await user.keyboard("{ArrowLeft}");
  expect(screen.getByText("18 / 20")).toBeInTheDocument();
  within(screen.getByRole("region", { name: "Session" })).getByRole("button", { name: /Copy a line for your assistant/ }).focus();
  await user.keyboard("{ArrowRight}");
  expect(screen.getByText("18 / 20")).toBeInTheDocument();

  await user.click(rowHead("r2"));
  const cli = await screen.findByText("Equivalent CLI commands");
  cli.focus();
  await user.keyboard("{ArrowRight}");
  expect(screen.getByText("18 / 20")).toBeInTheDocument();
  rowHead("r2").focus();
  await user.keyboard("{ArrowLeft}");
  expect(screen.getByText("18 / 20")).toBeInTheDocument();
  // A descendant of an interactive header must use the same boundary.
  fireEvent.keyDown(rowHead("r2").querySelector("span")!, { key: "ArrowRight", bubbles: true });
  expect(screen.getByText("18 / 20")).toBeInTheDocument();

  const ask = screen.getByRole("textbox", { name: "Message the agent" });
  expect(ask).toHaveAttribute("readonly");
  ask.focus();
  await user.keyboard("{ArrowRight}");
  expect(screen.getByText("18 / 20")).toBeInTheDocument();
  await user.click(screen.getByRole("tab", { name: "Conversation" }));
  await user.keyboard("{ArrowRight}");
  expect(screen.getByRole("tab", { name: "Program" })).toHaveFocus();
  expect(screen.getByText("18 / 20")).toBeInTheDocument();

  // Recorded playback buttons and noninteractive pane content retain the shortcut.
  await user.click(screen.getByRole("button", { name: "Previous event" }));
  await user.keyboard("{ArrowRight}");
  expect(screen.getByText("18 / 20")).toBeInTheDocument();
  (codeRegion().querySelector(".pg-code") as HTMLElement).focus();
  await user.keyboard("{ArrowRight}");
  expect(screen.getByText("19 / 20")).toBeInTheDocument();
  fireEvent.keyDown(document.body, { key: "ArrowRight" });
  expect(screen.getByText("20 / 20")).toBeInTheDocument();
  expect(ask).not.toHaveAttribute("readonly");
  ask.focus();
  await user.keyboard("{ArrowLeft}");
  expect(screen.getByText("20 / 20")).toBeInTheDocument();
  expect(posts(server)).toEqual([]);
});


test("playback context identifies the final recorded outcome separately from the current event", async () => {
  const { user, server } = await openPage();
  await runsLoaded();
  const context = within(screen.getByLabelText("Selected execution context"));
  expect(context.getByText("Completed")).toBeVisible();
  await user.click(screen.getByRole("button", { name: "Previous event" }));
  await user.keyboard("{Enter}");
  expect(screen.getByText("18 / 20")).toBeInTheDocument();
  expect(context.getByText("Viewing Run 4")).toBeVisible();
  expect(context.getByText("Recorded outcome: Completed")).toBeVisible();
  expect(within(logPane()).getByText(/Run 4, the agent’s 4th program, is running/)).toBeVisible();
  await user.click(screen.getByRole("button", { name: "Back to latest" }));
  expect(context.getByText("Completed")).toBeVisible();
  expect(context.queryByText("Recorded outcome: Completed")).not.toBeInTheDocument();
  expect(posts(server)).toEqual([]);
});


test("selected blueprint survives the draft reload and reaches the first message", async () => {
  const server = new FakeServer();
  const status = fixtureJson<Status>("status.json");
  status.blueprints = [
    { ...status.blueprint, packages: status.packages, declared_variables: status.declared_variables, blueprint_status: status.blueprint_status },
    { name: "support", path: "/demo/support.yaml", packages: [], declared_variables: [{ name: "ticketId", required: true }], blueprint_status: { version: 9, refused: null } },
  ];
  server.handle = (call) => call.url === "/api/status" ? json(status) : undefined;
  const { user } = await openPage(server);
  await runsLoaded();
  await user.click(screen.getByRole("button", { name: "New session" }));
  await user.selectOptions(screen.getByRole("combobox", { name: "Blueprint" }), "support");
  expect(screen.queryByRole("button", { name: /^Current blueprint/ })).not.toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: "Done configuring model" }));
  await user.type(screen.getByRole("textbox", { name: /ticketId/ }), "ticket_123");
  await user.click(screen.getByRole("button", { name: "Done preparing variables" }));
  await user.click(screen.getByRole("button", { name: "Start session" }));
  expect(screen.getByRole("button", { name: /Current blueprint support · v9/ })).toBeInTheDocument();
  expect(posts(server)).toEqual([]);
  cleanup(); store?.dispose();
  const reloaded = await openPage(server);
  expect(screen.getByRole("button", { name: /Current blueprint support · v9/ })).toBeInTheDocument();
  await reloaded.user.type(screen.getByRole("textbox", { name: "Message the agent" }), "Hello{Enter}");
  await expect.poll(() => server.calls.find((call) => call.url === "/api/agent")?.body).toMatchObject({ blueprint: "support", variables: { ticketId: "ticket_123" } });
});

test("setup inspection opens the selected blueprint source and every package guide before creation", async () => {
  const server = new FakeServer();
  const status = fixtureJson<Status>("status.json");
  status.blueprints = [
    { ...status.blueprint, packages: status.packages, declared_variables: status.declared_variables, blueprint_status: status.blueprint_status },
    { name: "support", path: "/demo/support.yaml", packages: ["@acme/support", "@acme/shared"].map((name) => ({ name, version: "1", origin: "blueprint", importable: true, project: true })), declared_variables: [{ name: "ticketId", required: true }], blueprint_status: { version: 9, refused: null } },
  ];
  server.handle = (call) => {
    if (call.url === "/api/status") return json(status);
    if (call.url === "/api/changes?version=9") return json({ versions: [{ version: 9, at: "2026-10-21T08:00:00Z", classification: "unknown", summary: "Registered support", current: false, text: "kind: blueprint\nname: support\nvariables:\n  ticketId:\n    required: true\n" }], voided: [] });
    if (call.url.startsWith("/api/packages/docs?")) {
      const name = new URL(call.url, "http://fixture").searchParams.get("package")!;
      return json({ package: name, bytes: 10, truncated: false, untrusted: { markdown: "# Guide for " + name } });
    }
    return undefined;
  };
  const { user } = await openPage(server);
  await runsLoaded();
  await user.click(screen.getByRole("button", { name: "New session" }));
  await user.selectOptions(screen.getByRole("combobox", { name: "Blueprint" }), "support");
  await user.click(screen.getByRole("button", { name: "View blueprint" }));
  let dialog = screen.getByRole("dialog", { name: "Blueprint support · v9" });
  expect(await within(dialog).findByText("name: support")).toBeVisible();
  expect(server.requests()).toContain("GET /api/changes?version=9");
  await user.click(within(dialog).getByRole("button", { name: /Project blueprint history/ }));
  await user.click(within(dialog).getByRole("button", { name: "v2" }));
  dialog = screen.getByRole("dialog", { name: "Project blueprint history · v2" });
  expect(within(dialog).getByText(/The selected blueprint is support v9/)).toBeVisible();
  await user.click(within(dialog).getByRole("button", { name: "Close" }));
  await user.click(screen.getByRole("button", { name: "Package guide" }));
  dialog = screen.getByRole("dialog", { name: "Packages in support v9" });
  expect(await within(dialog).findByText("Guide for @acme/support")).toBeVisible();
  expect(server.requests()).toContain("GET /api/packages/docs?package=%40acme%2Fsupport&blueprint=support");
  await user.click(within(dialog).getByRole("button", { name: "@acme/shared" }));
  expect(await within(dialog).findByText("Guide for @acme/shared")).toBeVisible();
  await user.click(within(dialog).getByRole("button", { name: "Close" }));
  await user.selectOptions(screen.getByRole("combobox", { name: "Blueprint" }), "billing");
  await user.click(screen.getByRole("button", { name: "Package guide" }));
  dialog = screen.getByRole("dialog", { name: "Packages in billing v2" });
  expect(await within(dialog).findByText("Guide for @pied-piper/billing")).toBeVisible();
  await user.click(within(dialog).getByRole("button", { name: "Close" }));
  await user.selectOptions(screen.getByRole("combobox", { name: "Blueprint" }), "support");
  await user.click(screen.getByRole("button", { name: "Package guide" }));
  dialog = screen.getByRole("dialog", { name: "Packages in support v9" });
  expect(await within(dialog).findByText("Guide for @acme/support")).toBeVisible();
  expect(server.requests().filter((request) => request === "GET /api/packages/docs?package=%40acme%2Fsupport&blueprint=support")).toHaveLength(1);
  expect(posts(server)).toEqual([]);
});


test("a nondefault Blueprint rerun uses its recorded context and submits only reviewed B variables", async () => {
  const server = new FakeServer();
  const status = fixtureJson<Status>("status.json");
  status.binding.variables = {};
  status.blueprints = [
    { ...status.blueprint, packages: status.packages, declared_variables: status.declared_variables, blueprint_status: status.blueprint_status },
    { name: "reports", path: "/demo/reports.yaml", packages: [], declared_variables: [{ name: "region", required: true }], blueprint_status: { version: 9, refused: null } },
  ];
  const sessions = fixtureJson<SessionList>("sessions.json");
  sessions.sessions = sessions.sessions.map((session) => session.session === SESSION(4)
    ? { ...session, blueprint: "reports", variables: { region: "session-value" } } : session);
  server.sessions = sessions;
  server.handle = (call) => {
    if (call.url === "/api/status") return json(status);
    if (call.url.startsWith("/api/runs/2?")) return json({ ...fixtureJson<Show>("show-2.json"), blueprint: "reports", variables: { region: "recorded-eu" } });
    return undefined;
  };
  const { user } = await openPage(server);
  await runsLoaded();
  await user.click(rowHead("r2"));
  await user.click(within(row("r2")).getByRole("button", { name: "Rerun live…" }));
  const dialog = screen.getByRole("dialog", { name: "Rerun run 2 for real?" });
  const region = within(dialog).getByLabelText("region (required)");
  expect(region).toHaveValue("recorded-eu");
  expect(within(dialog).queryByLabelText(/customerId/)).not.toBeInTheDocument();
  await user.clear(region);
  const go = within(dialog).getByRole("button", { name: "Rerun for real" });
  expect(go).toBeDisabled();
  expect(within(dialog).getByText("Enter region before rerunning.")).toBeVisible();
  await user.click(go);
  expect(posts(server)).toEqual([]);
  await user.type(region, "reviewed-us");
  await user.click(go);
  expect(posts(server).map((call) => [call.url, call.body])).toEqual([
    ["/api/rerun", { run: 2, variables: { region: "reviewed-us" } }],
  ]);
});

test("a rerun with an unavailable recorded Blueprint explains why and sends no action", async () => {
  const server = new FakeServer();
  server.handle = (call) => call.url.startsWith("/api/runs/2?")
    ? json({ ...fixtureJson<Show>("show-2.json"), blueprint: "missing" }) : undefined;
  const { user } = await openPage(server);
  await runsLoaded();
  await user.click(rowHead("r2"));
  await user.click(within(row("r2")).getByRole("button", { name: "Rerun live…" }));
  expect(screen.getByText("Blueprint missing is not available. Reload its context before rerunning.")).toBeVisible();
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  expect(posts(server)).toEqual([]);
});

test.each([
  { label: "browser chat", sources: ["browser-chat"], model: "anthropic:example", open: true },
  { label: "external stand-in", sources: ["stand-in"], model: undefined, open: true },
  { label: "no-model session", sources: ["exec"], model: undefined, open: true },
  { label: "ended session", sources: ["browser-chat"], model: "anthropic:example", open: false },
])("$label has one page New session action in the header", async (state) => {
  const server = new FakeServer();
  const sessions = fixtureJson<SessionList>("sessions.json");
  sessions.sessions = sessions.sessions.map((session) => session.session === SESSION(4) ? { ...session, sources: state.sources, model: state.model, open: state.open } : session);
  server.sessions = sessions;
  await openPage(server);
  await runsLoaded();
  const actions = screen.getAllByRole("button", { name: "New session" });
  expect(actions).toHaveLength(1);
  expect(within(screen.getByRole("banner")).getByRole("button", { name: "New session" })).toBe(actions[0]);
  expect(within(screen.getAllByRole("region", { name: "Conversation" })[0]).queryByRole("button", { name: "New session" })).not.toBeInTheDocument();
});

test("Sessions keeps one contextual New session action within its active modal", async () => {
  const { user } = await openPage();
  await runsLoaded();
  await user.click(screen.getByRole("button", { name: "Sessions" }));
  const dialog = screen.getByRole("dialog", { name: "Sessions" });
  expect(within(dialog).getAllByRole("button", { name: "New session" })).toHaveLength(1);
  for (let i = 0; i < 8; i++) {
    await user.tab();
    expect(dialog.contains(document.activeElement)).toBe(true);
  }
  await user.click(within(dialog).getByRole("button", { name: "New session" }));
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "New session" })).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Configure model" })).toBeVisible();
});

test("a Claude Code session that doesn't bill the subscription asks first, then resends with the confirmation", async () => {
  const server = new FakeServer();
  const confirmation = "api_key via https://proxy.example";
  server.handle = (call) => {
    if (call.url === "/api/status") {
      const s = fixtureJson<Record<string, unknown>>("status.json");
      s.claude_code = { state: "available", version: "2.1.310", source: "api_key", base_url: "https://proxy.example", billed: "an Anthropic API key, sending to https://proxy.example", subscription: false, confirmation };
      return json(s);
    }
    if (call.url !== "/api/agent") return undefined;
    const body = call.body as Record<string, unknown>;
    if (body.confirm_billing === confirmation) return new Response(fixture("agent-new.json"));
    const message = `Claude Code would bill ${confirmation}, not your Claude subscription. Confirm it with \`--confirm-billing "${confirmation}"\`.`;
    return json({ exit: 1, result: { error: { kind: "billing-unconfirmed", message } }, text: "" });
  };
  const { user } = await openPage(server);
  await runsLoaded();
  await user.click(screen.getByRole("button", { name: "New session" }));
  await user.click(within(screen.getByRole("group", { name: "Provider" })).getByRole("button", { name: "Claude Code" }));
  expect(screen.getByText(/^Bills an Anthropic API key, sending to https:\/\/proxy\.example, not your Claude subscription/)).toBeInTheDocument();
  // No key to add for it.
  expect(screen.queryByRole("button", { name: /key$/ })).not.toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: "Opus 5.5" }));
  // Its effort is by the model, not behind the advanced options.
  expect(within(screen.getByRole("group", { name: "Reasoning effort" })).getByRole("button", { name: "Extra high" })).toBeVisible();
  await user.click(screen.getByText("Advanced model options"));
  expect(screen.queryByRole("textbox", { name: /Max output/ })).not.toBeInTheDocument();
  expect(screen.queryByText("Provider settings")).not.toBeInTheDocument();
  await user.type(screen.getByRole("textbox", { name: /Spending limit/ }), "2.5");
  await completeSetup(user);
  await user.click(screen.getByRole("button", { name: "Start session" }));
  expect(within(screen.getByRole("region", { name: "Session" })).getByText("Agent Claude Code · claude-opus-5-5 · spending limit $2.50")).toBeInTheDocument();

  await user.type(screen.getByRole("textbox", { name: "Message the agent" }), "Hello{Enter}");
  const ask = await screen.findByRole("alert");
  expect(ask.textContent).toContain("Claude Code would bill an Anthropic API key, sending to https://proxy.example, not your Claude subscription.");
  // The message stays unsent, in the box.
  expect(screen.getByRole("textbox", { name: "Message the agent" })).toHaveValue("Hello");
  const sent = () => server.calls.filter((c) => c.url === "/api/agent").map((c) => c.body);
  expect(sent()).toEqual([{ task: "Hello", blueprint: "billing", model: "claude-code:claude-opus-5-5", variables: { customerId: "pied_piper" }, max_budget_usd: 2.5 }]);

  // Cancel says how to use the subscription instead, and sends nothing.
  await user.click(within(ask).getByRole("button", { name: "Cancel" }));
  expect(screen.getByText(/^Not sent; your message is still in the box\. To use your Claude subscription instead/)).toBeInTheDocument();
  expect(sent()).toHaveLength(1);

  // Sent again, it's asked again; Confirm resends it with the confirmation.
  await user.type(screen.getByRole("textbox", { name: "Message the agent" }), "{Enter}");
  await user.click(within(await screen.findByRole("alert")).getByRole("button", { name: "Confirm and send" }));
  await vi.waitFor(() => expect(sent()).toHaveLength(3));
  expect(sent()[2]).toEqual({ task: "Hello", blueprint: "billing", model: "claude-code:claude-opus-5-5", variables: { customerId: "pied_piper" }, max_budget_usd: 2.5, confirm_billing: confirmation });
});

test("Claude Code that can't answer says why under the choice, with no key to add", async () => {
  const server = new FakeServer();
  server.handle = (call) => {
    if (call.url !== "/api/status") return undefined;
    const s = fixtureJson<Record<string, unknown>>("status.json");
    s.claude_code = { state: "signed-out", subscription: false, reason: "Claude Code is signed out; run `claude` once to sign in" };
    return json(s);
  };
  const { user } = await openPage(server);
  await runsLoaded();
  await user.click(screen.getByRole("button", { name: "New session" }));
  const option = within(screen.getByRole("group", { name: "Provider" })).getByRole("button", { name: /^Claude Code/ });
  expect(option.textContent).toBe("Claude Code · signed out");
  await user.click(option);
  expect(screen.getByText("Claude Code is signed out; run `claude` once to sign in.")).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: /key$/ })).not.toBeInTheDocument();
});

test("a Claude Code model is picked from the offered ones, or typed in full, even one starting with an offered name", async () => {
  const server = new FakeServer();
  server.handle = (call) => {
    if (call.url !== "/api/status") return undefined;
    const s = fixtureJson<Record<string, unknown>>("status.json");
    s.claude_code = { state: "available", version: "2.1.310", source: "claude.ai", billed: "your Claude login", subscription: true, confirmation: "claude.ai" };
    return json(s);
  };
  const { user } = await openPage(server);
  await runsLoaded();
  await user.click(screen.getByRole("button", { name: "New session" }));
  await user.click(within(screen.getByRole("group", { name: "Provider" })).getByRole("button", { name: "Claude Code" }));
  const models = within(screen.getByRole("group", { name: "Model" }));
  expect(models.getAllByRole("button").map((b) => b.textContent)).toEqual(["Sonnet 5.5", "Opus 5.5", "Fable 5.1", "Haiku 5.5"]);
  const typed = screen.getByRole("textbox", { name: "Or another model" });
  await user.type(typed, "claude-sonnet-5-5-20261001");
  expect(typed).toHaveValue("claude-sonnet-5-5-20261001");
  await user.click(models.getByRole("button", { name: "Fable 5.1" }));
  expect(typed).toHaveValue("");
  expect(models.getByRole("button", { name: "Fable 5.1" })).toHaveAttribute("aria-pressed", "true");
  // Away from Claude Code, effort is an advanced option again; back on it, its default model shows on its button alone.
  const provider = within(screen.getByRole("group", { name: "Provider" }));
  await user.type(typed, "claude-opus-6");
  await user.click(provider.getByRole("button", { name: "OpenAI" }));
  expect(screen.queryByRole("group", { name: "Reasoning effort" })).not.toBeVisible();
  await user.click(provider.getByRole("button", { name: "Claude Code" }));
  expect(screen.getByRole("textbox", { name: "Or another model" })).toHaveValue("");
  expect(screen.getByRole("button", { name: "Sonnet 5.5" })).toHaveAttribute("aria-pressed", "true");
});

test("an answer stopped at a limit says so, as stopped rather than failed, for any agent", async () => {
  for (const [reason, said] of [
    ["turn-limit", "The agent stopped at the most turns one message may take."],
    ["spending-limit", "Claude Code reached this message’s spending limit."],
    ["call-limit", "The agent made as many tool calls as one message allows."],
    ["usage-limit", "Your Claude subscription has no usage left for now."],
  ]) {
    const server = new FakeServer();
    server.handle = (call) => {
      if (call.url !== "/api/agent") return undefined;
      const answer = fixtureJson<{ result: Record<string, unknown> }>("agent-continue.json");
      answer.result.stopped = reason;
      return json(answer);
    };
    const { user } = await openPage(server);
    await runsLoaded();
    await user.type(screen.getByRole("textbox", { name: "Message the agent" }), "Go on{Enter}");
    const line = await screen.findByText(said, { exact: false });
    expect(line.closest(".pg-stopped")).not.toBeNull();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    cleanup();
  }
});
