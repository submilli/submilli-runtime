// The log pane: a row per run that has ended, its permission checks, and the tests and
// reruns of it, each with what can be done next.
import type { ReactNode, RefObject, UIEventHandler } from "react";

import type { Action, AgainOption, DecisionRow, FollowRow, LogPane as Pane, RuleSpot, RunRow } from "../model/log";
import { FILTERS, type Filter } from "../model/ui";
import { Arrow, CancelButton, cls, Gated, press, Values } from "./controls";

/** What the log asks of the page. */
export interface LogActions {
  /** Why test, rerun and draft are off, or `null` when they can be used. */
  blocked: string | null;
  act: (action: Action) => void;
  copy: (text: string, what: "reference" | "command") => void;
  toggle: (id: string) => void;
  toggleChecks: (run: number, open: boolean) => void;
  toggleFollow: (run: number, open: boolean) => void;
  /** Shows the program line a check was asked from. */
  showLine: (id: string, run: number) => void;
  openTried: (run: number) => void;
  inspect: (spot: RuleSpot) => void;
  cancel: (run: number) => void;
  /** Reads a cut result or error whole. */
  showAll: (run: number) => void;
  /** Cuts a result or error shown whole back to its first part. */
  showLess: (run: number) => void;
  /** Opens a test's or rerun's own session at that run, to step through it. */
  stepThrough: (session: string, run: number) => void;
}

/**
 * An error's words wrap; the source lines it quotes, and the carets under them, keep
 * their columns, so a caret stays under its token.
 */
function ErrorText({ text }: { text: string }) {
  return (
    <pre className="pg-error">
      {text.split("\n").map((line, i, all) => (
        <span key={i} className={SOURCE_LINE.test(line) ? "pg-src" : undefined}>
          {line}
          {i < all.length - 1 ? "\n" : ""}
        </span>
      ))}
    </pre>
  );
}
const SOURCE_LINE = /^\s*\d*\s*\|/;

/** A meta line, its sizes kept with their units when it wraps: "385 B", "21.5 kB". */
function keepUnits(text: string): string {
  return text.replace(/(\d) (k?B)\b/g, "$1\u00a0$2");
}

// One button that changes its words, so focus stays on it between Show all and Show less.
function Cut(props: { cut: string; less: boolean; run: number; a: LogActions }) {
  if (!props.cut && !props.less) return null;
  return (
    <p className="pg-muted pg-small">
      {props.less ? null : keepUnits(props.cut) + " "}
      <button className="pg-link pg-inline" onClick={() => (props.less ? props.a.showLess(props.run) : props.a.showAll(props.run))}>
        {props.less ? "Show less" : "Show all"}
      </button>
    </p>
  );
}

function Again(props: { options: AgainOption[]; a: LogActions }) {
  return (
    <>
      {props.options.map((o) => (
        <div className="pg-again-opt" key={o.label}>
          <p>
            <strong>{o.goal}</strong> {o.what}
          </p>
          <div className="pg-again-buttons">
            <Gated className={cls("pg-btn", o.caution && "pg-caution")} blocked={props.a.blocked} onClick={() => props.a.act(o.action)}>
              {o.label}
            </Gated>
            <button className="pg-copycmd" title={o.copy.title} onClick={() => props.a.copy(o.copy.text, o.copy.what)}>
              <span aria-hidden="true">⧉</span> {o.copy.label}
            </button>
          </div>
          <small>{o.note}</small>
        </div>
      ))}
    </>
  );
}

function Head(props: { open: boolean; onToggle: () => void; children: ReactNode; status: string; tone: string; label?: string }) {
  const toggle = press(props.onToggle);
  return (
    <div className="pg-event-head" role="button" tabIndex={0} aria-expanded={props.open} aria-label={props.label} onClick={toggle} onKeyDown={toggle}>
      <span className="pg-event-main">{props.children}</span>
      <span className={"pg-status " + props.tone}>{props.status}</span>
    </div>
  );
}

function Check({ d, run, a }: { d: DecisionRow; run: number; a: LogActions }) {
  return (
    <div className={cls("pg-event nested", d.selected && "selected")} id={"pgrow-" + d.id}>
      <Head open={d.open} onToggle={() => a.toggle(d.id)} status={d.status} tone={d.tone}>
        <span className="pg-event-title" title={d.hint ? "Asked for " + d.hint : undefined}>
          <Arrow />
          Check <span className="pg-refnum">{d.ref}</span>
          {d.title}
        </span>
        {d.open && <span className="pg-event-meta">{keepUnits(d.meta)}</span>}
      </Head>
      {d.open && (
        <div className="pg-event-detail">
          <div className="pg-rule">
            <strong>{d.decidedBy}</strong>
            {d.near.map((n, i) => (
              <div key={i}>
                <p>{n.text}</p>
                {n.values && (
                  <table className="pg-kv pg-kv-tight">
                    <tbody>
                      {n.values.map((x) => (
                        <tr key={x.name}>
                          <td>{x.name}</td>
                          <td className="val">{x.value}</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                )}
              </div>
            ))}
          </div>
          {d.context && (
            <div className="pg-ctx">
              <span>Asked with</span> <code>{d.context}</code>
            </div>
          )}
          <div className="pg-actions">
            <button className="pg-link" title={d.refTitle} onClick={() => a.copy(d.copyText, "reference")}>
              Copy for assistant
            </button>
            {d.inspect && (
              <button className="pg-link" onClick={() => d.inspect && a.inspect(d.inspect)}>
                {d.inspectLabel}
              </button>
            )}
            {d.line !== null && (
              <button className="pg-link" onClick={() => a.showLine(d.id, run)}>
                Show line {d.line}
              </button>
            )}
          </div>
          {d.next.length > 0 && (
            <div className="pg-next">
              <span>Next, from the CLI</span>
              {d.next.map((c) => (
                <button className="pg-cmd" title="Copy this command" key={c} onClick={() => a.copy(c, "command")}>
                  {c}
                </button>
              ))}
            </div>
          )}
        </div>
      )}
    </div>
  );
}

function FollowUp({ f, a }: { f: FollowRow; a: LogActions }) {
  return (
    <div className={cls("pg-event nested", f.selected && "selected")} id={"pgrow-" + f.id}>
      <Head open={f.open} onToggle={() => a.toggle(f.id)} status={f.status} tone={f.tone} label={"Run " + f.num + f.title + ": " + f.status}>
        <span className="pg-event-title">
          <Arrow />
          <span className="pg-refnum">{f.num}</span>
          {f.title}
        </span>
        <span className="pg-event-meta">{keepUnits(f.meta)}</span>
      </Head>
      {f.open && (
        <div className="pg-event-detail">
          {f.verdict && <p className="pg-verdict">{f.verdict}</p>}
          {f.values && <Values v={f.values} />}
          {f.showFurther && <p className="pg-note">{f.further}</p>}
          <p className="pg-note pg-muted">{f.what}</p>
          {f.changes.map((c, i) => (
            <p className={cls("pg-change-line", c.tone)} key={i}>
              {c.text}
            </p>
          ))}
          {f.isStop && (
            <div className="pg-stop">
              <p>{f.stopLead}</p>
              <code className="pg-stopkey">{f.stopKey}</code>
              <p className="pg-stop-q">To see the rest of the program run:</p>
              <div className="pg-stop-opts">
                <div className="pg-stop-opt">
                  <div>
                    <Gated className="pg-btn" blocked={a.blocked} onClick={() => a.act(f.tryReads)}>
                      Test again with real reads
                    </Gated>
                  </div>
                  <p>This call only reads data, so it can go to the real service. Anything that would change data still stops the test.</p>
                </div>
                <div className="pg-stop-opt">
                  <div>
                    <Gated className="pg-btn pg-caution" blocked={a.blocked} onClick={() => a.act(f.tryLive)}>
                      Test it live…
                    </Gated>
                  </div>
                  <p>Every call goes out, including ones that change data. Asks first.</p>
                </div>
              </div>
              {f.tried !== null && (
                <p className="pg-muted pg-small">
                  Already tried:{" "}
                  <button className="pg-link pg-inline" onClick={() => f.tried !== null && a.openTried(f.tried)}>
                    {f.triedLabel}
                  </button>
                </p>
              )}
            </div>
          )}
          {f.wentLive && (
            <div className="pg-ctx">
              <span>Went live</span> <code>{f.wentLive}</code>
            </div>
          )}
          {f.error && <ErrorText text={f.error} />}
          {f.result && <pre className="pg-payload">{f.result}</pre>}
          <Cut cut={f.cut} less={f.less} run={f.n} a={a} />
          {f.again.length > 0 && (
            <div className="pg-again">
              <h3>{f.againHead}</h3>
              <Again options={f.again} a={a} />
            </div>
          )}
          <div className="pg-actions">
            {f.session && (
              <button className="pg-btn" onClick={() => f.session && a.stepThrough(f.session, f.n)}>
                {f.stepLabel}
              </button>
            )}
            <button className="pg-link" title={f.refTitle} onClick={() => a.copy(f.copyText, "reference")}>
              Copy for assistant
            </button>
            <button className="pg-link" title={f.compareCmd} onClick={() => a.copy(f.compareCmd, "command")}>
              Copy compare command
            </button>
          </div>
        </div>
      )}
    </div>
  );
}

function Run({ r, a }: { r: RunRow; a: LogActions }) {
  const showChecks = r.open && r.decisions.length > 0;
  const showFollow = r.open && r.follow.length > 0;
  const checks = press(() => a.toggleChecks(r.n, !r.checksOpen));
  const follow = press(() => a.toggleFollow(r.n, !r.followOpen));
  return (
    <article className={cls("pg-event", r.selected && "selected")} id={"pgrow-" + r.id}>
      <Head open={r.open} onToggle={() => a.toggle(r.id)} status={r.status} tone={r.tone}>
        <span className="pg-event-title" title={r.hint ? "Asked for " + r.hint : undefined}>
          <Arrow />
          {r.title}
        </span>
        {r.open && <span className="pg-event-meta">{keepUnits(r.meta)}</span>}
        {!r.open && r.closedSum && <span className="pg-event-meta">{r.closedSum}</span>}
      </Head>
      {r.preview && <div className="pg-error-preview">{r.preview}</div>}
      {r.open && (
        <div className="pg-event-detail">
          {r.error && <ErrorText text={r.error} />}
          {r.result && <pre className="pg-payload">{r.result}</pre>}
          <Cut cut={r.cut} less={r.less} run={r.n} a={a} />
          <div className="pg-actions">
            <button className="pg-link" title={r.refTitle} onClick={() => a.copy(r.copyText, "reference")}>
              Copy for assistant
            </button>
          </div>
          {r.again.length > 0 && (
            <div className="pg-again">
              <h3>{r.thing === "call" ? "If this call should be allowed" : "Try this program again"}</h3>
              <Again options={r.again} a={a} />
              {r.thing === "program" && <p className="pg-group-note pg-again-foot">Either shows up under this run, in Reruns by you.</p>}
            </div>
          )}
          {r.againNote && <p className="pg-group-note pg-again-note">{r.againNote}</p>}
        </div>
      )}
      {showChecks && (
        <div className="pg-follow pg-checks">
          <div className="pg-follow-head" role="button" tabIndex={0} aria-expanded={r.checksOpen} onClick={checks} onKeyDown={checks}>
            <Arrow />
            {r.checksLabel}
            <span className="pg-follow-sum">{r.checksSum}</span>
          </div>
          {r.checksOpen && (
            <>
              <p className="pg-group-note">What the blueprint decided each time this {r.thing} asked to do something.</p>
              {r.decisions.map((d) => (
                <Check d={d} run={r.n} a={a} key={d.id} />
              ))}
            </>
          )}
        </div>
      )}
      {showFollow && (
        <div className="pg-follow">
          <div className="pg-follow-head" role="button" tabIndex={0} aria-expanded={r.followOpen} onClick={follow} onKeyDown={follow}>
            <Arrow />
            {r.followLabel}
            <span className="pg-follow-sum">{r.followSum}</span>
          </div>
          {r.followOpen && (
            <>
              <p className="pg-group-note">
                Runs of this {r.thing} that you or your coding assistant started later, each in a throwaway session. The agent never saw them.
              </p>
              {r.follow.map((f) => (
                <FollowUp f={f} a={a} key={f.id} />
              ))}
            </>
          )}
        </div>
      )}
    </article>
  );
}

export function LogPane(props: {
  log: Pane;
  filter: Filter;
  /** One line per run whose events weren't all recorded. */
  gaps: string[];
  /** The run in the running note, when it is still in flight and can be cancelled. */
  cancelRun: number | null;
  cancelling: boolean;
  scrollRef: RefObject<HTMLDivElement>;
  onScroll: UIEventHandler;
  onFilter: (filter: Filter) => void;
  onToggleAll: () => void;
  a: LogActions;
}) {
  const { log, a } = props;
  return (
    <section className="pg-pane log" aria-label="Log">
      <div className="pg-pane-head">
        <h2>
          Log<span className="sub">{log.count}</span>
        </h2>
        <div className="pg-pane-tools">
          <button className="pg-link" onClick={props.onToggleAll}>
            {log.allLabel}
          </button>
          <div className="pg-pills" role="group" aria-label="Filter">
            {FILTERS.map((f) => (
              <button className={cls("pg-pill", props.filter === f.filter && "on")} aria-pressed={props.filter === f.filter} key={f.filter} onClick={() => props.onFilter(f.filter)}>
                {f.name}
              </button>
            ))}
          </div>
        </div>
      </div>
      <div className="pg-log" ref={props.scrollRef} onScroll={props.onScroll}>
        {props.gaps.map((g, i) => (
          <p className="pg-group-note pg-gap" key={i}>
            {g}
          </p>
        ))}
        {log.rows.map((r) => (
          <Run r={r} a={a} key={r.id} />
        ))}
        {/* After the runs that ended, where a reader following the log is. */}
        {log.runningNote && (
          <div className="pg-empty pg-running">
            {log.runningNote}
            {props.cancelRun !== null && (
              <>
                {" "}
                <CancelButton run={props.cancelRun} cancelling={props.cancelling} onCancel={a.cancel} />
              </>
            )}
          </div>
        )}
        {log.empty !== null && <div className="pg-empty">{log.empty}</div>}
      </div>
    </section>
  );
}
