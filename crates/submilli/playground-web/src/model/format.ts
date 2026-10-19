// Small wordings the whole page shares: outcomes, sizes, times, the agent's name.
import type { ChatSettings, Outcome, Show } from "../api/types";

/** The run a test or rerun ran again, or null for a run of its own. */
export function sourceRunOf(show: Show): number | null {
  return show.test?.source_run ?? show.rerun_of ?? null;
}

/** A failed run's error kind, as the log words it: a program that didn't compile was rejected. */
const ENDS: Record<string, string> = { compile_error: "rejected", runtime_error: "failed", permission_denied: "denied" };

/** How a run ended, in one word: completed, denied, failed, rejected, stopped or cancelled. */
export function end(outcome: Outcome): string {
  if (outcome.kind === "failed") return (outcome.error && ENDS[outcome.error]) || "failed";
  return outcome.kind;
}

/** The status colour for an outcome word: a run you cancelled is neither a success nor a failure. */
export type Tone = "ok" | "warn" | "bad" | "quiet";

export function tone(word: string): Tone {
  return word === "completed" ? "ok" : word === "stopped" ? "warn" : word === "cancelled" ? "quiet" : "bad";
}

/** How long a run took: milliseconds under a second, then seconds with one decimal. */
export function duration(ms: number): string {
  if (ms < 1000) return ms + " ms";
  return (ms / 1000).toLocaleString("en-US", { minimumFractionDigits: 1, maximumFractionDigits: 1 }) + " s";
}

export function capitalize(text: string): string {
  return text.charAt(0).toUpperCase() + text.slice(1);
}

/** Bytes as recorded, in B under a thousand and kB above, with one decimal at most. */
export function size(n: number): string {
  if (n < 1000) return n + " B";
  return (n / 1000).toLocaleString("en-US", { maximumFractionDigits: 1 }) + " kB";
}

/** A time in UTC, as "Oct 13, 13:26 UTC". */
export function when(iso: string): string {
  const d = new Date(iso);
  return (
    d.toLocaleString("en-US", {
      month: "short",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit",
      hourCycle: "h23",
      timeZone: "UTC",
    }) + " UTC"
  );
}

export function ordinal(n: number): string {
  const t = n % 100;
  const suffix = t > 10 && t < 14 ? "th" : ["th", "st", "nd", "rd"][n % 10] || "th";
  return n + suffix;
}

/** The CLI marks names with backticks; the page shows them plain. */
export function plainBy(text: string | undefined): string {
  return (text || "").replace(/`/g, "");
}

/** A decision's context, as `name = value` pairs. */
export function ctx(context: Record<string, unknown> | undefined): string {
  return context
    ? Object.keys(context)
        .map((k) => k + " = " + JSON.stringify(context[k]))
        .join(", ")
    : "";
}

/** A value that differs between two sets of values: what it `was`, and is `now`. */
export interface ValueChange {
  name: string;
  was: string;
  now: string;
}

/**
 * Which values differ, said as `lead`, the first change, its "+N more" that opens the
 * rest, then `tail`. One line however many values a project binds.
 */
export interface ValuesNote {
  lead: string;
  changes: ValueChange[];
  tail: string;
}

/** The values that differ between `before` and `after`, a name unset on one side shown as "not set". */
export function differing(before: Record<string, string>, after: Record<string, string>): ValueChange[] {
  const names = [...new Set([...Object.keys(before), ...Object.keys(after)])];
  return names.filter((name) => before[name] !== after[name]).map((name) => ({ name, was: before[name] ?? "not set", now: after[name] ?? "not set" }));
}

// Fields whose value reads on its own; any other is named, as `customerId = cus_globex`.
const SPEAKS_FOR_ITSELF = new Set(["path", "query", "package", "module", "name"]);

/**
 * The main thing a call asked for, to read at a glance: its first field's value, named
 * unless it reads on its own. "" when it asked for nothing. Every field is in
 * [`ctx`]'s words, for hover text and the opened check. The fields come from the agent
 * or the program, and show only as text.
 */
export function askedFor(fields: Record<string, unknown> | undefined): string {
  const key = fields ? Object.keys(fields)[0] : undefined;
  if (!fields || key === undefined) return "";
  const value = fields[key];
  const text = typeof value === "string" ? value : JSON.stringify(value) ?? String(value);
  const shown = text.length > 60 ? text.slice(0, 59) + "…" : text;
  return SPEAKS_FOR_ITSELF.has(key) ? shown : key + " = " + shown;
}

export function short(id: string): string {
  return id.slice(0, 8);
}

export function plural(n: number, one: string, many: string): string {
  return n + " " + (n === 1 ? one : many);
}

const PROVIDERS: Record<string, string> = { anthropic: "Claude", openai: "OpenAI", google: "Gemini" };

/** A provider's name as people say it. */
export function providerName(provider: string): string {
  return PROVIDERS[provider] || provider;
}

/** `provider:model` as people say it: "Claude · claude-sonnet-5-5". */
export function agentLabel(named: string | undefined | null): string {
  if (!named) return "";
  const i = named.indexOf(":");
  return providerName(named.slice(0, i)) + " · " + named.slice(i + 1);
}

/** Each provider setting of a session, by name, with its value as JSON. */
export function providerSettings(set: ChatSettings | undefined | null): { name: string; value: string }[] {
  return Object.entries(set?.provider ?? {}).map(([name, value]) => ({ name, value: JSON.stringify(value) ?? "" }));
}

/** A session's settings in a few words: the first provider setting with its value, and how many more. */
export function settingsLabel(set: ChatSettings | undefined | null): string {
  if (!set) return "";
  const parts: string[] = [];
  if (set.effort) parts.push("effort " + set.effort);
  if (set.max_output) parts.push("max output " + set.max_output);
  const names = set.provider ? Object.keys(set.provider) : [];
  if (set.provider && names.length) {
    const value = JSON.stringify(set.provider[names[0]]) ?? "";
    const shown = value.length > 24 ? value.slice(0, 23) + "…" : value;
    parts.push("provider settings " + names[0] + "=" + shown + (names.length > 1 ? " (+" + (names.length - 1) + " more)" : ""));
  }
  return parts.join(", ");
}

/** What the notice after a copy says. A failed copy stays up, so the text can be copied by hand. */
export function copyNotice(text: string, what: "reference" | "command", ok: boolean): { head: string; body: string; auto: boolean } {
  const name = what === "reference" ? "line for your assistant" : what;
  return { head: ok ? "Copied " + name + ":" : "Copy this " + name + ":", body: text, auto: ok };
}

/** One variable and how many more: the variables a run used, or a rerun would use. */
export function boundVars(vars: Record<string, string>): string {
  const names = Object.keys(vars);
  if (!names.length) return "no variables";
  return names[0] + "=" + vars[names[0]] + (names.length > 1 ? " (+" + (names.length - 1) + " more)" : "");
}
