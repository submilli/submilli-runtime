import { expect, test } from "vitest";

import { readLink, runHash } from "./login";

test("a login link carries its code, and a run link the run and decision", () => {
  expect(readLink("#login=abc")).toEqual({ code: "abc", requested: null });
  expect(readLink("#run=2&decision=1")).toEqual({
    code: null,
    requested: { run: 2, decision: 1 },
  });
  expect(readLink("#login=abc&run=5")).toEqual({
    code: "abc",
    requested: { run: 5, decision: null },
  });
  expect(readLink("")).toEqual({ code: null, requested: null });
  expect(readLink("#run=two")).toEqual({ code: null, requested: null });
});

test("a requested run goes back into the hash without the code", () => {
  expect(runHash({ run: 2, decision: 1 })).toBe("#run=2&decision=1");
  expect(runHash({ run: 5, decision: null })).toBe("#run=5");
  expect(runHash(null)).toBe("");
});
