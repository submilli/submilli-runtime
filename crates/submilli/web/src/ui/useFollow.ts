// Keeps a scrolling pane at its newest content while the reader is there, and leaves it
// alone once they scroll up to read something earlier.
import { useCallback, useLayoutEffect, useRef } from "react";

// How close to the end still counts as being at the end, in pixels.
const SLACK = 24;

export function useFollow<T extends HTMLElement>(changed: unknown, onAway: (away: boolean) => void) {
  const ref = useRef<T>(null);
  const atEnd = useRef(true);

  const onScroll = useCallback(() => {
    const el = ref.current;
    if (!el) return;
    const at = el.scrollHeight - el.scrollTop - el.clientHeight <= SLACK;
    if (at !== atEnd.current) {
      atEnd.current = at;
      onAway(!at);
    }
  }, [onAway]);

  useLayoutEffect(() => {
    const el = ref.current;
    if (el && atEnd.current) el.scrollTop = el.scrollHeight;
  }, [changed]);

  const toEnd = useCallback(() => {
    const el = ref.current;
    if (el) el.scrollTop = el.scrollHeight;
    atEnd.current = true;
    onAway(false);
  }, [onAway]);

  return { ref, onScroll, toEnd };
}
