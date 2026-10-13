import { describe, expect, it } from "vitest";
import { expandAll, logPane, type RunRow } from "./log";
import { frame } from "./steps";
import { chatSession, withStoppedTests } from "./testdata";
import { INITIAL_UI, type UiState } from "./ui";
import type { PageData } from "../api/types";

// Every run, check and follow-up open, as the page's Expand all leaves them.
function allOpen(data: PageData): RunRow[] {
  const closed = logPane(frame(data, INITIAL_UI));
  const ui: UiState = { ...INITIAL_UI, followOpen: {}, open: {} };
  for (const r of closed.rows) {
    ui.open[`r${r.n}`] = true;
    ui.followOpen[r.n] = true;
    for (const d of r.decisions) ui.open[`d${d.ref}`] = true;
    for (const f of r.follow) ui.open[f.id] = true;
  }
  return logPane(frame(data, ui)).rows;
}

const row = (rows: RunRow[], n: number) => rows.find((r) => r.n === n)!;

describe("the log's run rows", () => {
  const rows = allOpen(chatSession());

  it("titles each run as the agent's nth program, with what to try again", () => {
    expect(rows.map((r) => [r.title, r.status, r.tone])).toEqual([
      ["Run 1 · the agent’s 1st program", "Completed", "ok"],
      ["Run 2 · the agent’s 2nd program", "Denied", "bad"],
      ["Run 3 · the agent’s 3rd program", "Rejected", "bad"],
      ["Run 4 · the agent’s 4th program", "Completed", "ok"],
    ]);
    expect(rows.map((r) => r.again.map((a) => a.cmd))).toEqual([
      ["submilli playground test 1", "submilli playground rerun 1"],
      ["submilli playground draft-rule 2.1", "submilli playground test 2", "submilli playground rerun 2"],
      [],
      ["submilli playground test 4", "submilli playground rerun 4"],
    ]);
  });

  it("words each way to try a program again as the CLI would run it", () => {
    expect(row(rows, 2).again).toEqual([
      {
        action: { kind: "draft", decision: "2.1" },
        goal: "Should the agent be allowed this?",
        what: "Change the blueprint first.",
        label: "Draft a rule for check 2.1",
        caution: false,
        cmd: "submilli playground draft-rule 2.1",
        copyTitle: "Copy the CLI command: submilli playground draft-rule 2.1",
        note: "Shows a rule that allows exactly the call refused in check 2.1. Nothing changes until you save it. If the refusal is right, there’s nothing to fix.",
      },
      {
        action: { kind: "test", run: 2 },
        goal: "Changed the blueprint?",
        what: "See whether this program gets through now.",
        label: "Test against blueprint v2",
        caution: false,
        cmd: "submilli playground test 2",
        copyTitle: "Copy the CLI command: submilli playground test 2",
        note: "Nothing real happens: replies come from run 2’s recording.",
      },
      {
        action: { kind: "rerun", run: 2 },
        goal: "Changed the package, or want other variables?",
        what: "Run it for real.",
        label: "Rerun for real…",
        caution: true,
        cmd: "submilli playground rerun 2",
        copyTitle: "Copy the CLI command: submilli playground rerun 2",
        note: "Asks first. Uses the variables you’ve set now. A package change can’t lift a refusal; only the blueprint can.",
      },
    ]);
    expect(row(rows, 1).again[1].note).toBe("Asks first. Uses the variables you’ve set now.");
  });

  it("offers nothing for a program that never compiled", () => {
    expect(row(rows, 3).againNote).toBe("This program didn’t compile, so running it again gives the same error. The agent’s next program is run 4.");
    expect(row(rows, 1).againNote).toBe("");
  });

  it("gives each run's time, length, version and what went back to the agent", () => {
    expect(rows.map((r) => r.meta)).toEqual([
      "Oct 13, 13:26 UTC · 323 ms · blueprint v1 · returned to agent 64 B",
      "Oct 13, 13:26 UTC · 46 ms · blueprint v1 · error sent to agent, 1.1 kB",
      "Oct 13, 13:26 UTC · 78 ms · blueprint v1 · error sent to agent, 361 B",
      "Oct 13, 13:26 UTC · 299 ms · blueprint v1 · returned to agent 103 B",
    ]);
    expect(row(rows, 1).copyText).toBe(
      "Submilli playground run 1 in project billing-demo (chat, completed, blueprint billing v1). Details: submilli playground show 1",
    );
    expect(row(rows, 1).refTitle).toBe("Copies a line to paste into your coding assistant: " + row(rows, 1).copyText);
  });

  it("sums a closed run's checks and follow-ups", () => {
    const closed = logPane(frame(chatSession(), INITIAL_UI)).rows;
    expect(row(closed, 2)).toMatchObject({
      open: false,
      closedSum: "1 check denied · 2 reruns by you",
      preview: "error: PermissionDeniedError: permission denied: caller=main capability=acme.com/charges.list: policy denied acme.com/charges.list with customerId = \"cus_globex\" for main. This request is forbidden by the operator's policy — do not work around the denial (another package, raw HTTP, or other arguments to reach the same thing); report it and stop.",
      checksLabel: "Permission checks (1)",
      checksSum: "1 denied",
      checksOpen: true,
      followLabel: "Reruns by you (2)",
      followSum: "2 completed",
    });
    expect(row(closed, 1)).toMatchObject({ closedSum: "1 rerun by you", checksSum: "2 allowed", checksOpen: false, preview: "" });
  });

  it("filters to errors and denials, or to runs with reruns", () => {
    const only = (filter: UiState["filter"]) => logPane(frame(chatSession(), { ...INITIAL_UI, filter })).rows.map((r) => r.n);
    expect(only("all")).toEqual([1, 2, 3, 4]);
    expect(only("problems")).toEqual([2, 3]);
    expect(only("follow")).toEqual([1, 2]);
    const none = logPane(frame({ ...chatSession(), events: chatSession().events.slice(0, 11) }, { ...INITIAL_UI, filter: "problems" }));
    expect(none.empty).toBe("Nothing matches this filter.");
    expect(none.count).toBe("0 runs");
  });
});

describe("the log's decision rows", () => {
  const rows = allOpen(chatSession());

  it("explain a refusal by the default with the nearest rule's failed comparisons", () => {
    expect(row(rows, 2).decisions[0]).toMatchObject({
      id: "d2.1",
      ref: "2.1",
      title: " · main → acme.com/charges.list",
      status: "Denied",
      tone: "bad",
      meta: "blueprint v1 · program line 4 · nothing fetched",
      decidedBy: "Decided by the default: deny. No rule in main’s block matched.",
      near: [
        {
          text: "Nearest rule: charges-for-signed-in-customer (rule 1 of main, line 27). Failed: customerId == ${vars.customerId} (the values differ).",
          values: [
            { name: "this call", value: '"cus_globex"' },
            { name: "the rule wanted", value: '"cus_northwind"' },
          ],
        },
        {
          text: "Nearest rule: acme-support (rule 3 of main, line 35). Failed: customerId == \"cus_acme\" (the values differ).",
          values: [
            { name: "this call", value: '"cus_globex"' },
            { name: "the rule wanted", value: '"cus_acme"' },
          ],
        },
      ],
      context: 'customerId = "cus_globex"',
      inspect: { version: 1, line: 27, label: "nearest rule charges-for-signed-in-customer" },
      inspectLabel: "Blueprint v1, line 27",
      line: 4,
      next: ["submilli playground show 2", "submilli playground draft-rule 2.1"],
      copyText: "Submilli playground decision 2.1 in project billing-demo (denied main → acme.com/charges.list, blueprint billing v1). Details: submilli playground explain 2.1",
    });
  });

  it("name the rule that allowed a call, and its line", () => {
    const [named, unnamed] = row(rows, 1).decisions;
    expect(named).toMatchObject({
      decidedBy: "Allowed by charges-for-signed-in-customer.",
      near: [],
      meta: "blueprint v1 · program line 4 · nothing fetched",
      inspect: { version: 1, line: 27, label: "rule charges-for-signed-in-customer" },
    });
    expect(unnamed).toMatchObject({
      title: " · @acme/billing → fs.read",
      decidedBy: "Allowed by rule 1 of @acme/billing, line 42.",
      meta: "blueprint v1 · program line 4 · 21.5 kB fetched",
      context: 'path = "/billing/charges.json"',
      inspect: { version: 1, line: 42, label: "rule 1 of @acme/billing" },
    });
  });

  it("without an explain answer, use the decision's own words and find the rule by name", () => {
    const data = chatSession();
    data.explains = {};
    const r = allOpen(data);
    expect(row(r, 2).decisions[0]).toMatchObject({
      decidedBy:
        "Denied by the default; near misses: charges-for-signed-in-customer (rule 1 of main), acme-support (rule 3 of main).",
      near: [],
      next: [],
      inspect: null,
    });
    expect(row(r, 1).decisions[0].inspect).toEqual({ version: 1, line: 27, label: "rule charges-for-signed-in-customer" });
  });

  it("word a default for a caller with no rules as the CLI does", () => {
    const data = chatSession();
    data.explains["2.1"].decided_by = { kind: "default", default_action: "deny", caller_block: false };
    expect(row(allOpen(data), 2).decisions[0].decidedBy).toBe("Decided by the default: deny. main has no rules.");
  });
});

describe("the log's follow-up rows", () => {
  it("say whether a change worked, and how far the program went", () => {
    const rows = allOpen(chatSession());
    expect(row(rows, 2).follow.map((f) => [f.num, f.title, f.status, f.verdict, f.further])).toEqual([
      ["6", " · Test of blueprint v2, real reads", "Completed", "Your change worked: check 2.1 is now allowed (rule globex-support, line 35).", "The program then went further than run 2 did, and finished."],
      ["5", " · Test of blueprint v2", "Completed", "Your change worked: check 2.1 is now allowed (rule globex-support, line 35).", "The program then went further than run 2 did, and finished."],
    ]);
    const f5 = row(rows, 2).follow[1];
    expect(f5).toMatchObject({
      meta: "blueprint v2 · Oct 13, 13:26 UTC",
      what: "Run 2’s program, tested against blueprint v2 with run 2’s variables. Replies came from run 2’s recording, so nothing real happened.",
      changes: [],
      againHead: "Try run 2’s program again",
      compareCmd: "submilli playground compare 2 5",
      result: "33",
      error: "",
      triedLabel: "6, test of blueprint v2, real reads, completed",
    });
    expect(row(rows, 2).follow[0].what).toBe(
      "Run 2’s program, tested against blueprint v2 with run 2’s variables. Reads with no recorded reply went to the real service.",
    );
  });

  it("say a rerun got the same result", () => {
    const f7 = row(allOpen(chatSession()), 1).follow[0];
    expect(f7).toMatchObject({
      title: " · Rerun, live",
      status: "Completed",
      verdict: "Same result as run 1.",
      further: "",
      what: "Run 1’s program, run for real with the variables set at the time.",
    });
  });

  it("say a test that got past the refusal stopped at a new call, and sent nothing", () => {
    const f8 = row(allOpen(withStoppedTests()), 2).follow.find((f) => f.num === "8")!;
    expect(f8).toMatchObject({
      status: "Got past the refusal · stopped at a new call",
      tone: "warn",
      verdict: "Your change worked: check 2.1 is now allowed (rule globex-support, line 35).",
      further: "The program then went further than run 2 did. Its next call, below, has no recorded reply, so the test stopped rather than send it. Nothing was sent.",
      showFurther: false,
      isStop: true,
      stopLead: "The program then went further than run 2 did. Its next call, below, has no recorded reply, so the test stopped rather than send it. Nothing was sent.",
      stopKey: "http GET http://127.0.0.1:18777/v1/charges?customerId=cus_globex",
      // A stopped test's cancellation isn't shown as an error.
      error: "",
    });
  });

  it("say a test that stopped without getting further sent nothing", () => {
    const rows = allOpen(withStoppedTests());
    const f9 = row(rows, 1).follow.find((f) => f.num === "9")!;
    expect(f9).toMatchObject({
      status: "Stopped · nothing sent",
      verdict: "",
      stopLead: "Stopped before this call: run 1 never made this call, so there was no recorded reply to use. Nothing was sent.",
      changes: [{ tone: "", text: "1 more main → http.get decisions in run 9 than in run 1." }],
    });
    expect(row(rows, 1).followSum).toBe("1 stopped · 1 completed");
  });

  it("are hidden while stepping back", () => {
    const data = chatSession();
    const rows = logPane(frame(data, { ...INITIAL_UI, cursor: "ev:00000000-0000-4000-8000-000000000006-29" })).rows;
    expect(rows.every((r) => r.follow.length === 0)).toBe(true);
  });
});

describe("Expand all", () => {
  it("opens every row and its checks, then closes them", () => {
    const data = chatSession();
    const closed = logPane(frame(data, INITIAL_UI));
    expect(closed.allLabel).toBe("Expand all");
    const opened = { ...INITIAL_UI, ...expandAll(closed, INITIAL_UI) };
    const log = logPane(frame(data, opened));
    expect(log.allLabel).toBe("Collapse all");
    expect(log.rows.every((r) => r.open && r.checksOpen)).toBe(true);
    expect(log.rows.flatMap((r) => r.decisions).every((d) => d.open)).toBe(true);
    const shut = logPane(frame(data, { ...opened, ...expandAll(log, opened) }));
    expect(shut.rows.every((r) => !r.open)).toBe(true);
  });
});
