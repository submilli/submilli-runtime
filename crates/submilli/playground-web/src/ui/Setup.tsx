// The New session form. What it holds stays in the page until the first message is
// sent with it; Start session only checks it the way the playground would.
import { useEffect, useRef, useState, type FormEvent } from "react";

import type { ActionOutcome } from "../api/client";
import type { SavedKey, Status } from "../api/types";
import {
  checkSetup,
  declaredVariables,
  effortHint,
  effortOptions,
  fieldNote,
  modelHint,
  agentNote,
  providerOptions,
  switchVarMode,
  varsHelp,
  type SetupForm,
} from "../model/setup";
import { cls, Gated, Inline } from "./controls";
import { KeyDialog } from "./Dialogs";

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
  copy: (text: string, what: "reference" | "command") => void;
  keys: KeyActions;
}) {
  const { status, form, onForm } = props;
  const [keyFor, setKeyFor] = useState<string | null>(null);
  const keyButton = useRef<HTMLButtonElement>(null);
  const providers = providerOptions(status, form.provider);
  // With no provider picked yet, one with a key is picked, else the first.
  useEffect(() => {
    if (form.provider || !providers.length) return;
    const first = providers.find((o) => !o.needs) ?? providers[0];
    onForm({ ...form, provider: first.provider, model: first.model, effort: first.provider === "openai" ? form.effort : "" });
  }, [providers, form, onForm]);
  const picked = providers.find((o) => o.pressed);
  // Set when a provider is picked by saving its key; its model follows once status names it.
  const fillModel = useRef(false);
  useEffect(() => {
    if (!fillModel.current || !picked?.model) return;
    fillModel.current = false;
    if (!form.model) onForm({ ...form, model: picked.model });
  }, [picked, form, onForm]);
  const closeKey = () => {
    setKeyFor(null);
    keyButton.current?.focus({ preventScroll: true });
  };
  // Open while it holds settings, as when they carry on from the last session; emptying
  // it to retype doesn't close it.
  const holdsSettings = !!form.providerJson.trim();
  const [advanced, setAdvanced] = useState(holdsSettings);
  useEffect(() => {
    if (holdsSettings) setAdvanced(true);
  }, [holdsSettings]);
  const check = checkSetup(status, form);
  const help = varsHelp(check);
  // A field's problem shows once it's been typed in, or once Start session was pressed;
  // a form just opened isn't wrong yet.
  const [typed, setTyped] = useState<Record<string, boolean>>({});
  const [tried, setTried] = useState(false);
  const shown = (field: string) => (tried || typed[field] ? check.invalid[field] : undefined);
  const set = (patch: Partial<SetupForm>, field?: string) => {
    if (field && !typed[field]) setTyped((t) => ({ ...t, [field]: true }));
    onForm({ ...form, ...patch });
  };
  // A field that stops the start says so, and points to the line under it that says why.
  const bad = (field: string, note?: string) => ({
    "aria-invalid": !!shown(field) || undefined,
    "aria-describedby": [note, shown(field) && field !== "json" && "bad-" + field].filter(Boolean).join(" ") || undefined,
  });
  const why = (field: string) =>
    shown(field) ? (
      <small id={"bad-" + field} className="pg-field-err">
        {check.invalid[field]}
      </small>
    ) : null;
  const submit = (ev: FormEvent) => {
    ev.preventDefault();
    if (!check.cantStart) props.onStart();
    else setTried(true);
  };
  const modes: [boolean, string][] = [
    [false, "Fields"],
    [true, "Edit as JSON"],
  ];
  return (
    <>
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
                  onChange={(ev) => set({ fields: { ...form.fields, [d.name]: ev.target.value } }, d.name)}
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
              onChange={(ev) => set({ varJsonText: ev.target.value }, "json")}
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
              {providers.map((o) => (
                <button
                  type="button"
                  key={o.provider}
                  aria-pressed={o.pressed}
                  className={cls("pg-btn", o.pressed && "pg-primary")}
                  title={o.needs ?? undefined}
                  onClick={() => set({ provider: o.provider, model: o.model, effort: o.provider === "openai" ? form.effort : "" })}
                >
                  {o.label}
                  {o.needs && <span className="pg-nokey"> · no key</span>}
                </button>
              ))}
            </div>
            <button
              type="button"
              ref={keyButton}
              className="pg-link pg-keylink"
              onClick={() => setKeyFor(form.provider || providers[0]?.provider || "")}
            >
              {picked && !picked.needs ? "Replace or remove the " + picked.label + " key…" : "Add a key…"}
            </button>
            <small id="agent-help">
              <Inline text={agentNote(status)} />
            </small>
          </div>
          {form.provider !== "" && (
            <>
              <label className="pg-vfield">
                <span className="pg-vname">Model</span>
                <input value={form.model} onChange={(ev) => set({ model: ev.target.value }, "model")} spellCheck={false} {...bad("model")} />
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
                <input value={form.max} onChange={(ev) => set({ max: ev.target.value }, "max")} inputMode="numeric" placeholder="4096" {...bad("max")} />
                {why("max")}
                <small>The most one answer may take.</small>
              </label>
            </>
          )}
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
            onChange={(ev) => set({ providerJson: ev.target.value }, "provider")}
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
          <Gated type="submit" className="pg-btn pg-primary" reasonId="setup-hint" blocked={check.cantStart ? check.hint : null} onBlocked={() => setTried(true)}>
            Start session
          </Gated>
          <span id="setup-hint" className="pg-muted pg-small">
            {check.hint}
          </span>
        </div>
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
              onForm({ ...form, provider, model: "", effort: provider === "openai" ? form.effort : "" });
            }
          }}
          onClose={closeKey}
        />
      )}
    </>
  );
}
