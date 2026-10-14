import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";

import { App } from "./App";
import { SESSION_KEY } from "./api/client";
import { SIGNED_OUT_LINE } from "./api/login";
import { FakeServer } from "./api/testing/fakeServer";
import { createStore, type PageStore } from "./store/store";

let store: PageStore | null = null;

afterEach(() => {
  cleanup();
  store?.dispose();
  store = null;
  localStorage.clear();
  history.replaceState(null, "", "/");
});

async function show(server: FakeServer, hash: string) {
  history.replaceState(null, "", "/" + hash);
  store = createStore({ fetch: server.fetch, frame: (cb) => setTimeout(cb, 0), retryDelay: () => 0 });
  const started = store;
  await act(async () => {
    render(<App store={started} />);
    await started.start();
  });
}

test("a link's login code becomes a session token, leaves the address bar, and the page opens", async () => {
  const server = new FakeServer();
  await show(server, "#login=good");
  expect(await screen.findByRole("region", { name: "Chat" })).toBeInTheDocument();
  expect(location.hash).toBe("");
  expect(localStorage.getItem(SESSION_KEY)).toBe(server.token);
  expect(screen.getByRole("button", { name: /^Blueprint billing · v2/ })).toBeInTheDocument();
});

test("a refused session shows how to get a new link", async () => {
  const server = new FakeServer();
  localStorage.setItem(SESSION_KEY, "old");
  await show(server, "");
  // The command shows as code, not between backticks.
  const line = await screen.findByText((_, el) => el?.tagName === "P" && el.textContent === SIGNED_OUT_LINE.replaceAll("`", ""));
  expect(line.querySelector("code")?.textContent).toBe("submilli playground open");
  expect(screen.getByText(/You’re signed out/)).toBeInTheDocument();
  expect(localStorage.getItem(SESSION_KEY)).toBeNull();
});

test("with no sessions yet, the page says so and offers New session", async () => {
  const server = new FakeServer();
  server.sessions = { sessions: [], more: 0 };
  localStorage.setItem(SESSION_KEY, server.token);
  await show(server, "");
  expect(await screen.findByText("No sessions yet. Start one with New session.")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "New session" })).toBeInTheDocument();
});
