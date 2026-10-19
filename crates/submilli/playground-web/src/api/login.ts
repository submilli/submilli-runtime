// The page's links: `submilli playground open` gives `#login=<code>`, and a run's page
// link is `#run=<n>` or `#run=<n>&decision=<m>`.

import { SESSION_KEY } from "./client";

/** A run the link asked to open, and the decision in it, if one was named. */
export interface RequestedRun {
  run: number;
  decision: number | null;
}

/** What the signed-out screen tells the developer to do. */
export const SIGNED_OUT_LINE = "Run `submilli playground open` for a new link.";

/** The same, for a link to a run: the command whose link opens that run. */
export function signedOutLine(requested: RequestedRun | null): string {
  if (!requested) return SIGNED_OUT_LINE;
  const at = requested.decision === null ? String(requested.run) : requested.run + "." + requested.decision;
  return "Run `submilli playground open " + at + "` for a new link to it.";
}

export function readLink(hash: string): { code: string | null; requested: RequestedRun | null } {
  const params = new URLSearchParams(hash.replace(/^#/, ""));
  const run = whole(params.get("run"));
  return {
    code: params.get("login"),
    requested: run === null ? null : { run, decision: whole(params.get("decision")) },
  };
}

/** The hash that keeps a requested run in the address bar, without any login code. */
export function runHash(requested: RequestedRun | null): string {
  if (!requested) return "";
  const decision = requested.decision === null ? "" : `&decision=${requested.decision}`;
  return `#run=${requested.run}${decision}`;
}

/**
 * Trades a login code for a session token and stores it. An expired or already used
 * code is refused; the caller shows the signed-out screen.
 */
export async function exchangeCode(code: string, fetchFn: typeof fetch = globalThis.fetch): Promise<boolean> {
  try {
    const response = await fetchFn("/api/login", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ code }),
    });
    if (!response.ok) return false;
    const { session_token } = (await response.json()) as { session_token?: unknown };
    if (typeof session_token !== "string") return false;
    localStorage.setItem(SESSION_KEY, session_token);
    return true;
  } catch {
    return false;
  }
}

function whole(text: string | null): number | null {
  return text !== null && /^\d+$/.test(text) ? Number(text) : null;
}
