// The blueprint dialog: a version's text with a rule's block lit, and a package's own rules.
import { plainBy, when } from "./format";
import { ruleRef } from "./refs";
import type { Frame } from "./steps";

/** The line (from 1) where a rule of that name starts, or `null`. */
export function bpRuleLine(text: string, name: string): number | null {
  const lines = text.split("\n");
  const i = lines.findIndex((l) => /^\s*-\s+name:\s*/.test(l) && l.replace(/^\s*-\s+name:\s*/, "").replace(/^(['"])(.*)\1$/, "$2").trim() === name);
  return i < 0 ? null : i + 1;
}

const indentOf = (line: string) => line.length - line.trimStart().length;
const skippable = (line: string) => !line.trim() || line.trim().startsWith("#");

/**
 * A package's block under `permissions:`: the line its name is on, and how many rules it
 * lists. `null` when the version has no block for it.
 */
export function packageRules(text: string, name: string): { line: number; count: number } | null {
  const lines = text.split("\n");
  const start = lines.findIndex((l) => /^permissions:\s*$/.test(l));
  if (start < 0) return null;
  // Callers are the keys at the block's first indent; anything deeper belongs to a rule.
  let callerIndent = -1;
  for (let i = start + 1; i < lines.length; i++) {
    const l = lines[i];
    if (skippable(l)) continue;
    const keyIndent = indentOf(l);
    if (keyIndent === 0) break;
    if (callerIndent < 0) callerIndent = keyIndent;
    if (keyIndent !== callerIndent) continue;
    const key = /^(['"]?)(.+)\1:\s*$/.exec(l.trim());
    if (!key || key[2] !== name) continue;
    // Its rules are the list items under it, at whatever indent the list uses.
    let itemIndent = -1;
    let count = 0;
    for (let j = i + 1; j < lines.length; j++) {
      const m = lines[j];
      if (skippable(m)) continue;
      const ind = indentOf(m);
      const item = m.trimStart().startsWith("- ") || m.trim() === "-";
      if (itemIndent < 0) {
        if (!item || ind < keyIndent) break;
        itemIndent = ind;
      }
      if (ind < itemIndent || (ind === itemIndent && !item)) break;
      if (ind === itemIndent) count++;
    }
    return { line: i + 1, count };
  }
  return null;
}

/** How a version changed access, as the CLI words it. */
const CLASSIFIED: Record<string, string> = {
  widening: "widens access",
  narrowing: "narrows access",
  mixed: "widens and narrows access",
  unknown: "can’t tell whether this widens or narrows access",
};

export interface BlueprintLine {
  num: number;
  text: string;
  active: boolean;
}

export interface BlueprintDialog {
  version: number;
  title: string;
  summary: string;
  versions: { version: number; name: string; on: boolean }[];
  /** The lit rule: what it is, and its copy-for-assistant line. */
  focus: { label: string; copyText: string } | null;
  /** Empty until the version's text is loaded. */
  lines: BlueprintLine[];
  /** The CLI command that prints this version. */
  cli: string;
}

export function blueprintDialog(f: Frame): BlueprintDialog {
  // Oldest first, as the design lists them.
  const known = Object.keys(f.data.versions)
    .map(Number)
    .sort((a, b) => a - b);
  const inForce = f.inForce ?? known[known.length - 1] ?? 0;
  const v = f.ui.bpV ?? inForce;
  const ver = f.data.versions[v];
  const focus = f.ui.bpFocus;
  const text = ver?.text;
  const lines = text ? text.split("\n") : [];
  // A rule's block runs on while lines are indented past where the rule's own text starts.
  let last = 0;
  if (focus && lines[focus.line - 1] !== undefined) {
    const head = lines[focus.line - 1];
    const inner = head.length - head.replace(/^[\s-]*/, "").length;
    last = focus.line;
    while (last < lines.length && lines[last].trim() && indentOf(lines[last]) >= inner) last++;
  }
  return {
    version: v,
    title: "Blueprint " + f.names.blueprint + " · v" + v,
    summary: ver ? "v" + v + (v === inForce ? " (in force)" : "") + ", " + when(ver.at) + ", " + (CLASSIFIED[ver.classification] ?? ver.classification) + ". " + plainBy(ver.summary) : "",
    versions: known.map((k) => ({ version: k, name: "v" + k + (k === inForce ? " · in force" : ""), on: k === v })),
    focus: focus ? { label: "Highlighted: line " + focus.line + ", " + focus.label, copyText: ruleRef(f.names, v, focus.line, focus.label) } : null,
    lines: lines.map((t, i) => ({ num: i + 1, text: t, active: !!focus && i + 1 >= focus.line && i + 1 <= last })),
    cli: "submilli playground changes --version " + v,
  };
}
