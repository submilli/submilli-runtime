// The New session form. What it holds stays in the page until the first message is
// sent with it; Start session only checks it the way the playground would.
import { useState, type FormEvent } from "react";

import type { Status } from "../api/types";
import {
  checkSetup,
  declaredVariables,
  effortHint,
  effortOptions,
  fieldNote,
  modelHint,
  providerOptions,
  switchVarMode,
  varsHelp,
  type SetupForm,
} from "../model/setup";
import { cls, Gated } from "./controls";

export function Setup(props: {
  status: Status;
  form: SetupForm;
  onForm: (form: SetupForm) => void;
  onStart: () => void;
  copy: (text: string, what: "reference" | "command") => void;
}) {
  const { status, form, onForm } = props;
  // Open while it holds settings; emptying it to retype doesn't close it.
  const [advanced, setAdvanced] = useState(!!form.providerJson.trim());
  const check = checkSetup(status, form);
  const help = varsHelp(check);
  const set = (patch: Partial<SetupForm>) => onForm({ ...form, ...patch });
  // A field that stops the start says so, and points to the line under it that says why.
  const bad = (field: string, note?: string) => ({
    "aria-invalid": !!check.invalid[field] || undefined,
    "aria-describedby": [note, check.invalid[field] && field !== "json" && "bad-" + field].filter(Boolean).join(" ") || undefined,
  });
  const why = (field: string) =>
    check.invalid[field] ? (
      <small id={"bad-" + field} className="pg-field-err">
        {check.invalid[field]}
      </small>
    ) : null;
  const submit = (ev: FormEvent) => {
    ev.preventDefault();
    if (!check.cantStart) props.onStart();
  };
  const modes: [boolean, string][] = [
    [false, "Fields"],
    [true, "Edit as JSON"],
  ];
  return (
    <form className="pg-setup" onSubmit={submit}>
      <div className="pg-setup-row">
        <span className="pg-vname">Variables</span>
        <div className="pg-pills" role="group" aria-label="Enter values as">
          {modes.map(([json, name]) => (
            <button
              type="button"
              key={name}
              className={cls("pg-pill", form.varJson === json && "on")}
              aria-pressed={form.varJson === json}
              onClick={() => onForm(switchVarMode(status, form, json))}
            >
              {name}
            </button>
          ))}
        </div>
      </div>
      {!form.varJson && (
        <div className="pg-vargrid">
          {declaredVariables(status).map((d, i) => (
            <label className="pg-vfield" key={d.name}>
              <span className="pg-vname">
                {d.name}
                {d.required ? <em className="req">required</em> : <em className="opt">optional</em>}
              </span>
              <input
                value={form.fields[d.name] ?? ""}
                onChange={(ev) => set({ fields: { ...form.fields, [d.name]: ev.target.value } })}
                aria-required={d.required}
                {...bad(d.name, "vars-note")}
                autoFocus={i === 0}
                spellCheck={false}
              />
              {why(d.name)}
            </label>
          ))}
        </div>
      )}
      {!form.varJson && (
        <small id="vars-note" className="pg-muted pg-small">
          {fieldNote(status)}
        </small>
      )}
      {form.varJson && (
        <>
          <textarea
            className="pg-json"
            rows={4}
            spellCheck={false}
            aria-label="Session variables as JSON"
            {...bad("json", "json-help")}
            value={form.varJsonText}
            onChange={(ev) => set({ varJsonText: ev.target.value })}
          />
          <div id="json-help" className={help.error ? "pg-json-err" : "pg-muted pg-small"}>
            {help.text}
          </div>
        </>
      )}
      <div className="pg-agentgrid">
        <div className="pg-vfield" role="group" aria-label="Provider">
          <span className="pg-vname">Agent</span>
          <div className="pg-choices">
            {providerOptions(status, form.provider).map((o) => (
              <button
                type="button"
                key={o.provider}
                className={cls("pg-btn", o.pressed && "pg-primary")}
                aria-pressed={o.pressed}
                onClick={() => set({ provider: o.provider, model: o.model, effort: o.provider === "anthropic" ? "" : form.effort })}
              >
                {o.label}
              </button>
            ))}
          </div>
          <small>One per model key the playground started with. Keys stay with the playground; this page never sees them.</small>
        </div>
        <label className="pg-vfield">
          <span className="pg-vname">Model</span>
          <input value={form.model} onChange={(ev) => set({ model: ev.target.value })} spellCheck={false} {...bad("model")} />
          {why("model")}
          <small>{modelHint(status, form.provider)}</small>
        </label>
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
        <label className="pg-vfield">
          <span className="pg-vname">
            Max output<em className="opt">tokens, optional</em>
          </span>
          <input value={form.max} onChange={(ev) => set({ max: ev.target.value })} inputMode="numeric" placeholder="4096" {...bad("max")} />
          {why("max")}
          <small>The most one answer may take.</small>
        </label>
      </div>
      <details className="pg-advanced" open={advanced} onToggle={(ev) => setAdvanced(ev.currentTarget.open)}>
        <summary>Advanced: provider settings (JSON)</summary>
        <textarea
          className="pg-json"
          rows={4}
          spellCheck={false}
          aria-label="Provider settings as JSON"
          {...bad("provider")}
          placeholder='{"temperature": 0.2}'
          value={form.providerJson}
          onChange={(ev) => set({ providerJson: ev.target.value })}
        />
        {why("provider")}
        <small>
          Fields added to every request, in the provider’s own API terms. Fields the playground sets itself (the model, the
          conversation, the tools, the instructions) can’t be set here.
        </small>
      </details>
      <div className="pg-setupcli">
        <span>Same from the CLI</span>
        {/* The CLI's chat takes the bound variables, so ones set here are bound first. */}
        {check.cliBind && (
          <>
            <button type="button" className="pg-cmd" title="Copy this command" onClick={() => props.copy(check.cliBind, "command")}>
              {check.cliBind}
            </button>
            <span>(the binding stays for later runs, and the next start)</span>
          </>
        )}
        {check.cli ? (
          <button type="button" className="pg-cmd" title="Copy this command" onClick={() => props.copy(check.cli, "command")}>
            {check.cli}
          </button>
        ) : (
          <span>shows once the form can start a session.</span>
        )}
      </div>
      <div className="pg-setup-row">
        <Gated type="submit" className="pg-btn pg-primary" reasonId="setup-hint" blocked={check.cantStart ? check.hint : null}>
          Start session
        </Gated>
        <span id="setup-hint" className="pg-muted pg-small">
          {check.hint}
        </span>
      </div>
    </form>
  );
}
