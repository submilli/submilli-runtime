// The New session form: its variables, the agent and its settings, checked as the
// playground would check them before the first message goes.
import type { ChatSettings, DeclaredVariable, Status } from "../api/types";
import { namesOf } from "./refs";
import { providerName } from "./format";

export interface SetupForm {
  provider: string;
  model: string;
  /** "" for the provider's default. */
  effort: string;
  max: string;
  /** Provider settings, as JSON text. */
  providerJson: string;
  /** Variables are being edited as one JSON object rather than as fields. */
  varJson: boolean;
  varJsonText: string;
  fields: Record<string, string>;
}

export const INITIAL_SETUP: SetupForm = { provider: "", model: "", effort: "", max: "", providerJson: "", varJson: false, varJsonText: "", fields: {} };

/** What the playground sets itself in each provider's request, so provider settings can't. */
export const OWN_FIELDS: Record<string, string[]> = {
  anthropic: ["model", "messages", "system", "tools", "max_tokens", "stream", "thinking"],
  openai: ["model", "input", "instructions", "tools", "max_output_tokens", "stream"],
};

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
export function initialSetup(status: Status): SetupForm {
  const [provider, model] = split(status.chat_models[0] || ":");
  const fields: Record<string, string> = {};
  for (const d of declaredVariables(status)) fields[d.name] = status.binding.variables[d.name] || "";
  return { ...INITIAL_SETUP, provider, model, fields };
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
  /** The same session from the CLI. */
  cli: string;
  /**
   * The CLI's chat takes the bound variables, so the variables that differ from them are
   * bound first; "" when they all match.
   */
  cliBind: string;
  /** What's wrong with each field that stops the start, by a variable's name, "model", "max", "provider" or "json". */
  invalid: Record<string, string>;
}

export function checkSetup(status: Status, form: SetupForm): SetupCheck {
  const decls = declaredVariables(status);
  const parsed = form.varJson ? parseVars(form.varJsonText, decls, blueprintLabel(status)) : { vals: trimmed(form.fields) };
  const variables = parsed.vals || {};
  const missing = decls.filter((d) => d.required && !(variables[d.name] || "").trim()).map((d) => d.name);

  const model = form.model.trim();
  const max = form.max.trim();
  const json = form.providerJson.trim();
  const set: ChatSettings = {};
  let problem = "";
  const invalid: Record<string, string> = Object.fromEntries(missing.map((name) => [name, "Required."]));
  if (parsed.err) invalid.json = parsed.err;
  const fail = (field: string, text: string) => {
    invalid[field] = text;
    problem = problem || text;
  };
  if (!model) fail("model", "Give the model’s name.");
  else if (!/^(models\/)?[A-Za-z0-9._-]+$/.test(model)) fail("model", "A model name is letters, digits, “.”, “_”, and “-”.");
  if (form.effort) set.effort = form.effort;
  if (max) {
    if (!/^[0-9]+$/.test(max) || Number(max) < 1) fail("max", "Max output is a whole number of tokens.");
    else if (Number(max) > MAX_OUTPUT) fail("max", "Max output is at most " + MAX_OUTPUT.toLocaleString("en-US") + " tokens.");
    else set.max_output = Number(max);
  }
  if (json) {
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
  const settings = Object.keys(set).length ? set : null;
  const more = missing.length > 1 ? " (+" + (missing.length - 1) + " more)" : "";
  const hint = parsed.err
    ? "Fix the variables’ JSON to start."
    : missing.length
      ? "Still needed: " + missing[0] + more + "."
      : problem || "The variables, the agent, and its settings are fixed once the session starts.";
  const cli =
    "submilli playground chat --model " + shellWord(form.provider + ":" + model) + (form.effort ? " --effort " + form.effort : "") +
    (max ? " --max-output " + shellWord(max) : "") + (settings && settings.provider ? " --provider-settings " + shellWord(JSON.stringify(settings.provider)) : "") + ' "<message>"';
  const bound = status.binding.variables;
  const differ = Object.keys(variables).filter((k) => variables[k] !== bound[k]);
  // An optional variable left empty is unset; a required one left empty can't start anyway.
  const dropped = Object.keys(bound).filter((k) => !(k in variables) && decls.some((d) => d.name === k && !d.required));
  const cliBind =
    differ.length || dropped.length
      ? "submilli playground bind " + [...differ.map((k) => k + "=" + shellWord(variables[k])), ...dropped.map((k) => "--unset " + k)].join(" ")
      : "";
  // A form that can't start gives no command, which couldn't run either.
  const cantStart = missing.length > 0 || !!parsed.err || !!problem;
  return {
    variables,
    cliBind: cantStart ? "" : cliBind,
    invalid,
    model: form.provider + ":" + model,
    settings,
    problem,
    varError: parsed.err || "",
    hint,
    cantStart,
    cli: cantStart ? "" : cli,
  };
}

/** A value as a shell takes it: quoted when it holds anything but plain characters. */
function shellWord(value: string): string {
  return /^[A-Za-z0-9._@:/-]+$/.test(value) ? value : "'" + value.replace(/'/g, "'\\''") + "'";
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
  return "Declared in blueprint " + blueprintLabel(status) + ". Fixed once the session starts.";
}

/** One button per provider the playground has a model key for, with its default model. */
export function providerOptions(status: Status, provider: string): { provider: string; label: string; model: string; pressed: boolean }[] {
  const seen = new Set<string>();
  const out: { provider: string; label: string; model: string; pressed: boolean }[] = [];
  for (const named of status.chat_models) {
    const [p, m] = split(named);
    if (seen.has(p)) continue;
    seen.add(p);
    out.push({ provider: p, label: providerName(p), model: m, pressed: p === provider });
  }
  return out;
}

export function modelHint(status: Status, provider: string): string {
  const named = status.chat_models.find((m) => m.startsWith(provider + ":")) || "";
  return "Default for " + providerName(provider) + ": " + split(named)[1] + ". Any model name the provider knows works; an unknown one gets the provider’s own error.";
}

/** Claude's effort isn't available yet, so only Default can be picked for it. */
export function effortOptions(provider: string, effort: string): { effort: string; label: string; pressed: boolean; disabled: boolean }[] {
  return [
    ["", "Default"],
    ["low", "Low"],
    ["medium", "Medium"],
    ["high", "High"],
  ].map(([e, label]) => ({ effort: e, label, pressed: e === effort, disabled: provider === "anthropic" && e !== "" }));
}

export function effortHint(provider: string): string {
  return provider === "anthropic"
    ? "Not available for Claude yet: its thinking has to be handed back with each tool result, which chat doesn’t do yet."
    : "OpenAI’s reasoning effort.";
}
