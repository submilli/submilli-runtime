import { describe, expect, it } from "vitest";
import type { Recheck } from "../api/types";
import { chatFailed, feedStopped, newSessionNotice, recheckSaid } from "./wording";

describe("the page's own lines", () => {
  it("say a refused request won't pass by sending again, but a rate limit or timeout may", () => {
    expect(chatFailed("openai refused the request (HTTP 400): bad", true).note).toMatch(/which the provider refuses/);
    expect(chatFailed("openai refused the request (HTTP 429): slow down", true).note).toMatch(/Send again/);
    expect(chatFailed("openai refused the request (HTTP 408): timeout", false).note).toMatch(/Send again/);
  });

  it("name the session that started, and its first message when known", () => {
    expect(newSessionNotice("8d6321ef-0000-4000-8000-000000000001", "What is my total?")).toBe(
      "Another session started while you were watching this one: 8d6321ef, “What is my total?”.",
    );
    expect(newSessionNotice("8d6321ef-0000-4000-8000-000000000001")).toBe("Another session started while you were watching this one: 8d6321ef.");
  });

  it("keep the server's words out of the stopped feed's line", () => {
    expect(feedStopped()).toBe("Live updates for this session stopped. Reload the page to follow it again.");
  });

  it("say what a recheck found: the first check that changed and how many more, or that nothing did", () => {
    const change = (decision: string) => ({ decision, change: "newly-allowed" as const, caller: "main", capability: "fs.list", now_by: "`list-files` (rule 2 of `main`, line 31)" });
    const result: Recheck = { run: 23, in_force: "2", newly_allowed: 2, newly_denied: 0, different_rule: 0, cant_tell: [], changes: [change("23.1"), change("23.2")] };
    expect(recheckSaid(result)).toEqual({
      head: "Check 23.1 is allowed now (+1 more), under blueprint v2.",
      body: "Decided by `list-files` (rule 2 of `main`, line 31). Nothing ran: to make the call again, ask the agent.",
    });
    expect(recheckSaid({ ...result, newly_allowed: 0, changes: [] }).head).toBe("Nothing changes under blueprint v2.");
    expect(recheckSaid({ ...result, in_force: null, changes: [], cant_tell: ["23.1"] }).head).toBe("Check 23.1’s recording is too cut to decide again.");
  });
});


it("keeps uncertain execution distinct from provider rejection", () => {
  expect(chatFailed("The playground did not answer", false, "unreachable").note).toContain("execution may have started");
  expect(chatFailed("Denied (HTTP 401)", true).note).toContain("provider key");
  expect(chatFailed("insufficient_quota", true).note).toContain("quota or credit");
});
