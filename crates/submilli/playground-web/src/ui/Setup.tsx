// The New session form. What it holds stays in the page until the first message is
// sent with it; Start session only checks it the way the playground would.
import { useEffect, useRef, useState, type FormEvent } from "react";

import type { ActionOutcome } from "../api/client";
import type { SavedKey, Status } from "../api/types";
import {
  billingLine,
  checkSetup,
  CLAUDE_CODE,
  CLAUDE_CODE_MODELS,
  DEFAULT_BUDGET,
  declaredVariables,
  effortHint,
  effortOptions,
  providerOptions,
  selectedSetupStatus,
  switchVarMode,
  varsHelp,
  type SetupForm,
} from "../model/setup";
import { cls, Gated } from "./controls";
import { KeyDialog } from "./Dialogs";
import { ProviderKeyStatus } from "./ProviderKeyStatus";
import "../styles/setup-dialogs.css";

/** What the form's key dialog does: save or remove a key, then say what happened. */
export interface KeyActions {
  save: (provider: string, key: string | null) => Promise<ActionOutcome<SavedKey>>;
  done: (notice: string) => void;
}

export function Setup(props: {
  status: Status;
  form: SetupForm;
  onForm: (form: SetupForm) => void;
  onStart: () => void;
  onBlueprint: () => void;
  onPackages: () => void;
  keys: KeyActions;
}) {
  const { form, onForm } = props;
  const status = selectedSetupStatus(props.status, form.blueprint);
  const blueprints = props.status.blueprints?.length ? props.status.blueprints : [props.status.blueprint];
  const [keyFor, setKeyFor] = useState<string | null>(null);
  const formRef = useRef<HTMLFormElement>(null);
  const settingsSummary = useRef<HTMLButtonElement>(null);
  const variablesSummary = useRef<HTMLButtonElement>(null);
  const [activeStep, setActiveStep] = useState<"model" | "variables" | null>("model");
  const [modelDone, setModelDone] = useState(false);
  const [variablesDone, setVariablesDone] = useState(false);
  const providers = providerOptions(status, form.provider);
  // An effort carries over to another agent only when that agent takes it too.
  const keptEffort = (provider: string) => (effortOptions(provider, form.effort).some((o) => o.pressed && !o.disabled) ? form.effort : "");
  // With no provider picked yet, one with a key is picked, else the first.
  useEffect(() => {
    if (form.provider || !providers.length) return;
    const first = providers.find((o) => !o.needs && !o.unavailable) ?? providers[0];
    onForm({ ...form, provider: first.provider, model: first.model, effort: keptEffort(first.provider) });
  }, [providers, form, onForm]);
  const picked = providers.find((o) => o.pressed);
  const claudeCode = form.provider === CLAUDE_CODE;
  // Set when a provider is picked by saving its key; its model follows once status names it.
  const fillModel = useRef(false);
  useEffect(() => {
    if (!fillModel.current || !picked?.model) return;
    fillModel.current = false;
    if (!form.model) onForm({ ...form, model: picked.model });
  }, [picked, form, onForm]);
  const closeKey = () => {
    setKeyFor(null);
    settingsSummary.current?.focus({ preventScroll: true });
  };
  // Settings carried from the last session stay discoverable. After that, the disclosure
  // follows the reader and never discards values when it closes.
  const holdsSettings = !!form.providerJson.trim();
  const [advancedOpen, setAdvancedOpen] = useState(holdsSettings);
  const [providerSettingsOpen, setProviderSettingsOpen] = useState(holdsSettings);
  useEffect(() => {
    if (holdsSettings) {
      setAdvancedOpen(true);
      setProviderSettingsOpen(true);
    }
  }, [holdsSettings]);
  const check = checkSetup(status, form);
  const help = varsHelp(check);
  const variables = declaredVariables(status);
  // A field's problem shows once it's been typed in, or once Start session was pressed;
  // a form just opened isn't wrong yet.
  const [typed, setTyped] = useState<Record<string, boolean>>({});
  const [tried, setTried] = useState(false);
  const [triedModel, setTriedModel] = useState(false);
  const shown = (field: string, variable = false) => ((variable ? tried : triedModel) || typed[(variable ? "var:" : "") + field] ? (variable ? check.variableInvalid : check.modelInvalid)[field] : undefined);
  const set = (patch: Partial<SetupForm>, field?: string) => {
    if (field && !typed[field]) setTyped((t) => ({ ...t, [field]: true }));
    onForm({ ...form, ...patch });
  };
  // A field that stops the start says so, and points to the line under it that says why.
  const bad = (field: string, note?: string, variable = false) => ({
    "aria-invalid": !!shown(field, variable) || undefined,
    "aria-describedby": [note, shown(field, variable) && field !== "json" && "bad-" + (variable ? "var-" : "") + field].filter(Boolean).join(" ") || undefined,
  });
  const why = (field: string, variable = false) =>
    shown(field, variable) ? (
      <small id={"bad-" + (variable ? "var-" : "") + field} className="pg-field-err">
        {shown(field, variable)}
      </small>
    ) : null;
  const submit = (ev: FormEvent) => {
    ev.preventDefault();
    if (activeStep === "model") finishModel();
    else if (activeStep === "variables") finishVariables();
    else if (!check.cantStart && modelDone && variablesDone) props.onStart();
    else revealProblems();
  };
  const revealProblems = () => {
    const opensModel = !!check.problem;
    setActiveStep(opensModel ? "model" : "variables");
    if (opensModel) {
      setTriedModel(true);
      setModelDone(false);
      if (check.modelInvalid.max || check.modelInvalid.budget || check.modelInvalid.provider) setAdvancedOpen(true);
      if (check.modelInvalid.provider) setProviderSettingsOpen(true);
    } else {
      setTried(true);
      setVariablesDone(false);
    }
    // State-driven validation markers render on the next pass. Focus the first one after
    // that pass so missing context receives the same recovery treatment as model errors.
    setTimeout(() => {
      const badField = formRef.current?.querySelector<HTMLElement>((opensModel ? "#model-panel " : "#variables-panel ") + '[aria-invalid="true"]');
      (badField ?? (opensModel ? settingsSummary.current : null))?.focus();
    }, 0);
  };
  const missingVariables = check.missingVariables;
  const jsonMissing = form.varJson && tried && missingVariables.length > 0;
  const jsonInvalid = !!shown("json", true) || jsonMissing;
  const jsonHelpText = help.error ? help.text : jsonMissing ? "Still needed: " + missingVariables.join(", ") + "." : help.text;
  const finishModel = () => {
    if (check.problem) return revealProblems();
    setModelDone(true);
    setActiveStep(variablesDone ? null : "variables");
    setTimeout(() => variablesDone ? formRef.current?.querySelector<HTMLButtonElement>('[type="submit"]')?.focus() : variablesSummary.current?.focus(), 0);
  };
  const finishVariables = () => {
    if (check.varError || missingVariables.length) return revealProblems();
    setVariablesDone(true);
    setActiveStep(null);
    setTimeout(() => formRef.current?.querySelector<HTMLButtonElement>('[type="submit"]')?.focus(), 0);
  };
  const variableSummary = Object.entries(check.variables).map(([name, value]) => `${name}: ${value}`).join(" · ") || "No application variables";
  const settingsLabel = [picked?.label, form.model.trim()].filter(Boolean).join(" · ") || "Choose an agent";
  return (
    <>
      <form ref={formRef} className="pg-setup" onSubmit={submit}>
        <div className="pg-blueprint-choice">
          {blueprints.length > 1 ? <label className="pg-vfield"><span className="pg-vname">Blueprint</span><span className="ds-select"><select value={form.blueprint || props.status.blueprint.name} onChange={(event) => {
            onForm({ ...form, blueprint: event.target.value, fields: {}, varJsonText: "{}" });
            setModelDone(false); setVariablesDone(false); setActiveStep("model"); setTyped({}); setTried(false); setTriedModel(false);
          }}>{blueprints.map((blueprint) => <option key={blueprint.name} value={blueprint.name}>{blueprint.name}</option>)}</select></span></label>
            : <div className="pg-setup-row"><span className="pg-vname">Blueprint</span><strong>{status.blueprint.name}</strong></div>}
          <small className="pg-muted">Packages: {status.packages.filter((entry) => entry.importable).map((entry) => entry.name).join(", ") || "none"}</small>
          <div className="pg-setup-row">
            <button type="button" className="pg-link" disabled={status.blueprint_status.version === null} onClick={props.onBlueprint}>View blueprint</button>
            <button type="button" className="pg-link" onClick={props.onPackages}>Package guide</button>
          </div>
          <details className="ds-help"><summary>About blueprint selection</summary><p>Choose an existing blueprint from this local project. A new session uses that blueprint’s required variables, packages and access rules.</p></details>
        </div>
        <section className="ds-step pg-agent-section">
          <h3 className="ds-step-heading pg-step-heading"><button ref={settingsSummary} type="button" className="ds-step-trigger" aria-expanded={activeStep === "model"} aria-controls="model-panel" onClick={() => { setActiveStep("model"); setModelDone(false); }}>
            Configure model
            {modelDone && <span className="ds-step-state">✓ Done · Edit</span>}
            {modelDone && activeStep !== "model" && <span className="ds-step-summary">{settingsLabel}</span>}
          </button></h3>
          <div id="model-panel" className="ds-step-panel" hidden={activeStep !== "model"}>
            <details className="ds-help"><summary>About model configuration</summary><p>Choose the model that answers in this session. Advanced model and provider settings remain available below.</p></details>
            <div className="pg-model-primary">
              <div className="pg-vfield" role="group" aria-label="Provider">
                <span className="pg-vname">Provider</span>
                <div className="pg-choices">
                  {providers.map((o) => (
                    <button
                      type="button"
                      key={o.provider}
                      aria-pressed={o.pressed}
                      className={cls("pg-btn", o.pressed && "pg-primary")}
                      title={o.needs ?? o.unavailable?.reason ?? undefined}
                      onClick={() => set({ provider: o.provider, model: o.model, effort: keptEffort(o.provider) })}
                    >
                      {o.label}
                      {o.unavailable && <span className="pg-choice-note"> · {o.unavailable.suffix}</span>}
                    </button>
                  ))}
                </div>
                {claudeCode ? (
                  // Claude Code runs on the developer's own login: no key, but whom it bills.
                  <small role="status">{picked?.unavailable ? picked.unavailable.reason : billingLine(status)}</small>
                ) : (
                  <ProviderKeyStatus
                    configured={!!picked && !picked.needs}
                    providerLabel={picked?.label}
                    onManage={() => setKeyFor(form.provider || providers[0]?.provider || "")}
                  />
                )}
              </div>
              {claudeCode && (
                <div className="pg-vfield" role="group" aria-label="Model">
                  <span className="pg-vname">Model</span>
                  <div className="pg-choices">
                    {CLAUDE_CODE_MODELS.map(([m, label]) => (
                      <button type="button" key={m} aria-pressed={form.model === m} className={cls("pg-btn", form.model === m && "pg-primary")} onClick={() => set({ model: m }, "model")}>
                        {label}
                      </button>
                    ))}
                  </div>
                  {why("model")}
                </div>
              )}
              {form.provider !== "" && !claudeCode && (
                <label className="pg-vfield">
                  <span className="pg-vname">Model</span>
                  <input value={form.model} onChange={(ev) => set({ model: ev.target.value }, "model")} spellCheck={false} {...bad("model")} />
                  {why("model")}
                  <small>Use your provider’s model name.</small>
                </label>
              )}
            </div>
            {form.provider !== "" && (
              <details className="pg-advanced-details pg-model-advanced" open={advancedOpen} onToggle={(ev) => setAdvancedOpen(ev.currentTarget.open)}>
                <summary>Advanced model options <em className="opt">optional</em></summary>
                <div className="pg-model-options">
                  {form.provider !== "google" && (
                    <div className="pg-vfield" role="group" aria-label="Reasoning effort">
                      <span className="pg-vname">Reasoning effort</span>
                      <div className="pg-choices">
                        {effortOptions(form.provider, form.effort).map((o) => (
                          <Gated
                            key={o.effort}
                            reasonId="effort-help"
                            pressed={o.pressed}
                            className={cls("pg-btn", o.pressed && "pg-primary")}
                            blocked={o.disabled ? effortHint(form.provider) : null}
                            onClick={() => set({ effort: o.effort })}
                          >
                            {o.label}
                          </Gated>
                        ))}
                      </div>
                      <small id="effort-help">{effortHint(form.provider)}</small>
                    </div>
                  )}
                  {claudeCode ? (
                    <label className="pg-vfield">
                      <span className="pg-vname">Spending limit <em className="opt">optional</em></span>
                      <input value={form.budget} onChange={(ev) => set({ budget: ev.target.value }, "budget")} inputMode="decimal" placeholder={DEFAULT_BUDGET.toFixed(2)} {...bad("budget")} />
                      {why("budget")}
                      <small>The most one message may spend, in dollars. Claude Code stops when it’s reached.</small>
                    </label>
                  ) : (
                  <label className="pg-vfield">
                    <span className="pg-vname">Max output <em className="opt">optional</em></span>
                    <input value={form.max} onChange={(ev) => set({ max: ev.target.value }, "max")} inputMode="numeric" placeholder="4096" {...bad("max")} />
                    {why("max")}
                    <small>Maximum tokens in one answer.</small>
                  </label>
                  )}
                </div>
                {!claudeCode && <details className="pg-advanced-details" open={providerSettingsOpen} onToggle={(ev) => setProviderSettingsOpen(ev.currentTarget.open)}>
                  <summary>Provider settings <em className="opt">optional JSON</em></summary>
                  <div className="pg-vfield pg-provider-json">
                    <textarea
                      className="pg-json"
                      rows={4}
                      spellCheck={false}
                      aria-label="Provider settings as JSON"
                      {...bad("provider")}
                      placeholder='{"temperature": 0.2}'
                      value={form.providerJson}
                      onChange={(ev) => set({ providerJson: ev.target.value }, "provider")}
                    />
                    {why("provider")}
                    <small>Extra fields for the provider’s request. The playground controls the model, conversation, tools, and instructions.</small>
                  </div>
                </details>}
              </details>
            )}
            {triedModel && check.problem && <p className="pg-field-err" role="alert">{check.problem}</p>}
            <button type="button" className="pg-btn pg-primary pg-step-done" aria-label="Done configuring model" onClick={finishModel}>Done</button>
          </div>
        </section>
        <section className="ds-step pg-variable-section">
          <h3 className="ds-step-heading pg-step-heading"><button ref={variablesSummary} type="button" className="ds-step-trigger" disabled={!modelDone} aria-expanded={activeStep === "variables"} aria-controls="variables-panel" onClick={() => { setActiveStep("variables"); setVariablesDone(false); }}>
            Bind application variables
            {variablesDone && <span className="ds-step-state">✓ Ready · Edit</span>}
            {variablesDone && activeStep !== "variables" && <span className="ds-step-summary">{variableSummary}</span>}
          </button></h3>
          <div id="variables-panel" className="ds-step-panel" hidden={activeStep !== "variables"}>
          <details className="ds-help"><summary>About application variables</summary><p>Your application supplies these values in production. They stay fixed for this session and can be used by blueprint {status.blueprint.name}’s access rules.</p></details>
          <div className="pg-setup-row pg-variable-head">
            <span className="pg-vname">Values</span>
            <button type="button" className="pg-link" aria-pressed={form.varJson} onClick={() => onForm(switchVarMode(status, form, !form.varJson))}>
              {form.varJson ? "Edit as fields" : "Edit as JSON"}
            </button>
          </div>
          {!form.varJson && (
            <div className={cls("pg-vargrid", variables.length === 1 && "pg-vargrid-single")}>
              {variables.map((d) => (
                <div className="pg-vfield" key={d.name}>
                  <label className="pg-vlabel">
                    <span className="pg-vname">
                      {d.name}
                      {d.required ? <em className="req">required</em> : <em className="opt">optional</em>}
                    </span>
                    <input
                      value={form.fields[d.name] ?? ""}
                      onChange={(ev) => set({ fields: { ...form.fields, [d.name]: ev.target.value } }, "var:" + d.name)}
                      aria-required={d.required}
                      placeholder={d.example}
                      {...bad(d.name, d.required ? "required-" + d.name : undefined, true)}
                      spellCheck={false}
                    />
                  </label>
                  {d.required && <small id={"required-" + d.name}>Required by blueprint {status.blueprint.name} to start this session.</small>}
                  {why(d.name, true)}
                  {d.example && (
                    <small className="pg-variable-example">
                      <span>Example: <code>{d.example}</code></span>
                      <button type="button" className="pg-link pg-use-example" onClick={() => set({ fields: { ...form.fields, [d.name]: d.example! } }, "var:" + d.name)}>Use example</button>
                    </small>
                  )}
                  {d.description && <details id={"help-" + d.name} className="ds-help pg-variable-help"><summary>About {d.name}</summary><small>{d.description}</small></details>}
                </div>
              ))}
            </div>
          )}
          {form.varJson && (
            <>
              <textarea
                className="pg-json"
                rows={4}
                spellCheck={false}
                aria-label="Session variables as JSON"
                {...bad("json", "json-help", true)}
                aria-invalid={jsonInvalid || undefined}
                value={form.varJsonText}
                onChange={(ev) => set({ varJsonText: ev.target.value }, "var:json")}
              />
              <div id="json-help" className={help.error || jsonMissing ? "pg-json-err" : "pg-muted pg-small"}>
                {jsonHelpText}
              </div>
            </>
          )}
          <button type="button" className="pg-btn pg-primary pg-step-done" aria-label="Done preparing variables" onClick={finishVariables}>Done</button>
          </div>
        </section>
        {modelDone && variablesDone && <div className="pg-setup-row pg-start-row">
          <Gated type="submit" className="pg-btn pg-primary" reasonId="setup-hint" blocked={check.cantStart ? check.hint : null} onBlocked={revealProblems}>
            Start session
          </Gated>
          <span id="setup-hint" className="pg-muted pg-small">
            {check.cantStart ? check.hint : "Ready to chat. Starting sends no message."}
          </span>
        </div>}
      </form>
      {keyFor !== null && (
        <KeyDialog
          status={status}
          provider={keyFor}
          onSave={props.keys.save}
          onDone={(notice, provider, removed) => {
            closeKey();
            props.keys.done(notice);
            // A key just added is for the provider wanted: pick it, with its model once status says.
            if (!removed && provider !== form.provider) {
              fillModel.current = true;
              onForm({ ...form, provider, model: "", effort: keptEffort(provider) });
            }
          }}
          onClose={closeKey}
        />
      )}
    </>
  );
}
