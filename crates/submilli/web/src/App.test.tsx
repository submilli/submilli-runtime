import { render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import { App } from "./App";
import { SESSION_KEY } from "./session";

afterEach(() => {
  vi.unstubAllGlobals();
  localStorage.clear();
});

test("a link's login code becomes a session token and leaves the address bar", async () => {
  history.replaceState(null, "", "/#login=abc");
  const fetch = vi.fn(async (url: string) =>
    url === "/api/login"
      ? Response.json({ session_token: "tok" })
      : Response.json({ running: true }),
  );
  vi.stubGlobal("fetch", fetch);
  render(<App />);
  expect(await screen.findByText(/"running": true/)).toBeInTheDocument();
  expect(location.hash).toBe("");
  expect(localStorage.getItem(SESSION_KEY)).toBe("tok");
  expect(fetch).toHaveBeenLastCalledWith("/api/status", {
    headers: { authorization: "Bearer tok" },
  });
});

test("a refused session is dropped and the page says how to get a new link", async () => {
  localStorage.setItem(SESSION_KEY, "old");
  vi.stubGlobal("fetch", vi.fn(async () => new Response(null, { status: 401 })));
  render(<App />);
  expect(
    await screen.findByText("Run `submilli playground open` for a new link."),
  ).toBeInTheDocument();
  expect(localStorage.getItem(SESSION_KEY)).toBeNull();
});
