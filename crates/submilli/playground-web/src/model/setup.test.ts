import { describe, expect, it } from "vitest";
import { checkSetup, declaredVariables, effortHint, effortOptions, modelHint, parseVars, providerOptions, INITIAL_SETUP, type SetupForm } from "./setup";
import { chatSession } from "./testdata";
import type { Status } from "../api/types";

const status = chatSession().status as Status;
const withDecls: Status = {
  ...status,
  declared_variables: [
    { name: "customerId", required: true },
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
  const ok = { customerId: "cus_northwind" };

  it("start when everything checks out, with the CLI command that does the same", () => {
    const check = checkSetup(status, form({ fields: ok, max: "2000", providerJson: '{"temperature": 0.2}' }));
    expect(check).toMatchObject({
      cantStart: false,
      problem: "",
      hint: "The variables, the agent, and its settings are fixed once the session starts.",
      variables: ok,
      model: "anthropic:claude-sonnet-5-5",
      settings: { max_output: 2000, provider: { temperature: 0.2 } },
    });
    expect(check.cli).toBe(
      "submilli playground chat --model anthropic:claude-sonnet-5-5 --max-output 2000 --provider-settings '{\"temperature\":0.2}' \"<message>\"",
    );
  });

  it("refuse a bad model name", () => {
    expect(checkSetup(status, form({ fields: ok, model: "claude sonnet" })).hint).toBe("A model name is letters, digits, “.”, “_”, and “-”.");
    expect(checkSetup(status, form({ fields: ok, model: "models/gemini-3" })).problem).toBe("");
    expect(checkSetup(status, form({ fields: ok, model: " " })).hint).toBe("Give the model’s name.");
  });

  it("say what's wrong with each field that stops the start", () => {
    const check = checkSetup(withDecls, form({ fields: { customerId: "cus_a" }, model: "claude sonnet", max: "lots" }));
    expect(check.invalid).toEqual({
      region: "Required.",
      plan: "Required.",
      model: "A model name is letters, digits, “.”, “_”, and “-”.",
      max: "Max output is a whole number of tokens.",
    });
    // A form that can't start gives no command.
    expect(check.cli).toBe("");
    expect(checkSetup(status, form({ fields: ok })).invalid).toEqual({});
    expect(checkSetup(status, form({ fields: ok, model: "gpt 6 luna", provider: "openai" })).cli).toBe("");
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
    expect(f.hint).toBe("Gemini needs a key first: use Add a key under Agent.");
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
      { provider: "anthropic", label: "Claude", model: "claude-sonnet-5-5", pressed: true, needs: null },
      // Every provider chat works with can be picked; one without a key says so.
      { provider: "openai", label: "OpenAI", model: "", pressed: false, needs: "No OpenAI key yet (OPENAI_API_KEY)." },
      { provider: "google", label: "Gemini", model: "", pressed: false, needs: "No Gemini key yet (GEMINI_API_KEY)." },
    ]);
    expect(modelHint(status, "anthropic")).toBe(
      "Default for Claude: claude-sonnet-5-5. Any model name the provider knows works; an unknown one gets the provider’s own error.",
    );
  });
});

describe("fixes found trying the form", () => {
  const decls: Status = { ...status, declared_variables: [{ name: "note", required: false }, { name: "customerId", required: true }] };

  it("trims values typed as JSON, as the fields do", () => {
    expect(parseVars('{"customerId": " cus_northwind "}', declaredVariables(status), "billing v2")).toEqual({ vals: { customerId: "cus_northwind" } });
  });

  it("puts the required variables first", () => {
    expect(declaredVariables(decls).map((d) => d.name)).toEqual(["customerId", "note"]);
  });

  it("refuses a max output past what the playground takes", () => {
    expect(checkSetup(status, form({ fields: { customerId: "cus_northwind" }, max: "99999999999999999999" })).hint).toBe("Max output is at most 1,000,000 tokens.");
  });

  it("binds variables that differ from the binding before the CLI's chat, which uses the binding", () => {
    const same = checkSetup(status, form({ fields: { customerId: status.binding.variables.customerId } }));
    expect(same.cliBind).toBe("");
    const other = checkSetup(status, form({ fields: { customerId: "cus globex" } }));
    expect(other.cliBind).toBe("submilli playground bind customerId='cus globex'");
  });
});
