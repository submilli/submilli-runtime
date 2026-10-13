// References: one line a coding assistant can use with no other context. Run and decision
// numbers are per project, so each line names the project.
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

export function runRef(names: Names, r: Show): string {
  return (
    "Submilli playground run " + r.run + " in project " + names.project + " (" + r.source + ", " + end(r.outcome) +
    ", blueprint " + names.blueprint + " v" + r.blueprint_version + "). Details: submilli playground show " + r.run
  );
}

export function decisionRef(names: Names, r: Show, d: ShowDecision): string {
  const ref = d.refs[0];
  return (
    "Submilli playground decision " + ref + " in project " + names.project + " (" + (d.outcome === "deny" ? "denied" : "allowed") +
    " " + d.caller + " → " + d.capability + ", blueprint " + names.blueprint + " v" + r.blueprint_version +
    "). Details: submilli playground explain " + ref
  );
}

export function programRef(names: Names, run: number): string {
  return "Submilli playground program of run " + run + " in project " + names.project + ". Print it: submilli playground show " + run + " --program";
}

export function sessionRef(names: Names, id: string): string {
  return "Submilli playground session " + id + " in project " + names.project + ". Its runs: submilli playground runs --session " + id;
}

export function ruleRef(names: Names, version: number, line: number, label: string): string {
  return (
    "Blueprint " + names.blueprint + " v" + version + ", line " + line + (label ? " (" + label + ")" : "") +
    ", in Submilli playground project " + names.project + ". Print that version: submilli playground changes --version " + version
  );
}

/** What a copy-for-assistant button's hover text says it copies. */
export function forAsst(text: string): string {
  return text ? "Copies a line to paste into your coding assistant: " + text : text;
}
