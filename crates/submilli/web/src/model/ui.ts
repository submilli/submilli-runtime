// What the page keeps about how the reader is looking at the record. The wording reads it;
// the components change it.
import type { ChatSettings } from "../api/types";

/** Viewing a session, filling in a new one, or a new one started and waiting for its first message. */
export type Mode = "open" | "setup" | "fresh";

export type Filter = "all" | "problems" | "follow";

export const FILTERS: { filter: Filter; name: string }[] = [
  { filter: "all", name: "All" },
  { filter: "problems", name: "Errors and denials" },
  { filter: "follow", name: "With reruns" },
];

/** `confirm` asks before a live test or rerun; `draft` shows a drafted rule. */
export type Dialog = null | "sessions" | "bp" | "pkgs" | "allvars" | "confirm" | "draft";

/** A new session as the page holds it until the first message is sent with it. */
export interface Fresh {
  variables: Record<string, string>;
  /** `<provider>:<model>`. */
  agent: string;
  settings: ChatSettings | null;
  /** When Start session was pressed, as an ISO time. */
  started: string;
  /** What the developer sent, until the session's own feed shows it. */
  messages: string[];
}

export interface UiState {
  mode: Mode;
  /** The step the reader is on, by key; `AT_START` before the first; `null` at the latest. */
  cursor: string | null;
  playing: boolean;
  /** The picked log row: `r<run>`, `d<run>.<n>` or `f<run>` (a follow-up). */
  sel: string | null;
  /** Which log rows are open, by the same ids as `sel`. */
  open: Record<string, boolean>;
  /** Whether a run's follow-up list is open, by run. */
  followOpen: Record<number, boolean>;
  /** Whether a run's checks list is open, by run; unset means open when a check was denied. */
  checksOpen: Record<number, boolean>;
  filter: Filter;
  /** A run whose program the reader picked to show. */
  program: number | null;
  dlg: Dialog;
  more: boolean;
  focus: boolean;
  /** The blueprint version the dialog shows; `null` for the one in force. */
  bpV: number | null;
  /** The line the blueprint dialog highlights, and what it is. */
  bpFocus: { line: number; label: string } | null;
  fresh: Fresh | null;
}

export const INITIAL_UI: UiState = {
  mode: "open",
  cursor: null,
  playing: false,
  sel: null,
  open: {},
  followOpen: {},
  checksOpen: {},
  filter: "all",
  program: null,
  dlg: null,
  more: false,
  focus: false,
  bpV: null,
  bpFocus: null,
  fresh: null,
};
