// References: one line to paste to a coding assistant: what it is, its details, and the
// command that shows more. Run and decision numbers are per project, so each line names
// the project.
import type { Show, ShowDecision, Status } from "../api/types";
import { end } from "./format";

/** The project and blueprint every reference names. */
export interface Names {
  /** The project's directory name. */
  project: string;
  blueprint: string;
}

export function namesOf(status: Status | null): Names {
  if (!status) return { project: "", blueprint: "" };
  const parts = status.project.split(/[\\/]/).filter(Boolean);
  return { project: parts[parts.length - 1] || status.project, blueprint: status.blueprint.name };
}

/** One line: what it is, then its details as `key: \`value\`` pairs, the command to see more last. */
function line(what: string, details: [string, string][]): string {
  return "Submilli playground, " + what + " (details - " + details.map(([k, v]) => k + ": `" + v + "`").join(", ") + ")";
}

/** How a run ended, as you'd tell someone. */
function ended(r: Show): string {
  const word = end(r.outcome);
  if (word === "denied") return r.outcome.kind === "denied" && r.outcome.decision ? "denied at check " + r.outcome.decision : "denied";
  if (word === "rejected") return "rejected: its program didn’t compile";
  if (word === "failed") return "failed with an error";
  if (word === "stopped") return "stopped at a call with no recorded reply";
  return word;
}

export function runRef(names: Names, r: Show): string {
  return line("run " + r.run, [
    ["project", names.project],
    ["blueprint", names.blueprint + " v" + r.blueprint_version],
    ["outcome", ended(r)],
    ["see_with", "submilli playground show " + r.run],
  ]);
}

export function decisionRef(names: Names, r: Show, d: ShowDecision): string {
  const ref = d.refs[0];
  return line("check " + ref, [
    ["project", names.project],
    ["blueprint", names.blueprint + " v" + r.blueprint_version],
    ["outcome", d.outcome === "deny" ? "denied" : "allowed"],
    ["call", d.caller + " → " + d.capability],
    ["see_with", "submilli playground explain " + ref],
  ]);
}

export function programRef(names: Names, run: number): string {
  return line("the program of run " + run, [
    ["project", names.project],
    ["see_with", "submilli playground show " + run + " --program"],
  ]);
}

export function sessionRef(names: Names, id: string): string {
  return line("session " + id.slice(0, 8), [
    ["project", names.project],
    ["see_with", "submilli playground runs --session " + id],
  ]);
}

export function ruleRef(names: Names, version: number, lineNo: number, label: string): string {
  return line("blueprint line " + lineNo, [
    ["project", names.project],
    ["blueprint", names.blueprint + " v" + version],
    ...(label ? [["rule", label.replace(/^rule /, "")] as [string, string]] : []),
    ["see_with", "submilli playground changes --version " + version],
  ]);
}

/** What a copy-for-assistant button's hover text says it copies. */
export function forAsst(text: string): string {
  return text ? "Copies a line to paste into your coding assistant: " + text : text;
}
