import { afterEach, expect, test, vi } from "vitest";

import type { AgentResult } from "./types";
import { Client, ReadError, SESSION_KEY, SignedOutError } from "./client";
import { FakeServer } from "./testing/fakeServer";

afterEach(() => localStorage.clear());

function signedIn() {
  const server = new FakeServer();
  localStorage.setItem(SESSION_KEY, server.token);
  const onSignedOut = vi.fn();
  return { server, onSignedOut, client: new Client({ fetch: server.fetch, onSignedOut }) };
}

test("a read sends the stored token as a bearer header and returns the answer", async () => {
  const { server, client } = signedIn();
  const status = await client.read<{ project: string }>("/api/status");
  expect(status.project).toBe("/home/dev/billing-demo");
  expect(server.calls[0].headers.get("authorization")).toBe(`Bearer ${server.token}`);
});

test("a refused cancel comes back as a refusal with its message, not a success", async () => {
  const { client } = signedIn();
  const answer = await client.act("/api/cancel", { run: 1 });
  expect(answer).toEqual({
    ok: false,
    kind: "not-in-flight",
    message: "run 1 already finished; there is nothing to cancel",
  });
});

test("an accepted action returns its result", async () => {
  const { client } = signedIn();
  const answer = await client.act<AgentResult>("/api/agent", { task: "hi" });
  expect(answer.ok).toBe(true);
  if (answer.ok) expect(answer.result.session).toBe("5e550000-0000-4000-8000-000000000004");
});

test("a 401 from any route clears the token and reports the page signed out", async () => {
  const { server, client, onSignedOut } = signedIn();
  server.token = "rotated";
  await expect(client.read("/api/sessions")).rejects.toBeInstanceOf(SignedOutError);
  expect(localStorage.getItem(SESSION_KEY)).toBeNull();
  expect(onSignedOut).toHaveBeenCalledOnce();
  await expect(client.act("/api/cancel", { run: 1 })).rejects.toBeInstanceOf(SignedOutError);
});

test("a read that isn't found carries the route's code and message", async () => {
  const { client } = signedIn();
  const error = await client
    .read("/api/packages/source?package=%40acme%2Fbilling&module=nope")
    .catch((e: unknown) => e);
  expect(error).toBeInstanceOf(ReadError);
  expect(error).toMatchObject({ status: 404, code: "unknown_module" });
});
