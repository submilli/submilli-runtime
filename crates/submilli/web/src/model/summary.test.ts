import { describe, expect, it } from "vitest";
import { summary } from "./summary";
import { frame, readSession } from "./steps";
import { chatSession, plainSession } from "./testdata";
import { INITIAL_UI } from "./ui";

describe("the session summary", () => {
  it("at the latest event counts every reached call, and only completed runs as returned", () => {
    expect(summary(frame(chatSession(), INITIAL_UI))).toEqual({
      fetched: "43.1 kB",
      returned: "167 B",
      dataNote:
        "Fetched data stayed in code; the agent got only each finished run’s result. Not counted: 1 refused run and 1 failed run, where the agent got the error message instead.",
      execHead: "4 runs",
      execSub: "2 completed · 1 denied · 1 rejected",
      checksHead: "4 allowed",
      checksSub: "1 denied",
      checksBad: true,
      followHead: "3 runs",
      followSub: "tests and live reruns",
    });
  });

  it("counts a call's bytes as soon as its step is reached", () => {
    const data = chatSession();
    const { steps } = readSession(data.events);
    const at = (k: number) => summary(frame(data, { ...INITIAL_UI, cursor: steps[k - 1].key }));
    expect(at(4).fetched).toBe("0 B");
    expect(at(5).fetched).toBe("21.5 kB");
    // Not returned until the run completes.
    expect(at(5).returned).toBe("0 B");
    expect(at(6).returned).toBe("64 B");
    expect(at(2).dataNote).toBe("Nothing fetched or returned yet.");
    // Stepped back: follow-ups aren't counted.
    expect(at(10).followHead).toBe("None");
    expect(at(10).followSub).toBe("None yet");
  });

  it("says a refusal came before any data", () => {
    const data = chatSession();
    // Only the refused run.
    data.events = data.events.filter((e) => e.run === 2 || e.run === undefined);
    const s = summary(frame(data, INITIAL_UI));
    expect(s.fetched).toBe("0 B");
    expect(s.dataNote).toBe(
      "The call was denied before any data was fetched; the agent got the denial. Not counted: 1 refused run, where the agent got the error message instead.",
    );
  });

  it("counts a call with no line at its run's end", () => {
    const data = plainSession();
    const { steps } = readSession(data.events);
    expect(summary(frame(data, { ...INITIAL_UI, cursor: steps[3].key })).fetched).toBe("1.2 kB");
    expect(summary(frame(data, { ...INITIAL_UI, cursor: steps[4].key })).fetched).toBe("1.7 kB");
  });

  it("before a session starts shows dashes", () => {
    expect(summary(frame(chatSession(), { ...INITIAL_UI, mode: "setup" }))).toMatchObject({
      fetched: "—",
      returned: "—",
      dataNote: "Start a session to record runs.",
      execHead: "—",
      execSub: "",
    });
  });
});
