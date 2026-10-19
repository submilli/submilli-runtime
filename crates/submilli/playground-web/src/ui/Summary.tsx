// What the session fetched and returned up to the cursor, and its counts.
import type { Summary as Counts } from "../model/summary";
import type { StepBar as Bar } from "../model/steps";

export function Summary({ s }: { s: Counts }) {
  return (
    <section className="pg-summary" aria-label="Session summary">
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
        <div className="pg-data-note">{s.dataNote}</div>
      </div>
      <div className="pg-facts">
        <div className="pg-fact">
          <span>Runs</span>
          <strong>{s.execHead}</strong>
          <small>{s.execSub}</small>
        </div>
        <div className="pg-fact">
          <span>Blueprint checks</span>
          <strong>{s.checksHead}</strong>
          <small className={s.checksBad ? "bad" : undefined}>{s.checksSub}</small>
        </div>
        <div className="pg-fact">
          <span>Reruns by you</span>
          <strong>{s.followHead}</strong>
          <small>{s.followSub}</small>
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
    <div className="pg-step" role="group" aria-label="Step through">
      <button className="pg-btn" {...off(bar.atStart)} onClick={guard(bar.atStart, props.onStart)}>
        From the start
      </button>
      <button className="pg-btn" aria-label="Previous event" {...off(bar.atStart)} onClick={guard(bar.atStart, props.onPrev)}>
        ←
      </button>
      <button className="pg-btn pg-play" onClick={props.onPlay}>
        {props.playing ? "Pause" : "Play"}
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
      <span className="pg-step-help">Replay this session with ← →: each message, program, and call it makes. Nothing runs again.</span>
    </div>
  );
}
