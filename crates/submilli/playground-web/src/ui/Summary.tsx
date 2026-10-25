// What the session fetched and returned up to the cursor, and its counts.
import type { Summary as Counts } from "../model/summary";
import type { StepBar as Bar } from "../model/steps";

export function Summary({ s, open, onToggle }: { s: Counts; open: boolean; onToggle: () => void }) {
  return (
    <section className="pg-summary" aria-label="Session summary">
      <button
        className="pg-icon pg-summary-toggle"
        aria-label={open ? "Hide summary" : "Show summary"}
        title={open ? "Hide summary" : "Show summary"}
        aria-expanded={open}
        aria-controls="pg-summary-content"
        onClick={onToggle}
      >
        <svg width="12" height="12" viewBox="0 0 10 10" aria-hidden="true">
          <path d={open ? "M2 6l3-3 3 3" : "M2 3l3 3 3-3"} fill="none" stroke="currentColor" strokeWidth="1.5" />
        </svg>
      </button>
      <span className="pg-summary-scope">Session totals{open ? " · through the displayed event" : ` · ${s.execHead} · ${s.checksHead}${s.checksSub ? ` · ${s.checksSub}` : ""}`}</span>
      <div className="pg-summary-content" id="pg-summary-content" hidden={!open}>
          <div className="pg-summary-data">
            <div className="pg-data-row">
              <div className="pg-data">
                <strong>{s.fetched}</strong>
                <span>Fetched</span>
              </div>
              <span className="pg-flow" aria-hidden="true">
                →
              </span>
              <div className="pg-data agent">
                <strong>{s.returned}</strong>
                <span>Returned to agent</span>
              </div>
            </div>
            {s.dataNote && <div className="pg-data-note">{s.dataNote}</div>}
          </div>
          <div className="pg-facts">
            <div className="pg-fact">
              <span>Runs</span>
              <strong>{s.execHead}</strong>
              {s.execSub && <small>{s.execSub}</small>}
            </div>
            <div className="pg-fact">
              <span>Blueprint checks</span>
              <strong>{s.checksHead}</strong>
              {s.checksSub && <small className={s.checksBad ? "bad" : undefined}>{s.checksSub}</small>}
            </div>
            <div className="pg-fact">
              <span>Reruns by you</span>
              <strong>{s.followHead}</strong>
              {s.followSub && <small>{s.followSub}</small>}
            </div>
          </div>
      </div>
    </section>
  );
}

export function StepBar(props: {
  bar: Bar;
  playing: boolean;
  /** Stepped back, or scrolled up in a pane: the reader isn't at the latest. */
  behind: boolean;
  onStart: () => void;
  onPrev: () => void;
  onPlay: () => void;
  onNext: () => void;
  onLatest: () => void;
}) {
  const { bar } = props;
  // These stay focusable at either end, so stepping with the keyboard never drops focus.
  const off = (yes: boolean) => (yes ? { "aria-disabled": true as const } : {});
  const guard = (yes: boolean, fn: () => void) => () => {
    if (!yes) fn();
  };
  return (
    <div className="pg-step" role="group" aria-label="Recorded playback">
      <button className="pg-btn" {...off(bar.atStart)} onClick={guard(bar.atStart, props.onStart)}>
        From the start
      </button>
      <button className="pg-btn" aria-label="Previous event" {...off(bar.atStart)} onClick={guard(bar.atStart, props.onPrev)}>
        ←
      </button>
      <button className="pg-btn pg-play" onClick={props.onPlay}>
        {props.playing ? "Pause" : bar.atEnd ? "Replay recording" : "Play recording"}
      </button>
      <button className="pg-btn" aria-label="Next event" {...off(bar.atEnd)} onClick={guard(bar.atEnd, props.onNext)}>
        →
      </button>
      <span className="pg-pos">{bar.position}</span>
      {props.behind && (
        <button className="pg-link" onClick={props.onLatest}>
          Back to latest
        </button>
      )}
      <span className="sr-only" aria-live="polite">
        {bar.announce}
      </span>
      <span className="pg-step-help">Recorded events only · no calls sent</span>
    </div>
  );
}
