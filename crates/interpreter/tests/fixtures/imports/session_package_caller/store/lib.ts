import session from "submilli:session";

/**
 * Records progress on the caller's behalf.
 * @capability session.write { op: "set", key }
 */
export function record(key: string, step: number): void {
  session.set(key, { step });
}
