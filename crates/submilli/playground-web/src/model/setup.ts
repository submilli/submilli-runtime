// The New session form: its variables, the agent and its settings, checked as the
// playground would check them before the first message goes.
import type { ChatSettings, ClaudeCodeStatus, DeclaredVariable, SavedKey, SessionSummary, Status } from "../api/types";
import { namesOf } from "./refs";
import { dollars, providerName } from "./format";

export interface SetupForm {
  blueprint?: string;
  provider: string;
  model: string;
  /** "" for the provider's default. */
  effort: string;
  max: string;
  /** Claude Code's spending limit for each message, in dollars; "" for the default. */
  budget: string;
  /** Provider settings, as JSON text. */
  providerJson: string;
  /** Variables are being edited as one JSON object rather than as fields. */
  varJson: boolean;
  varJsonText: string;
  fields: Record<string, string>;
}

export const INITIAL_SETUP: SetupForm = { blueprint: "", provider: "", model: "", effort: "", max: "", budget: "", providerJson: "", varJson: false, varJsonText: "", fields: {} };

/** What the playground sets itself in each provider's request, so provider settings can't. */
export const OWN_FIELDS: Record<string, string[]> = {
  anthropic: ["model", "messages", "system", "tools", "max_tokens", "stream", "thinking"],
  openai: ["model", "input", "instructions", "tools", "max_output_tokens", "stream"],
};

/** The developer's own Claude Code, which needs no key: it runs on their login. */
export const CLAUDE_CODE = "claude-code";

/** The models Claude Code is asked for, by full name so the session says which one answered, the default first. */
export const CLAUDE_CODE_MODELS: [string, string][] = [
  ["claude-sonnet-5-5", "Sonnet 5.5"],
  ["claude-opus-5-5", "Opus 5.5"],
  ["claude-fable-5-1", "Fable 5.1"],
  ["claude-haiku-5-5", "Haiku 5.5"],
];

/** Whether `model` is a full model name Claude Code takes, as the playground checks it: a version, which its short names lack. */
export function isFullModelName(model: string): boolean {
  return /^[A-Za-z0-9][A-Za-z0-9._-]*$/.test(model) && /[0-9]/.test(model);
}

/** Claude Code's spending limit for each message unless one is set, and the most it may be, in dollars. */
export const DEFAULT_BUDGET = 1;
export const MAX_BUDGET = 100;

/** A form's selected blueprint, leaving the shared server status unchanged. */
export function selectedSetupStatus(status: Status, name?: string): Status {
  const selected = status.blueprints?.find((blueprint) => blueprint.name === name);
  if (!selected) return status;
  return { ...status, blueprint: { name: selected.name, path: selected.path }, packages: selected.packages, packages_error: selected.packages_error ?? undefined, declared_variables: selected.declared_variables, blueprint_status: selected.blueprint_status };
}

/** The variables the blueprint in force declares, the required ones first. */
export function declaredVariables(status: Status): DeclaredVariable[] {
  return [...status.declared_variables].sort((a, b) => Number(b.required) - Number(a.required));
}

/** The most a session's answer may take, as the playground accepts it. */
export const MAX_OUTPUT = 1_000_000;

/** "billing v2": the blueprint in force, as the form names it. */
function blueprintLabel(status: Status): string {
  const v = status.blueprint_status.version;
  return namesOf(status).blueprint + (v !== null ? " v" + v : "");
}

function split(named: string): [string, string] {
  const i = named.indexOf(":");
  return [named.slice(0, i), named.slice(i + 1)];
}

/** The form as it opens: the default model, and the variables the playground is bound with. */
export function initialSetup(status: Status, sessions: SessionSummary[] = []): SetupForm {
  const fields: Record<string, string> = {};
  for (const d of declaredVariables(status)) fields[d.name] = status.binding.variables[d.name] || "";
  // The agent and its settings carry on from the latest session an agent chatted in.
  const last = latestAgent(sessions);
  if (last?.model) {
    const [provider, model] = split(last.model);
    const s = last.settings ?? {};
    return {
      ...INITIAL_SETUP,
      blueprint: status.blueprint.name,
      provider,
      model,
      effort: takesEffort(provider) ? s.effort ?? "" : "",
      max: s.max_output !== undefined ? String(s.max_output) : "",
      budget: s.max_budget_usd !== undefined ? s.max_budget_usd.toFixed(2) : "",
      providerJson: s.provider && Object.keys(s.provider).length ? JSON.stringify(s.provider) : "",
      fields,
    };
  }
  // Else the playground's default, else the first provider, whose key the form helps add.
  const first = providerOptions(status, "")[0];
  const [provider, model] = status.chat_models[0] ? split(status.chat_models[0]) : [first?.provider || "", first?.model || ""];
  return { ...INITIAL_SETUP, blueprint: status.blueprint.name, provider, model, fields };
}

/** The most recently started session an agent chatted in. */
function latestAgent(sessions: SessionSummary[]): SessionSummary | undefined {
  let latest: SessionSummary | undefined;
  for (const s of sessions) if (s.model && (!latest || s.started_at_micros > latest.started_at_micros)) latest = s;
  return latest;
}

/** The variables in a JSON object: one object of text values, each declared by the blueprint. */
export function parseVars(text: string, decls: DeclaredVariable[], blueprint: string): { vals?: Record<string, string>; err?: string } {
  let o: unknown;
  try {
    o = JSON.parse(text);
  } catch (e) {
    return { err: "Not valid JSON yet: " + (e as Error).message };
  }
  if (!o || typeof o !== "object" || Array.isArray(o)) return { err: "Use one object: { \"name\": \"value\", … }." };
  const names = decls.map((d) => d.name);
  const vals: Record<string, string> = {};
  for (const [k, v] of Object.entries(o as Record<string, unknown>)) {
    if (!names.includes(k)) return { err: k + " isn’t declared in blueprint " + blueprint + "." };
    if (v !== null && typeof v !== "string") return { err: k + " must be text. Blueprint variables are text for now." };
    // As the fields do: a value pasted with a space around it means the value.
    if (v && v.trim()) vals[k] = v.trim();
  }
  return { vals };
}

export interface SetupCheck {
  /** The variables the session starts with. */
  variables: Record<string, string>;
  /** `<provider>:<model>`. */
  model: string;
  settings: ChatSettings | null;
  /** What's wrong with the agent or its settings, or "". */
  problem: string;
  /** The variables' JSON doesn't parse, or "". */
  varError: string;
  /** The line under Start session. */
  hint: string;
  cantStart: boolean;
  /** Kept separate so application variables may share model field names. */
  modelInvalid: Record<string, string>;
  variableInvalid: Record<string, string>;
  missingVariables: string[];
}

export function checkSetup(status: Status, form: SetupForm): SetupCheck {
  const unavailable = !!form.blueprint && (status.blueprints ? !status.blueprints.some((b) => b.name === form.blueprint) : form.blueprint !== status.blueprint.name);
  status = selectedSetupStatus(status, form.blueprint);
  const decls = declaredVariables(status);
  const parsed = form.varJson ? parseVars(form.varJsonText, decls, blueprintLabel(status)) : { vals: trimmed(form.fields) };
  const variables = parsed.vals || {};
  const missing = decls.filter((d) => d.required && !(variables[d.name] || "").trim()).map((d) => d.name);

  const model = form.model.trim();
  const max = form.max.trim();
  const json = form.providerJson.trim();
  const set: ChatSettings = {};
  let problem = "";
  const variableInvalid: Record<string, string> = Object.fromEntries(missing.map((name) => [name, "Required."]));
  if (parsed.err) variableInvalid.json = parsed.err;
  const modelInvalid: Record<string, string> = {};
  const fail = (field: string, text: string) => {
    modelInvalid[field] = text;
    problem = problem || text;
  };
  const claudeCode = form.provider === CLAUDE_CODE;
  if (claudeCode) {
    // Claude Code runs on the developer's own login, so it needs no key, only to be ready.
    const cc = status.claude_code;
    if (cc?.state !== "available") problem = cc ? "Claude Code can’t answer: " + (cc.reason || "it isn’t ready") + "." : "Still checking for Claude Code. Try again in a moment.";
    else if (!isFullModelName(model)) fail("model", model ? "Give a full model name with its version, such as claude-opus-5-5: Claude Code’s short names move to newer models." : "Choose a model, or type a full model name.");
  }
  // Without a model key there's no agent to set up; the Agent field says how to add one.
  else if (form.provider && !hasKey(status, form.provider)) problem = providerName(form.provider) + " needs a key first: use Add key under Provider.";
  else if (!model) fail("model", "Give the model’s name.");
  else if (!/^(models\/)?[A-Za-z0-9._-]+$/.test(model)) fail("model", "A model name is letters, digits, “.”, “_”, and “-”.");
  if (form.effort) set.effort = form.effort;
  if (claudeCode) {
    // Always set, so the session shows the limit it runs under.
    const budget = form.budget.trim();
    const amount = budget ? Number(budget) : DEFAULT_BUDGET;
    if (budget && (!/^([0-9]+(\.[0-9]*)?|\.[0-9]+)$/.test(budget) || !(amount > 0))) fail("budget", "The spending limit is an amount in dollars, more than 0.");
    else if (amount > MAX_BUDGET) fail("budget", "The spending limit is at most " + dollars(MAX_BUDGET) + ".");
    else set.max_budget_usd = amount;
  } else if (max) {
    if (!/^[0-9]+$/.test(max) || Number(max) < 1) fail("max", "Max output is a whole number of tokens.");
    else if (Number(max) > MAX_OUTPUT) fail("max", "Max output is at most " + MAX_OUTPUT.toLocaleString("en-US") + " tokens.");
    else set.max_output = Number(max);
  }
  if (json && !claudeCode) {
    let given: unknown = null;
    try {
      given = JSON.parse(json);
    } catch (e) {
      fail("provider", "Provider settings: not JSON (" + (e as Error).message + ").");
    }
    if (given !== null && (typeof given !== "object" || Array.isArray(given))) {
      fail("provider", "Provider settings: give a JSON object, such as {\"temperature\": 0.2}.");
    } else if (given) {
      const obj = given as Record<string, unknown>;
      const own = (OWN_FIELDS[form.provider] || []).filter((k) => k in obj);
      const reasoning = obj.reasoning;
      if (own.length) {
        fail("provider", "Provider settings can’t set “" + own[0] + "”: the playground sets it itself" + (own[0] === "thinking" ? " (Claude’s thinking isn’t supported yet)" : "") + ".");
      } else if (form.effort && reasoning && typeof reasoning === "object" && "effort" in reasoning) {
        fail("provider", "Provider settings can’t set “reasoning.effort”: Effort sets it.");
      } else if (Object.keys(obj).length) {
        set.provider = obj;
      }
    }
  }
  if (unavailable) fail("blueprint", "Choose an available blueprint from this project.");
  if (status.packages_error) fail("blueprint", status.packages_error);
  const settings = Object.keys(set).length ? set : null;
  const more = missing.length > 1 ? " (+" + (missing.length - 1) + " more)" : "";
  const hint = parsed.err
    ? "Fix the variables’ JSON to start."
    : missing.length
      ? "Still needed: " + missing[0] + more + "."
      : problem || "The variables, the agent, and its settings are fixed once the session starts.";
  const cantStart = missing.length > 0 || !!parsed.err || !!problem;
  return {
    variables,
    modelInvalid,
    variableInvalid,
    missingVariables: missing,
    model: form.provider + ":" + model,
    settings,
    problem,
    varError: parsed.err || "",
    hint,
    cantStart,
  };
}

function trimmed(fields: Record<string, string>): Record<string, string> {
  const out: Record<string, string> = {};
  for (const [k, v] of Object.entries(fields)) if (v.trim()) out[k] = v.trim();
  return out;
}

/** Switches the variables between fields and JSON; JSON that doesn't parse stays as JSON. */
export function switchVarMode(status: Status, form: SetupForm, json: boolean): SetupForm {
  if (json && !form.varJson) return { ...form, varJson: true, varJsonText: JSON.stringify(form.fields, null, 2) };
  if (!json && form.varJson) {
    const parsed = parseVars(form.varJsonText, declaredVariables(status), blueprintLabel(status));
    if (parsed.err) return form;
    const fields: Record<string, string> = {};
    for (const d of declaredVariables(status)) fields[d.name] = parsed.vals?.[d.name] || "";
    return { ...form, varJson: false, fields };
  }
  return form;
}

/** The help under the variables' JSON: what's wrong with it, or what it takes. */
export function varsHelp(check: SetupCheck): { text: string; error: boolean } {
  return check.varError
    ? { text: check.varError, error: true }
    : { text: "One object of text values, one for each variable the blueprint declares. Fields shows the same values.", error: false };
}

/** The note under the variables' fields. */
export function fieldNote(status: Status): string {
  return "Declared in blueprint " + blueprintLabel(status) + ".";
}

/** Whether the page offers efforts for `provider`. */
export function takesEffort(provider: string): boolean {
  return provider === "openai" || provider === CLAUDE_CODE;
}

/** The providers chat works with, in the order the page offers them, and the key each needs. */
const CHAT_PROVIDERS: [string, string][] = [
  ["anthropic", "ANTHROPIC_API_KEY"],
  ["openai", "OPENAI_API_KEY"],
  ["google", "GEMINI_API_KEY"],
];

/** Whether the playground has a key for `provider`, wherever it found it. */
export function hasKey(status: Status, provider: string): boolean {
  return status.chat_models.some((m) => m.startsWith(provider + ":")) || !!status.model_keys?.some((k) => k.provider === provider && k.from);
}

/** The Agent field's note: how to add a key when there's none yet, else where keys stay. */
export function agentNote(status: Status): string {
  return status.chat_models.length
    ? "Keys stay with the playground; this page never sees one once it’s saved."
    : "No model key yet. Add one with Add a key, put it in the project’s .env, or run `submilli secret put OPENAI_API_KEY`.";
}

export interface ProviderOption {
  provider: string;
  label: string;
  model: string;
  pressed: boolean;
  /** The key it still needs, or `null`. */
  needs: string | null;
  /** For Claude Code: a word after its name and the line under the choices while it can't answer, or `null`. */
  unavailable: { suffix: string; reason: string } | null;
}

/**
 * One button per provider chat works with, each with its default model, then Claude Code.
 * Every one can be picked; one without a key, or a Claude Code that can't answer, says
 * so, and the form won't start until it can.
 */
export function providerOptions(status: Status, provider: string): ProviderOption[] {
  const models = new Map<string, string>();
  for (const named of status.chat_models) {
    const [p, m] = split(named);
    if (!models.has(p)) models.set(p, m);
  }
  for (const k of status.model_keys ?? []) if (!models.has(k.provider) && k.default_model) models.set(k.provider, k.default_model);
  const known = CHAT_PROVIDERS.map(([p]) => p);
  // A provider the server offers that the page doesn't know yet still shows, after the rest.
  const all = [...CHAT_PROVIDERS, ...[...models.keys()].filter((p) => !known.includes(p)).map((p): [string, string] => [p, ""])];
  const keyed = all.map(([p, key]): ProviderOption => ({
    provider: p,
    label: providerName(p),
    model: models.get(p) || "",
    pressed: p === provider,
    needs: hasKey(status, p) ? null : "No " + providerName(p) + " key yet (" + key + ").",
    unavailable: null,
  }));
  const claudeCode: ProviderOption = {
    provider: CLAUDE_CODE,
    label: providerName(CLAUDE_CODE),
    model: CLAUDE_CODE_MODELS[0][0],
    pressed: provider === CLAUDE_CODE,
    needs: null,
    unavailable: claudeCodeUnavailable(status.claude_code),
  };
  return [...keyed, claudeCode];
}

/** Why Claude Code can't answer, in a word and in a line saying what to do; `null` once it can. */
function claudeCodeUnavailable(cc: ClaudeCodeStatus | undefined): { suffix: string; reason: string } | null {
  if (!cc) return { suffix: "checking", reason: "Checking for Claude Code on this machine…" };
  if (cc.state === "available") return null;
  const suffix = { missing: "not installed", "too-old": "too old", "signed-out": "signed out", failed: "not checked" }[cc.state] ?? "unavailable";
  return { suffix, reason: (cc.reason || "Claude Code can’t answer").replace(/\.$/, "") + "." };
}

/** The agent step's line once it's done: the agent, its model, and Claude Code's spending limit. */
export function agentSummary(label: string | undefined, form: SetupForm, settings: ChatSettings | null): string {
  const limit = form.provider === CLAUDE_CODE && settings?.max_budget_usd ? "spending limit " + dollars(settings.max_budget_usd) : "";
  return [label, form.model.trim(), limit].filter(Boolean).join(" · ") || "Choose an agent";
}

/** Whom Claude Code bills, as the line under the agent choice says it; "" until it can answer. */
export function billingLine(status: Status): string {
  const cc = status.claude_code;
  if (cc?.state !== "available") return "";
  if (cc.subscription) return "Bills your Claude subscription, as your own Claude Code does. Your settings, hooks, and CLAUDE.md files don’t apply here.";
  return "Bills " + billedName(cc) + ", not your Claude subscription. The first message asks you to confirm.";
}

/** What Claude Code bills other than the subscription, and where it sends, when an address is set. */
export function billedName(cc: ClaudeCodeStatus): string {
  const names: Record<string, string> = {
    "claude.ai": "your Claude login",
    api_key: "an Anthropic API key",
    api_key_helper: "the key your apiKeyHelper gives",
    oauth_token: "a Claude login token (CLAUDE_CODE_OAUTH_TOKEN)",
    third_party: "a cloud provider account",
  };
  const name = names[cc.source ?? ""] ?? cc.source ?? "an unknown account";
  return name + (cc.base_url ? ", sending to " + cc.base_url : "");
}

export function modelHint(status: Status, provider: string): string {
  const named = status.chat_models.find((m) => m.startsWith(provider + ":")) || "";
  return "Default for " + providerName(provider) + ": " + split(named)[1] + ". Any model name the provider knows works; an unknown one gets the provider’s own error.";
}

/** Only OpenAI and Claude Code take an effort for now, so elsewhere only Default can be picked. */
export function effortOptions(provider: string, effort: string): { effort: string; label: string; pressed: boolean; disabled: boolean }[] {
  const levels = [
    ["", "Default"],
    ["low", "Low"],
    ["medium", "Medium"],
    ["high", "High"],
    ...(provider === CLAUDE_CODE ? [["xhigh", "Extra high"], ["max", "Max"]] : []),
  ];
  return levels.map(([e, label]) => ({ effort: e, label, pressed: e === effort, disabled: !takesEffort(provider) && e !== "" }));
}

export function effortHint(provider: string): string {
  if (provider === CLAUDE_CODE) return "How hard Claude Code thinks before it answers. Default leaves it to Claude Code.";
  if (provider === "anthropic") return "Not available for Claude yet: its thinking has to be handed back with each tool result, which chat doesn’t do yet.";
  if (provider === "openai") return "How hard the model reasons before it answers. Default leaves it to the model.";
  return "Not available for " + providerName(provider) + " yet: its thinking setting differs from model to model.";
}

/** What the key dialog says about `provider`'s key now: none, saved here, or one that comes first. */
export function keyNow(status: Status, provider: string): { text: string; saved: boolean } {
  const key = status.model_keys?.find((k) => k.provider === provider);
  const name = providerName(provider);
  const variable = key?.variable || keyVariable(provider);
  // A playground too old to say where its keys are still says which providers have one.
  if (!key?.from) return hasKey(status, provider) ? { text: "A " + name + " key is in use.", saved: false } : { text: "No " + name + " key yet.", saved: false };
  if (key.from === "secret_store") return { text: "A saved " + name + " key is in use. Saving replaces it.", saved: true };
  const where = key.from === "env_file" ? "the project’s .env" : "the shell the playground started from";
  return {
    text: "The " + name + " key in use comes from " + where + ", which comes first. A key saved here is used once " + variable + " is gone from there.",
    saved: false,
  };
}

/** The line under the key field: where a saved key goes. */
export function keyNote(status: Status, provider: string): string {
  const variable = status.model_keys?.find((k) => k.provider === provider)?.variable || keyVariable(provider);
  return "Saved on this machine in your local secret store, as `submilli secret put " + variable + "` does. It isn’t shown again.";
}

/** The notice after a save: done, or saved but not used yet. */
export function keySaved(provider: string, saved: SavedKey, removed: boolean): string {
  const name = providerName(provider);
  if (removed) return saved.from ? "Removed the saved " + name + " key; the one from " + (saved.from === "env_file" ? "the project’s .env" : "the shell") + " is in use." : "Removed the saved " + name + " key.";
  if (saved.from === "secret_store") return "Saved the " + name + " key.";
  return "Saved the " + name + " key, but the one from " + (saved.from === "env_file" ? "the project’s .env" : "the shell") + " comes first.";
}

/** The variable a key for `provider` goes under, as `submilli secret put` names it. */
export function keyVariable(provider: string): string {
  return CHAT_PROVIDERS.find(([p]) => p === provider)?.[1] || "";
}
