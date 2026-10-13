// Small wordings the whole page shares: outcomes, sizes, times, the agent's name.
import type { ChatSettings, Outcome } from "../api/types";

/** A failed run's error kind, as the log words it: a program that didn't compile was rejected. */
const ENDS: Record<string, string> = { compile_error: "rejected", runtime_error: "failed", permission_denied: "denied" };

/** How a run ended, in one word: completed, denied, failed, rejected, stopped or cancelled. */
export function end(outcome: Outcome): string {
  if (outcome.kind === "failed") return (outcome.error && ENDS[outcome.error]) || "failed";
  return outcome.kind;
}

/** The status colour for an outcome word. */
export type Tone = "ok" | "warn" | "bad";

export function tone(word: string): Tone {
  return word === "completed" ? "ok" : word === "stopped" ? "warn" : "bad";
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

/** A session's settings in a few words, as `sessions` prints them. */
export function settingsLabel(set: ChatSettings | undefined | null): string {
  if (!set) return "";
  const parts: string[] = [];
  if (set.effort) parts.push("effort " + set.effort);
  if (set.max_output) parts.push("max output " + set.max_output);
  if (set.provider && Object.keys(set.provider).length) parts.push("provider settings " + Object.keys(set.provider).join(" "));
  return parts.join(", ");
}

/** What the notice after a copy says. A failed copy stays up, so the text can be copied by hand. */
export function copyNotice(text: string, what: "reference" | "command", ok: boolean): { head: string; body: string; auto: boolean } {
  const name = what === "reference" ? "line for your assistant" : what;
  return { head: ok ? "Copied " + name + ":" : "Copy this " + name + ":", body: text, auto: ok };
}
