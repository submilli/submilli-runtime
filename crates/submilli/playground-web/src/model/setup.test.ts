import { describe, expect, it } from "vitest";
import { agentSummary, billingLine, checkSetup, declaredVariables, effortHint, effortOptions, initialSetup, modelHint, parseVars, providerOptions, INITIAL_SETUP, type SetupForm } from "./setup";
import type { ClaudeCodeStatus, SessionSummary } from "../api/types";
import { chatSession } from "./testdata";
import type { Status } from "../api/types";

const status = chatSession().status as Status;
const withDecls: Status = {
  ...status,
  declared_variables: [
    { name: "customerId", required: true, description: "The customer this session may access.", example: "cus_northwind" },
    { name: "region", required: true },
    { name: "plan", required: true },
    { name: "note", required: false },
  ],
};
const form = (over: Partial<SetupForm>): SetupForm => ({ ...INITIAL_SETUP, provider: "anthropic", model: "claude-sonnet-5-5", ...over });

describe("the variables a new session asks for", () => {
  it("come from the blueprint's declarations", () => {
    expect(declaredVariables(withDecls).map((d) => d.name)).toEqual(["customerId", "region", "plan", "note"]);
    expect(declaredVariables(status)).toEqual([{ name: "customerId", required: true }]);
  });

  it("as JSON take one object of declared text values", () => {
    const decls = declaredVariables(withDecls);
    expect(parseVars('{"customerId": "cus_a", "note": ""}', decls, "billing v2")).toEqual({ vals: { customerId: "cus_a" } });
    expect(parseVars("[1]", decls, "billing v2")).toEqual({ err: "Use one object: { \"name\": \"value\", … }." });
    expect(parseVars('{"other": "x"}', decls, "billing v2")).toEqual({ err: "other isn’t declared in blueprint billing v2." });
    expect(parseVars('{"customerId": 3}', decls, "billing v2")).toEqual({ err: "customerId must be text. Blueprint variables are text for now." });
    expect(parseVars("{", decls, "billing v2").err).toMatch(/^Not valid JSON yet: /);
  });

  it("name one missing variable and how many more", () => {
    const check = checkSetup(withDecls, form({ fields: { customerId: "cus_a" } }));
    expect(check.cantStart).toBe(true);
    expect(check.hint).toBe("Still needed: region (+1 more).");
    expect(checkSetup(withDecls, form({ fields: { customerId: "a", region: "eu" } })).hint).toBe("Still needed: plan.");
  });

  it("with JSON that doesn't parse, say to fix it", () => {
    const check = checkSetup(withDecls, form({ varJson: true, varJsonText: "{" }));
    expect(check.hint).toBe("Fix the variables’ JSON to start.");
    expect(check.cantStart).toBe(true);
  });
});

describe("the agent and its settings", () => {
  const ok = { customerId: "pied_piper" };

  it("start when the variables, model, and provider settings check out", () => {
    const check = checkSetup(status, form({ fields: ok, max: "2000", providerJson: '{"temperature": 0.2}' }));
    expect(check).toMatchObject({
      cantStart: false,
      problem: "",
      hint: "The variables, the agent, and its settings are fixed once the session starts.",
      variables: ok,
      model: "anthropic:claude-sonnet-5-5",
      settings: { max_output: 2000, provider: { temperature: 0.2 } },
    });
  });

  it("refuse a bad model name", () => {
    expect(checkSetup(status, form({ fields: ok, model: "claude sonnet" })).hint).toBe("A model name is letters, digits, “.”, “_”, and “-”.");
    expect(checkSetup(status, form({ fields: ok, model: "models/gemini-3" })).problem).toBe("");
    expect(checkSetup(status, form({ fields: ok, model: " " })).hint).toBe("Give the model’s name.");
  });

  it("say what's wrong with each field that stops the start", () => {
    const check = checkSetup(withDecls, form({ fields: { customerId: "cus_a" }, model: "claude sonnet", max: "lots" }));
    expect(check.variableInvalid).toEqual({ region: "Required.", plan: "Required." });
    expect(check.modelInvalid).toEqual({
      model: "A model name is letters, digits, “.”, “_”, and “-”.",
      max: "Max output is a whole number of tokens.",
    });
    const ready = checkSetup(status, form({ fields: ok }));
    expect(ready.variableInvalid).toEqual({});
    expect(ready.modelInvalid).toEqual({});
  });

  it("refuse a max output that isn't a whole number", () => {
    for (const max of ["lots", "0", "1.5"]) {
      expect(checkSetup(status, form({ fields: ok, max })).hint).toBe("Max output is a whole number of tokens.");
    }
  });

  it("refuse provider settings that aren't a JSON object", () => {
    expect(checkSetup(status, form({ fields: ok, providerJson: "[1, 2]" })).hint).toBe(
      "Provider settings: give a JSON object, such as {\"temperature\": 0.2}.",
    );
    expect(checkSetup(status, form({ fields: ok, providerJson: "{nope" })).hint).toMatch(/^Provider settings: not JSON \(.+\)\.$/);
  });

  it("refuse fields the playground sets itself", () => {
    expect(checkSetup(status, form({ fields: ok, providerJson: '{"max_tokens": 5}' })).hint).toBe(
      "Provider settings can’t set “max_tokens”: the playground sets it itself.",
    );
    expect(checkSetup(status, form({ fields: ok, providerJson: '{"thinking": {}}' })).hint).toBe(
      "Provider settings can’t set “thinking”: the playground sets it itself (Claude’s thinking isn’t supported yet).",
    );
    const withOpenai: Status = { ...status, chat_models: [...status.chat_models, "openai:gpt-5.5"] };
    expect(
      checkSetup(withOpenai, form({ fields: ok, provider: "openai", model: "gpt-5", effort: "low", providerJson: '{"reasoning": {"effort": "high"}}' })).hint,
    ).toBe("Provider settings can’t set “reasoning.effort”: Effort sets it.");
  });

  it("let a provider without a key be picked, but not start until it has one", () => {
    const f = checkSetup(status, form({ fields: ok, provider: "google", model: "gemini-flash-latest" }));
    expect(f.cantStart).toBe(true);
    expect(f.hint).toBe("Gemini needs a key first: use Add key under Provider.");
  });

  it("offer effort only where the provider takes it", () => {
    expect(effortOptions("anthropic", "").map((o) => [o.label, o.disabled])).toEqual([
      ["Default", false],
      ["Low", true],
      ["Medium", true],
      ["High", true],
    ]);
    expect(effortHint("anthropic")).toBe(
      "Not available for Claude yet: its thinking has to be handed back with each tool result, which chat doesn’t do yet.",
    );
    expect(effortHint("openai")).toBe("How hard the model reasons before it answers. Default leaves it to the model.");
    // Gemini doesn't take one either; the CLI says the same.
    expect(effortOptions("google", "").filter((o) => !o.disabled).map((o) => o.label)).toEqual(["Default"]);
    expect(effortOptions("openai", "").every((o) => !o.disabled)).toBe(true);
    expect(effortHint("google")).toBe("Not available for Gemini yet: its thinking setting differs from model to model.");
  });

  it("offer one provider per model the playground started with, and its default model", () => {
    expect(providerOptions(status, "anthropic")).toEqual([
      { provider: "anthropic", label: "Claude", model: "claude-sonnet-5-5", pressed: true, needs: null, unavailable: null },
      // Every provider chat works with can be picked; one without a key says so.
      { provider: "openai", label: "OpenAI", model: "", pressed: false, needs: "No OpenAI key yet (OPENAI_API_KEY).", unavailable: null },
      { provider: "google", label: "Gemini", model: "", pressed: false, needs: "No Gemini key yet (GEMINI_API_KEY).", unavailable: null },
      // Claude Code needs no key; until the playground has looked for it, it says so.
      { provider: "claude-code", label: "Claude Code", model: "claude-sonnet-5-5", pressed: false, needs: null, unavailable: { suffix: "checking", reason: "Checking for Claude Code on this machine…" } },
    ]);
    expect(modelHint(status, "anthropic")).toBe(
      "Default for Claude: claude-sonnet-5-5. Any model name the provider knows works; an unknown one gets the provider’s own error.",
    );
  });
});

describe("fixes found trying the form", () => {
  const decls: Status = { ...status, declared_variables: [{ name: "note", required: false }, { name: "customerId", required: true }] };

  it("trims values typed as JSON, as the fields do", () => {
    expect(parseVars('{"customerId": " pied_piper "}', declaredVariables(status), "billing v2")).toEqual({ vals: { customerId: "pied_piper" } });
  });

  it("puts the required variables first", () => {
    expect(declaredVariables(decls).map((d) => d.name)).toEqual(["customerId", "note"]);
  });

  it("refuses a max output past what the playground takes", () => {
    expect(checkSetup(status, form({ fields: { customerId: "pied_piper" }, max: "99999999999999999999" })).hint).toBe("Max output is at most 1,000,000 tokens.");
  });

});

it("selects existing blueprint declarations without changing shared status or accepting unknown names", () => {
  const reports: Status = {
    ...status,
    blueprints: [
      { name: status.blueprint.name, path: status.blueprint.path, packages: status.packages, declared_variables: status.declared_variables, blueprint_status: status.blueprint_status },
      { name: "reports", path: "submilli/blueprints/reports.yaml", packages: [], declared_variables: [{ name: "region", required: true }], blueprint_status: { version: 3, refused: null } },
    ],
  };
  const missing = checkSetup(reports, form({ blueprint: "reports", fields: { customerId: "cus_northwind" } }));
  expect(missing.missingVariables).toEqual(["region"]);
  const ready = checkSetup(reports, form({ blueprint: "reports", fields: { region: "eu" } }));
  expect(ready.cantStart).toBe(false);
  expect(ready.variables).toEqual({ region: "eu" });
  expect(reports.blueprint.name).toBe(status.blueprint.name);
  expect(checkSetup(reports, form({ blueprint: "../other.yaml", fields: { customerId: "cus_northwind" } })).cantStart).toBe(true);
});

describe("Claude Code as the agent", () => {
  const ready: ClaudeCodeStatus = { state: "available", version: "2.1.310", source: "claude.ai", subscription: true };
  const withCc = (cc?: ClaudeCodeStatus): Status => ({ ...status, claude_code: cc });
  const cc = (over: Partial<SetupForm> = {}): SetupForm => form({ provider: "claude-code", model: "claude-sonnet-5-5", fields: { customerId: "cus_a" }, ...over });
  const option = (s: Status) => providerOptions(s, "claude-code").find((o) => o.provider === "claude-code")!;

  it("is offered once Claude Code can answer, with no key needed", () => {
    expect(option(withCc(ready))).toEqual({ provider: "claude-code", label: "Claude Code", model: "claude-sonnet-5-5", pressed: true, needs: null, unavailable: null });
    const check = checkSetup(withCc(ready), cc());
    expect(check.cantStart).toBe(false);
    expect(check.model).toBe("claude-code:claude-sonnet-5-5");
    // The limit is always set, so the session shows what it runs under.
    expect(check.settings).toEqual({ max_budget_usd: 1 });
  });

  it("names the spending limit once the agent step is done", () => {
    expect(agentSummary("Claude Code", cc({ model: "claude-haiku-5-5" }), checkSetup(withCc(ready), cc({ model: "claude-haiku-5-5" })).settings)).toBe("Claude Code · claude-haiku-5-5 · spending limit $1.00");
    expect(agentSummary("Claude Code", cc({ budget: "0.05" }), checkSetup(withCc(ready), cc({ budget: "0.05" })).settings)).toBe("Claude Code · claude-sonnet-5-5 · spending limit $0.05");
    expect(agentSummary("OpenAI", form({ provider: "openai", model: "gpt-6-luna" }), { max_output: 100 })).toBe("OpenAI · gpt-6-luna");
  });

  it("says it's still being checked before the first status names it", () => {
    expect(option(withCc(undefined)).unavailable?.suffix).toBe("checking");
    expect(checkSetup(withCc(undefined), cc()).problem).toBe("Still checking for Claude Code. Try again in a moment.");
  });

  it("is marked unavailable with each reason in view, and no key problem", () => {
    for (const [state, suffix] of [["missing", "not installed"], ["too-old", "too old"], ["signed-out", "signed out"], ["failed", "not checked"]] as const) {
      const s = withCc({ state, subscription: false, reason: "Claude Code is signed out; run `claude` once to sign in" });
      expect(option(s).unavailable).toEqual({ suffix, reason: "Claude Code is signed out; run `claude` once to sign in." });
      const check = checkSetup(s, cc());
      expect(check.cantStart).toBe(true);
      expect(check.problem).toBe("Claude Code can’t answer: Claude Code is signed out; run `claude` once to sign in.");
      expect(check.problem).not.toMatch(/key/);
    }
  });

  it("takes a model by its full name, and every Claude Code effort", () => {
    // Claude Code's short names move to newer models, so they aren't taken.
    expect(checkSetup(withCc(ready), cc({ model: "sonnet" })).modelInvalid.model).toBe("Give a full model name with its version, such as claude-opus-5-5: Claude Code’s short names move to newer models.");
    expect(checkSetup(withCc(ready), cc({ model: "" })).modelInvalid.model).toBe("Choose a model, or type a full model name.");
    // A newer model than the page offers can be typed in.
    for (const model of ["claude-opus-5-5", "claude-fable-5-1", "claude-opus-6"]) expect(checkSetup(withCc(ready), cc({ model })).modelInvalid.model).toBeUndefined();
    expect(effortOptions("claude-code", "xhigh").filter((o) => !o.disabled).map((o) => o.label)).toEqual(["Default", "Low", "Medium", "High", "Extra high", "Max"]);
    expect(checkSetup(withCc(ready), cc({ effort: "max" })).settings).toEqual({ effort: "max", max_budget_usd: 1 });
  });

  it("leaves out max output and provider settings carried in the form", () => {
    expect(checkSetup(withCc(ready), cc({ max: "abc", providerJson: "{nope" })).settings).toEqual({ max_budget_usd: 1 });
  });

  it("checks the spending limit: more than 0, at most 100 dollars", () => {
    const limit = (budget: string) => checkSetup(withCc(ready), cc({ budget }));
    expect(limit("2.50").settings).toEqual({ max_budget_usd: 2.5 });
    expect(limit("100").settings).toEqual({ max_budget_usd: 100 });
    expect(limit("0").modelInvalid.budget).toBe("The spending limit is an amount in dollars, more than 0.");
    expect(limit("-1").modelInvalid.budget).toBe("The spending limit is an amount in dollars, more than 0.");
    expect(limit("$5").modelInvalid.budget).toBe("The spending limit is an amount in dollars, more than 0.");
    expect(limit("100.01").modelInvalid.budget).toBe("The spending limit is at most $100.00.");
  });

  it("carries the last session's model, effort, and limit into a new one", () => {
    const last = { session: "s", started_at_micros: 1, model: "claude-code:claude-opus-5-5", settings: { effort: "xhigh", max_budget_usd: 3 } } as SessionSummary;
    expect(initialSetup(withCc(ready), [last])).toMatchObject({ provider: "claude-code", model: "claude-opus-5-5", effort: "xhigh", budget: "3.00" });
  });

  it("says whom it bills", () => {
    expect(billingLine(withCc(undefined))).toBe("");
    expect(billingLine(withCc(ready))).toMatch(/^Bills your Claude subscription/);
    expect(billingLine(withCc({ ...ready, source: "api_key", subscription: false }))).toBe("Bills an Anthropic API key, not your Claude subscription. The first message asks you to confirm.");
    expect(billingLine(withCc({ ...ready, base_url: "https://proxy.example", subscription: false }))).toBe(
      "Bills your Claude login, sending to https://proxy.example, not your Claude subscription. The first message asks you to confirm.",
    );
  });

  it("has its own effort note", () => {
    expect(effortHint("claude-code")).toBe("How hard Claude Code thinks before it answers. Default leaves it to Claude Code.");
  });
});
