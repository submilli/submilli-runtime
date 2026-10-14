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
    expect(pane.sub).toBe("program 1 · run 1");
    expect(active(pane)).toBe(4);
    expect(pane.isPackage).toBe(false);
    expect(pane.stepNote).toBe("Line 4 calls into @acme/billing (listCharges). Step → to follow it into the package’s code.");
  });

  it("then the package's own line, asking on the program's behalf", () => {
    const pane = at(data, 4);
    expect(pane.sub).toBe("@acme/billing · lib · called from program 1, line 4");
    expect(pane.isPackage).toBe(true);
    expect(active(pane)).toBe(28);
    expect(pane.lines[27].text).toBe('    check("acme.com/charges.list", { customerId });');
    expect(pane.stepNote).toBe(
      "Inside @acme/billing, listCharges(), line 28 asks the blueprint whether the program may use acme.com/charges.list. Allowed (check 1.1).",
    );
    expect(pane.codeNote).toBe("The package’s source as installed now, which may differ from the build this run used.");
  });

  it("then the package making its own call", () => {
    const pane = at(data, 5);
    expect(active(pane)).toBe(31);
    expect(pane.stepNote).toBe(
      "Inside @acme/billing, listCharges(), line 31 makes the package’s own call, fs.read, under the blueprint’s rules for @acme/billing. Allowed (check 1.2), 21.5 kB fetched.",
    );
  });

  it("names a refusal inside the package", () => {
    const pane = at(data, 9);
    expect(pane.stepNote).toBe(
      "Inside @acme/billing, listCharges(), line 28 asks the blueprint whether the program may use acme.com/charges.list. Denied (check 2.1).",
    );
  });

  it("at a run's end shows its program with no step note", () => {
    const pane = at(data, 6);
    expect(pane.sub).toBe("program 1 · run 1");
    expect(active(pane)).toBeNull();
    expect(pane.stepNote).toBe("");
  });

  it("before any program says how to reach one", () => {
    const pane = at(data, 2);
    expect(pane.lines).toEqual([]);
    expect(pane.empty).toBe("No program yet at this point. Step forward (→) to reach the agent’s first program.");
  });

  it("shows a run picked while stepping, until the next step", () => {
    expect(at(data, 7, { sel: "r1" }).sub).toBe("program 1 · run 1");
    expect(at(data, 8).sub).toBe("program 2 · run 2");
  });

  it("says when the package source isn't available", () => {
    const missing = chatSession();
    missing.sources = {};
    const pane = at(missing, 4);
    expect(pane.lines).toEqual([]);
    expect(pane.empty).toBe("@acme/billing · lib: its source isn’t available.");
    expect(pane.stepNote).toContain("Inside @acme/billing, listCharges(), line 28");
  });

  it("a plain call is one step on the program's line", () => {
    const plain = plainSession();
    const pane = at(plain, 4);
    expect(pane.sub).toBe("program 1 · run 10");
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
    expect(codePane(frame(data, INITIAL_UI)).sub).toBe("program 4 · run 4");
    const picked = codePane(frame(data, { ...INITIAL_UI, sel: "f5" }));
    expect(picked.sub).toBe("program 2 · run 2");
    expect(picked.codeNote).toBe("Run 5 ran this same program, from run 2.");
    expect(picked.programRef).toBe(
      "Submilli playground, the program of run 2 (details - project: `billing-demo`, see_with: `submilli playground show 2 --program`)",
    );
  });

  it("marks a picked decision's program line", () => {
    const pane = codePane(frame(chatSession(), { ...INITIAL_UI, sel: "d2.1", program: 2 }));
    expect(pane.sub).toBe("program 2 · run 2");
    expect(active(pane)).toBe(4);
  });
});
