import { describe, expect, it } from "vitest";
import { duration, tone } from "./format";
import { expandAll, logPane, uncut, type RunRow } from "./log";
import { frame } from "./steps";
import { chatSession, withStoppedTests } from "./testdata";
import { INITIAL_UI, type UiState } from "./ui";
import type { PageData } from "../api/types";

const DENIED_CHECK_REF =
  "Submilli playground, check 2.1 (details - project: `billing-demo`, blueprint: `billing v1`, outcome: `denied`, call: `main → pied-piper.example/charges.list`, see_with: `submilli playground explain 2.1`, draft_with: `submilli playground draft-rule 2.1`)";

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
    expect(rows.map((r) => r.again.map((a) => a.copy.text))).toEqual([
      ["submilli playground test 1", "submilli playground rerun 1"],
      [DENIED_CHECK_REF, "submilli playground test 2", "submilli playground rerun 2"],
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
        copy: {
          label: "Copy for assistant",
          text: DENIED_CHECK_REF,
          title: "Copies a line to paste into your coding assistant: " + DENIED_CHECK_REF,
          what: "reference",
        },
        note: "Shows a rule that allows exactly the call refused in check 2.1. Nothing changes until you save it. If the refusal is right, there’s nothing to fix.",
      },
      {
        action: { kind: "test", run: 2 },
        goal: "Changed the blueprint?",
        what: "See whether this program gets through now.",
        label: "Test against blueprint v2",
        caution: false,
        copy: { label: "Copy CLI command", text: "submilli playground test 2", title: "Copy the CLI command: submilli playground test 2", what: "command" },
        note: "Nothing real happens: replies come from run 2’s recording, and it keeps run 2’s values.",
      },
      {
        action: { kind: "rerun", run: 2 },
        goal: "Changed the package, or want other variables?",
        what: "Run it for real.",
        label: "Rerun for real…",
        caution: true,
        copy: { label: "Copy CLI command", text: "submilli playground rerun 2", title: "Copy the CLI command: submilli playground rerun 2", what: "command" },
        note: "Asks first. Uses the variables you’ve set now. A package change can’t lift a refusal; only the blueprint can.",
      },
    ]);
    expect(row(rows, 1).again[1].note).toBe("Asks first. Uses the variables you’ve set now.");
  });

  it("says nothing went back to the agent for a run that was cancelled", () => {
    const data = chatSession();
    data.runs[2] = { ...data.runs[2], outcome: { kind: "cancelled" } };
    expect(row(allOpen(data), 2).meta).toBe("Oct 13, 13:26 UTC · 46 ms · blueprint v1");
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
      "Submilli playground, run 1 (details - project: `billing-demo`, blueprint: `billing v1`, outcome: `completed`, see_with: `submilli playground show 1`)",
    );
    expect(row(rows, 1).refTitle).toBe("Copies a line to paste into your coding assistant: " + row(rows, 1).copyText);
  });

  it("sums a closed run's checks and follow-ups", () => {
    const closed = logPane(frame(chatSession(), INITIAL_UI)).rows;
    expect(row(closed, 2)).toMatchObject({
      open: false,
      closedSum: "1 check denied · 2 reruns by you",
      preview: "error: PermissionDeniedError: permission denied: caller=main capability=pied-piper.example/charges.list: policy denied pied-piper.example/charges.list with customerId = \"cus_globex\" for main. This request is forbidden by the operator's policy — do not work around the denial (another package, raw HTTP, or other arguments to reach the same thing); report it and stop.",
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
    expect(none.count).toBe("0 of 1 run");
  });
});

describe("the log's decision rows", () => {
  const rows = allOpen(chatSession());

  it("name every line a repeated check was asked from, and all it fetched", () => {
    const data = chatSession();
    const show = data.runs[1];
    data.runs[1] = { ...show, decisions: [{ ...show.decisions[0], refs: ["1.1", "1.2"] }] };
    // The second call asks from another line.
    data.events = data.events.map((e) => {
      const body = e.body as { event?: { kind?: string; call_index?: number; line?: { line: number } } };
      return e.run === 1 && body.event?.kind === "call-started" && body.event.call_index === 1
        ? ({ ...e, body: { ...body, event: { ...body.event, line: { line: 9, column: 3 } } } } as typeof e)
        : e;
    });
    expect(row(allOpen(data), 1).decisions[0].meta).toBe("blueprint v1 · program lines 4 and 9 · 21.5 kB fetched");
  });

  it("explain a refusal by the default with the nearest rule's failed comparisons", () => {
    expect(row(rows, 2).decisions[0]).toMatchObject({
      id: "d2.1",
      ref: "2.1",
      title: " · main → pied-piper.example/charges.list customerId = cus_globex",
      hint: 'customerId = "cus_globex"',
      status: "Denied",
      tone: "bad",
      meta: "blueprint v1 · program line 4 · nothing fetched",
      decidedBy: "Decided by the default: deny. No rule in main’s block matched.",
      near: [
        {
          text: "Nearest rule: charges-for-signed-in-customer (rule 1 of main, line 27). Failed: customerId == ${vars.customerId} (the values differ).",
          values: [
            { name: "this call", value: '"cus_globex"' },
            { name: "the rule wanted", value: '"pied_piper"' },
          ],
        },
        {
          text: "Nearest rule: pied-piper-support (rule 3 of main, line 35). Failed: customerId == \"cus_pied-piper\" (the values differ).",
          values: [
            { name: "this call", value: '"cus_globex"' },
            { name: "the rule wanted", value: '"cus_pied-piper"' },
          ],
        },
      ],
      context: 'customerId = "cus_globex"',
      inspect: { version: 1, line: 27, label: "nearest rule charges-for-signed-in-customer" },
      inspectLabel: "Blueprint v1, line 27",
      line: 4,
      next: ["submilli playground show 2", "submilli playground draft-rule 2.1"],
      copyText: DENIED_CHECK_REF,
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
      title: " · @pied-piper/billing → fs.read /billing/charges.json",
      hint: 'path = "/billing/charges.json"',
      decidedBy: "Allowed by rule 1 of @pied-piper/billing, line 42.",
      meta: "blueprint v1 · program line 4 · 21.5 kB fetched",
      context: 'path = "/billing/charges.json"',
      inspect: { version: 1, line: 42, label: "rule 1 of @pied-piper/billing" },
    });
  });

  it("without an explain answer, use the decision's own words and find the rule by name", () => {
    const data = chatSession();
    data.explains = {};
    const r = allOpen(data);
    expect(row(r, 2).decisions[0]).toMatchObject({
      decidedBy:
        "Denied by the default; near misses: charges-for-signed-in-customer (rule 1 of main), pied-piper-support (rule 3 of main).",
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

  it("don't credit a blueprint change when a rerun got past a check with other variables", () => {
    const data = chatSession();
    data.compares[7] = { ...data.compares[7], changes: data.compares[5].changes.filter((c) => c.kind === "newly-allowed") };
    data.runs[7] = { ...data.runs[7], blueprint_version: data.runs[1].blueprint_version, variables: { customerId: "cus_globex" } };
    const f7 = row(allOpen(data), 1).follow[0];
    expect(f7.verdict).toBe("Check 2.1 is now allowed (rule globex-support, line 35).");
    expect(f7.values).toEqual({
      lead: "This rerun used other values than run 1: ",
      changes: [{ name: "customerId", was: "pied_piper", now: "cus_globex" }],
      tail: ".",
    });
  });

  it("say a test kept the run's values when the ones bound now differ, all of them, however many", () => {
    const data = chatSession();
    data.status = { ...data.status!, binding: { variables: { customerId: "cus_globex", region: "us", tier: "gold" }, secrets: [] } };
    const f5 = row(allOpen(data), 2).follow[1];
    expect(f5.values).toEqual({
      lead: "This test kept run 2’s values; the ones bound now differ: ",
      changes: [
        { name: "customerId", was: "pied_piper", now: "cus_globex" },
        { name: "region", was: "not set", now: "us" },
        { name: "tier", was: "not set", now: "gold" },
      ],
      tail: ". Rerun for real uses the bound ones.",
    });
    // Bound as the run was, nothing differs and nothing is said.
    data.status = { ...data.status!, binding: { variables: { ...data.runs[2].variables }, secrets: [] } };
    expect(row(allOpen(data), 2).follow[1].values).toBeNull();
  });

  it("say a rerun got the same result", () => {
    const f7 = row(allOpen(chatSession()), 1).follow[0];
    expect(f7).toMatchObject({
      title: " · Rerun, live",
      status: "Completed",
      verdict: "Same result as run 1.",
      further: "",
      what: "Run 1’s program, run for real with the variables set at the time: customerId=pied_piper.",
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
    // Reruns by you too, and each rerun in them.
    const withReruns = log.rows.filter((r) => r.follow.length);
    expect(withReruns.length).toBeGreaterThan(0);
    expect(withReruns.every((r) => r.followOpen && r.follow.every((f) => f.open))).toBe(true);
    const shut = logPane(frame(data, { ...opened, ...expandAll(log, opened) }));
    expect(shut.rows.every((r) => !r.open)).toBe(true);
  });
});

describe("a result the server cut", () => {
  it("says how much shows, without the CLI's flag", () => {
    expect(uncut("abc…[4000 of 280000 bytes shown; --include-payloads shows all]")).toEqual({ text: "abc…", cut: "Shows 4 kB of 280 kB.", less: false });
    expect(uncut("whole")).toEqual({ text: "whole", cut: "", less: false });
  });

  it("once read whole, shows it all with Show less, or its first part again", () => {
    const long = "x".repeat(5000);
    expect(uncut(long, true)).toEqual({ text: long, cut: "", less: true });
    expect(uncut(long, false)).toEqual({ text: "x".repeat(4000) + "…", cut: "Shows 4 kB of 5 kB.", less: false });
  });
});

describe("runs as they happen", () => {
  // Run 99 started and has made no call yet: it has no step of its own.
  const started = (data: PageData) => {
    const template = data.events.find((e) => "event" in e.body && e.body.event.kind === "run-started")!;
    data.events = [...data.events, { ...template, session_seq: 999, event_id: "started-99", run: 99, position: { ...template.position, at_micros: 9e15, run: 99 } }];
    return data;
  };

  it("say a run that hasn't made a call yet is running, after the runs that ended", () => {
    const log = logPane(frame(started(chatSession()), INITIAL_UI));
    expect(log.runningRun).toBe(99);
    expect(log.runningNote).toBe("Run 99, the agent’s 5th program, is running. It appears here when it ends.");
  });

  it("name a run the coding assistant made as theirs, not the agent's", () => {
    const data = chatSession();
    data.runs[3] = { ...data.runs[3], source: "assistant" };
    const rows = logPane(frame(data, INITIAL_UI)).rows;
    expect(row(rows, 3).title).toBe("Run 3 · run by your assistant");
    expect(row(rows, 4).title).toBe("Run 4 · the agent’s 3rd program");
  });
});

describe("how a run's end and length are shown", () => {
  it("gives a run you cancelled its own quiet colour, apart from failures", () => {
    expect([tone("completed"), tone("stopped"), tone("cancelled"), tone("failed"), tone("denied")]).toEqual(["ok", "warn", "quiet", "bad", "bad"]);
  });

  it("gives milliseconds under a second, then seconds", () => {
    expect([duration(323), duration(999), duration(1000), duration(87005)]).toEqual(["323 ms", "999 ms", "1.0 s", "87.0 s"]);
  });
});

describe("a run that was one of the agent's file tools", () => {
  it("is named as that call, not as a program, and doesn't take a program's number", () => {
    const data = chatSession();
    data.runs[1] = { ...data.runs[1], entry: "mcp:list_files" };
    const rows = logPane(frame(data, INITIAL_UI)).rows;
    expect(row(rows, 1).title).toBe("Run 1 · the agent’s list_files call customerId = pied_piper");
    expect(row(rows, 2).title).toBe("Run 2 · the agent’s 1st program");
  });

  it("offers a rule and a recheck for a call the blueprint refused: it has no program to test or rerun", () => {
    const data = chatSession();
    data.runs[2] = { ...data.runs[2], entry: "mcp:list_files" };
    const denied = row(logPane(frame(data, INITIAL_UI)).rows, 2);
    expect(denied.thing).toBe("call");
    expect(denied.again.map((a) => a.copy.text)).toEqual([DENIED_CHECK_REF, "submilli playground recheck 2"]);
    expect(denied.again[1]).toMatchObject({ action: { kind: "recheck", run: 2 }, label: "Check against blueprint v2", note: "Nothing runs. To make the call again, ask the agent." });
  });

  it("names the path each call asked for, so two calls to one tool tell apart, and offers nothing for one that went through", () => {
    const data = chatSession();
    data.runs[1] = { ...data.runs[1], entry: "mcp:submilli__files__list", untrusted: { contexts: { "1.1": { path: "/", recursive: false } } } };
    data.runs[2] = { ...data.runs[2], entry: "mcp:submilli__files__list", untrusted: { contexts: { "2.1": { path: "/billing", recursive: true } } } };
    const rows = logPane(frame(data, INITIAL_UI)).rows;
    expect(row(rows, 1).title).toBe("Run 1 · the agent’s files.list call /");
    expect(row(rows, 2).title).toBe("Run 2 · the agent’s files.list call /billing");
    expect(row(rows, 2).hint).toBe('path = "/billing", recursive = true');
    expect(row(rows, 1).again).toEqual([]);
    expect(row(rows, 2).again.map((a) => a.action.kind)).toEqual(["draft", "recheck"]);
  });
});

describe("your coding assistant's runs among the agent's", () => {
  it("are known as theirs while still running, from who started them", () => {
    const data = chatSession();
    const template = data.events.find((e) => "event" in e.body && e.body.event.kind === "run-started")!;
    const body = template.body as { event: Record<string, unknown> };
    data.events = [...data.events, { ...template, session_seq: 999, event_id: "started-99", run: 99, body: { event: { ...body.event, label: "assistant" } } } as typeof template];
    expect(logPane(frame(data, INITIAL_UI)).runningNote).toBe("Run 99, run by your assistant, is running. It appears here when it ends.");
  });

  it("aren't named as the agent's next program after one that didn't compile", () => {
    // Run 3 didn't compile; run 4 after it was the coding assistant's, and the agent made none after.
    const data = chatSession();
    data.runs[4] = { ...data.runs[4], source: "assistant" };
    const note = row(logPane(frame(data, INITIAL_UI)).rows, 3).againNote;
    expect(note).toBe("This program didn’t compile, so running it again gives the same error.");
  });
});
