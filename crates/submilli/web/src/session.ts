/** Where the page keeps its browser session token between loads. */
export const SESSION_KEY = "submilli-playground-session";

/** The login code a `submilli playground open` link carries in `#login=<code>`. */
export function loginCodeFrom(hash: string): string | null {
  return new URLSearchParams(hash.replace(/^#/, "")).get("login");
}

/**
 * What the page shows: the status, or the sentence that tells the developer how to
 * sign in again. The link's login code is traded for a session token first, and is
 * taken out of the address bar so it is not kept in history.
 */
export async function loadStatus(): Promise<string> {
  const code = loginCodeFrom(location.hash);
  history.replaceState(null, "", location.pathname + location.search);
  if (code) {
    const login = await fetch("/api/login", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ code }),
    });
    if (login.ok) {
      const { session_token } = (await login.json()) as { session_token: string };
      localStorage.setItem(SESSION_KEY, session_token);
    }
  }
  const token = localStorage.getItem(SESSION_KEY);
  const status = await fetch("/api/status", {
    headers: token ? { authorization: `Bearer ${token}` } : {},
  });
  if (status.status === 401) {
    localStorage.removeItem(SESSION_KEY);
    return "Run `submilli playground open` for a new link.";
  }
  return JSON.stringify(await status.json(), null, 2);
}
