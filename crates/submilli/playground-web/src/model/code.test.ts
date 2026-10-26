import { describe, expect, it } from "vitest";
import { codePane } from "./code";
import { frame, readSession } from "./steps";
import { chatSession, plainSession } from "./testdata";
import { INITIAL_UI, type UiState } from "./ui";
import type { PageData } from "../api/types";

function at(data: PageData, stepNumber: number, over: Partial<UiState> = {}) {
  const { steps } = readSession(data.events);
  const ui = { ...INITIAL_UI, cursor: steps[stepNumber - 1].key, ...over };
  return codePane(frame(data, ui));
}

function active(pane: ReturnType<typeof codePane>): number | null {
  const line = pane.lines.find((l) => l.active);
  return line ? line.num : null;
}

describe("the code pane while stepping", () => {
  const data = chatSession();

  it("shows the program's line that calls into the package first", () => {
    const pane = at(data, 3);
    expect(pane.sub).toBe("Program 1 · Run 1");
    expect(active(pane)).toBe(4);
    expect(pane.isPackage).toBe(false);
    expect(pane.stepNote).toBe("Line 4 calls into @pied-piper/billing (listCharges). Step → to follow it into the package’s code.");
  });

  it("then the package's own line, asking on the program's behalf", () => {
    const pane = at(data, 4);
    expect(pane.sub).toBe("@pied-piper/billing · lib · called from program 1, line 4");
    expect(pane.isPackage).toBe(true);
    expect(active(pane)).toBe(28);
    expect(pane.lines[27].text).toBe('    check("pied-piper.example/charges.list", { customerId });');
    expect(pane.stepNote).toBe(
      "Inside @pied-piper/billing, listCharges(), line 28 asks the blueprint whether the program may use pied-piper.example/charges.list. Allowed (check 1.1).",
    );
    expect(pane.codeNote).toBe("The package’s source as installed now, which may differ from the build this run used.");
  });

  it("then the package making its own call", () => {
    const pane = at(data, 5);
    expect(active(pane)).toBe(31);
    expect(pane.stepNote).toBe(
      "Inside @pied-piper/billing, listCharges(), line 31 makes the package’s own call, fs.read, under the blueprint’s rules for @pied-piper/billing. Allowed (check 1.2), 21.5 kB fetched.",
    );
  });

  it("names a refusal inside the package", () => {
    const pane = at(data, 9);
    expect(pane.stepNote).toBe(
      "Inside @pied-piper/billing, listCharges(), line 28 asks the blueprint whether the program may use pied-piper.example/charges.list. Denied (check 2.1).",
    );
  });

  it("at a run's end shows its program with no step note", () => {
    const pane = at(data, 6);
    expect(pane.sub).toBe("Program 1 · Run 1");
    expect(active(pane)).toBeNull();
    expect(pane.stepNote).toBe("");
  });

  it("before any program says how to reach one", () => {
    const pane = at(data, 2);
    expect(pane.lines).toEqual([]);
    expect(pane.empty).toBe("No program yet at this point. Step forward (→) to reach the agent’s first program.");
  });

  it("shows a run picked while stepping, until the next step", () => {
    expect(at(data, 7, { sel: "r1" }).sub).toBe("Program 1 · Run 1");
    expect(at(data, 8).sub).toBe("Program 2 · Run 2");
  });

  it("says when the package source isn't available", () => {
    const missing = chatSession();
    missing.sources = {};
    const pane = at(missing, 4);
    expect(pane.lines).toEqual([]);
    expect(pane.empty).toBe("@pied-piper/billing · lib: its source isn’t available.");
    expect(pane.stepNote).toContain("Inside @pied-piper/billing, listCharges(), line 28");
  });

  it("a plain call is one step on the program's line", () => {
    const plain = plainSession();
    const pane = at(plain, 4);
    expect(pane.sub).toBe("Program 1 · Run 10");
    expect(active(pane)).toBe(3);
    expect(pane.stepNote).toBe("Line 3 made a call: main → http.get, allowed by the blueprint (check 10.1), 1.2 kB fetched.");
  });

  it("leaves out what wasn't recorded", () => {
    const plain = plainSession();
    // No decision for the call, and no bytes back.
    plain.events = plain.events.filter((e) => e.decision !== 1);
    for (const e of plain.events) {
      const ev = (e.body as { event?: Record<string, unknown> }).event;
      if (ev && ev.kind === "call-finished" && ev.call_index === 0) delete ev.result_bytes;
    }
    expect(at(plain, 4).stepNote).toBe("Line 3 made a call: main → http.get.");
  });
});

describe("the code pane at the latest event", () => {
  it("shows the last program, and the program a picked follow-up ran", () => {
    const data = chatSession();
    expect(codePane(frame(data, INITIAL_UI)).sub).toBe("Program 4 · Run 4");
    const picked = codePane(frame(data, { ...INITIAL_UI, sel: "f5" }));
    expect(picked.sub).toBe("Source: Program 2 · Run 2");
    expect(picked.codeNote).toBe("Used by Run 5. This is the original program from Run 2.");
    expect(picked.programRef).toBe(
      "Submilli playground, the program of run 2 (details - project: `billing-demo`, see_with: `submilli playground show 2 --program`)",
    );
  });

  it("shows a run still going as soon as its program is read, ahead of the finished ones", () => {
    const data = chatSession();
    const last = data.events.at(-1)!;
    const at = (last.position?.at_micros ?? 0) + 1;
    data.events = [
      ...data.events,
      {
        format: 1,
        session_seq: last.session_seq + 1,
        event_id: "started-9",
        position: { at_micros: at, rank: 0 },
        run: 9,
        body: { event: { schema: 1, kind: "run-started", at_micros: at, label: "browser-chat", entry: "mcp" } },
      } as unknown as (typeof data.events)[number],
    ];
    // Before its program is read, the last finished program still shows.
    expect(codePane(frame(data, INITIAL_UI)).sub).toBe("Program 4 · Run 4");
    data.running = { 9: "function main(): number {\n  return 1;\n}" };
    const pane = codePane(frame(data, INITIAL_UI));
    expect(pane.sub).toBe("Program 5 · Run 9 · Running");
    expect(pane.lines.length).toBe(3);
    expect(pane.codeNote).toBe("Still running: its result shows in the log when it ends.");
  });

  it("marks a picked decision's program line", () => {
    const pane = codePane(frame(chatSession(), { ...INITIAL_UI, sel: "d2.1", program: 2 }));
    expect(pane.sub).toBe("Program 2 · Run 2");
    expect(active(pane)).toBe(4);
  });
});


it("does not substitute the newest program when a selected historical source is missing", () => {
  const data = chatSession();
  delete data.runs[2];
  const pane = codePane(frame(data, { ...INITIAL_UI, sel: "r2" }));
  expect(pane.sub).toBe("run 2");
  expect(pane.lines).toEqual([]);
  expect(pane.empty).toBe("Source was not recorded for this run.");
});
