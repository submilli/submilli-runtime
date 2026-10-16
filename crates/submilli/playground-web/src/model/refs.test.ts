import { describe, expect, it } from "vitest";
import { agentLabel, copyNotice, ctx, end, ordinal, plainBy, settingsLabel, short, size, tone, when } from "./format";
import { decisionRef, forAsst, namesOf, programRef, ruleRef, runRef, sessionRef } from "./refs";
import { chatSession } from "./testdata";

const data = chatSession();
const names = namesOf(data.status);

describe("copy-for-assistant lines", () => {
  it("name the project's directory and the blueprint from status", () => {
    expect(names).toEqual({ project: "billing-demo", blueprint: "billing" });
  });

  it("each read as one line with the command that prints the item", () => {
    expect(runRef(names, data.runs[2])).toBe(
      "Submilli playground, run 2 (details - project: `billing-demo`, blueprint: `billing v1`, outcome: `denied at check 2.1`, see_with: `submilli playground show 2`)",
    );
    expect(runRef(names, data.runs[3])).toBe(
      "Submilli playground, run 3 (details - project: `billing-demo`, blueprint: `billing v1`, outcome: `rejected: its program didn’t compile`, see_with: `submilli playground show 3`)",
    );
    expect(decisionRef(names, data.runs[1], data.runs[1].decisions[1])).toBe(
      "Submilli playground, check 1.2 (details - project: `billing-demo`, blueprint: `billing v1`, outcome: `allowed`, call: `@acme/billing → fs.read`, see_with: `submilli playground explain 1.2`)",
    );
    expect(programRef(names, 4)).toBe(
      "Submilli playground, the program of run 4 (details - project: `billing-demo`, see_with: `submilli playground show 4 --program`)",
    );
    expect(sessionRef(names, "5e550000-0000-4000-8000-000000000004")).toBe(
      "Submilli playground, session 5e550000 (details - project: `billing-demo`, see_with: `submilli playground runs --session 5e550000-0000-4000-8000-000000000004`)",
    );
    expect(ruleRef(names, 2, 35, "rule globex-support")).toBe(
      "Submilli playground, blueprint line 35 (details - project: `billing-demo`, blueprint: `billing v2`, rule: `globex-support`, see_with: `submilli playground changes --version 2`)",
    );
    expect(ruleRef(names, 2, 35, "")).toBe(
      "Submilli playground, blueprint line 35 (details - project: `billing-demo`, blueprint: `billing v2`, see_with: `submilli playground changes --version 2`)",
    );
  });

  it("say on hover what they copy", () => {
    expect(forAsst("x")).toBe("Copies a line to paste into your coding assistant: x");
    expect(forAsst("")).toBe("");
  });
});

describe("formatting", () => {
  it("words an outcome as the log does", () => {
    expect(end({ kind: "completed" })).toBe("completed");
    expect(end({ kind: "denied", decision: "2.1" })).toBe("denied");
    expect(end({ kind: "failed", error: "compile_error" })).toBe("rejected");
    expect(end({ kind: "failed", error: "runtime_error" })).toBe("failed");
    expect(end({ kind: "failed", error: "something_new" })).toBe("failed");
    expect(end({ kind: "stopped" })).toBe("stopped");
    expect(["completed", "stopped", "denied"].map(tone)).toEqual(["ok", "warn", "bad"]);
  });

  it("sizes, times, ordinals and contexts", () => {
    expect([0, 999, 1000, 1061, 21530, 43060].map(size)).toEqual(["0 B", "999 B", "1 kB", "1.1 kB", "21.5 kB", "43.1 kB"]);
    expect(when("2026-10-13T13:26:56Z")).toBe("Oct 13, 13:26 UTC");
    expect(when("2026-10-14T00:05:00Z")).toBe("Oct 14, 00:05 UTC");
    expect([1, 2, 3, 4, 11, 12, 13, 21, 22, 111].map(ordinal)).toEqual(["1st", "2nd", "3rd", "4th", "11th", "12th", "13th", "21st", "22nd", "111th"]);
    expect(ctx({ customerId: "cus_globex", limit: 3 })).toBe('customerId = "cus_globex", limit = 3');
    expect(plainBy("by `globex-support`")).toBe("by globex-support");
    expect(short("5e550000-0000-4000-8000-000000000004")).toBe("5e550000");
  });

  it("names the agent and its settings as people say them", () => {
    expect(agentLabel("anthropic:claude-sonnet-5-5")).toBe("Claude · claude-sonnet-5-5");
    expect(agentLabel("openai:gpt-5")).toBe("OpenAI · gpt-5");
    expect(agentLabel("acme:m1")).toBe("acme · m1");
    expect(agentLabel(undefined)).toBe("");
    expect(settingsLabel({ effort: "low", max_output: 2000, provider: { temperature: 0.2, top_p: 1 } })).toBe(
      "effort low, max output 2000, provider settings temperature=0.2 (+1 more)",
    );
  });

  it("says what a copy put on the clipboard, or leaves it up to copy by hand", () => {
    expect(copyNotice("submilli playground test 2", "command", true)).toEqual({
      head: "Copied command:",
      body: "submilli playground test 2",
      auto: true,
    });
    expect(copyNotice("x", "reference", false)).toEqual({ head: "Copy this line for your assistant:", body: "x", auto: false });
  });
});
