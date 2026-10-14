import { describe, expect, it } from "vitest";
import { chatFailed, feedStopped, newSessionNotice } from "./wording";

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
});
