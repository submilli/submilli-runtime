import { describe, expect, it } from "vitest";
import { blueprintDialog, bpRuleLine, packageRules } from "./blueprint";
import { frame } from "./steps";
import { chatSession } from "./testdata";
import { INITIAL_UI } from "./ui";

const data = chatSession();
const v2 = data.versions[2].text as string;

describe("the blueprint dialog", () => {
  it("finds a rule's line by name", () => {
    expect(bpRuleLine(v2, "globex-support")).toBe(35);
    expect(bpRuleLine(v2, "nope")).toBeNull();
  });

  it("opens on the version in force with its summary", () => {
    const d = blueprintDialog(frame(data, INITIAL_UI));
    expect(d.version).toBe(2);
    expect(d.title).toBe("Blueprint billing · v2");
    expect(d.summary).toBe("v2 (in force), Oct 13, 13:26 UTC, widens access. Widened: main gained an allow rule globex-support for acme.com/charges.list.");
    expect(d.versions).toEqual([
      { version: 1, name: "v1", on: false },
      { version: 2, name: "v2 · in force", on: true },
    ]);
    expect(d.focus).toBeNull();
    expect(d.lines.length).toBe(52);
    expect(d.lines.some((l) => l.active)).toBe(false);
    expect(d.cli).toBe("submilli playground changes --version 2");
  });

  it("highlights a focused rule's block and copies its line", () => {
    const d = blueprintDialog(frame(data, { ...INITIAL_UI, bpV: 1, bpFocus: { line: 27, label: "nearest rule charges-for-signed-in-customer" } }));
    expect(d.lines.filter((l) => l.active).map((l) => l.num)).toEqual([27, 28, 29, 30]);
    expect(d.focus).toEqual({
      label: "Highlighted: line 27, nearest rule charges-for-signed-in-customer",
      copyText:
        "Blueprint billing v1, line 27 (nearest rule charges-for-signed-in-customer), in Submilli playground project billing-demo. Print that version: submilli playground changes --version 1",
    });
  });

  it("has no lines for a version whose text isn't loaded yet", () => {
    const d = blueprintDialog(frame({ ...data, versions: { 2: { ...data.versions[2], text: undefined } } }, INITIAL_UI));
    expect(d.lines).toEqual([]);
  });
});

describe("a package's own rules", () => {
  it("counts the rules in the package's block under permissions", () => {
    expect(packageRules(v2, "@acme/billing")).toEqual({ line: 45, count: 2 });
  });

  it("works for any package name and any list indent", () => {
    const text = "permissions:\n  main:\n    - name: a\n      action: allow\n  other-pkg:\n    - capability: fs.read\n      action: allow\n    # a comment\n    - capability: http.get\n      action: allow\nvfs: {}\n";
    expect(packageRules(text, "other-pkg")).toEqual({ line: 5, count: 2 });
    expect(packageRules(text, "@acme/billing")).toBeNull();
  });
});
