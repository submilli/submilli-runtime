// Keeps a scrolling pane at its newest content while the reader is there, and leaves it
// alone once they scroll up to read something earlier. The reader is told they are
// behind only once something new arrives while they're away, so opening a row, which
// scrolls the pane, doesn't read as having missed anything.
import { useCallback, useLayoutEffect, useRef } from "react";

// How close to the end still counts as being at the end, in pixels.
const SLACK = 24;

/**
 * `changed` is what counts as something new; `from` is what the pane shows, such as
 * the session: when it changes, the pane starts over at its end with nothing missed.
 */
export function useFollow<T extends HTMLElement>(changed: unknown, onAway: (away: boolean) => void, from?: unknown, storageKey?: string) {
  const ref = useRef<T>(null);
  const atEnd = useRef(true);
  const missed = useRef(false);
  // Nothing seen yet, so the first content scrolls to its end.
  const last = useRef<unknown>(Symbol());
  const shown = useRef<unknown>(from);

  const restored = useRef<string | undefined>(undefined);

  const onScroll = useCallback(() => {
    const el = ref.current;
    if (!el) return;
    if (storageKey) {
      try { sessionStorage.setItem(storageKey, String(el.scrollTop)); } catch { /* Storage may be disabled. */ }
    }
    const at = el.scrollHeight - el.scrollTop - el.clientHeight <= SLACK;
    if (at === atEnd.current) return;
    atEnd.current = at;
    if (at && missed.current) {
      missed.current = false;
      onAway(false);
    }
  }, [onAway, storageKey]);

  useLayoutEffect(() => {
    if (shown.current !== from) {
      shown.current = from;
      atEnd.current = true;
      if (missed.current) {
        missed.current = false;
        onAway(false);
      }
    }
    const current = ref.current;
    if (storageKey && restored.current !== storageKey && current && current.scrollHeight > current.clientHeight) {
      restored.current = storageKey;
      let saved: string | null = null;
      try { saved = sessionStorage.getItem(storageKey); } catch { /* Storage may be disabled. */ }
      if (saved !== null && Number.isFinite(Number(saved))) {
        current.scrollTop = Number(saved);
        atEnd.current = current.scrollHeight - current.scrollTop - current.clientHeight <= SLACK;
        last.current = changed;
        return;
      }
    }
    if (last.current === changed) return;
    last.current = changed;
    const el = ref.current;
    if (!el) return;
    // A pane whose content fits has no end to be away from.
    if (el.scrollHeight - el.clientHeight <= SLACK) atEnd.current = true;
    if (atEnd.current) el.scrollTop = el.scrollHeight;
    else if (!missed.current) {
      missed.current = true;
      onAway(true);
    }
  });

  const toEnd = useCallback(() => {
    const el = ref.current;
    if (el) el.scrollTop = el.scrollHeight;
    atEnd.current = true;
    missed.current = false;
    onAway(false);
  }, [onAway]);

  return { ref, onScroll, toEnd };
}
