import session from "submilli:session";

/**
 * Reads a session value checked against `T`.
 * @param key The session key.
 * @returns The value, if any.
 */
export const load = <T,>(key: string): T | undefined => session.get<T>(key);

/** Reads session values checked against `T`. */
export class Reader {
  /**
   * Reads a session value checked against `T`.
   * @param key The session key.
   * @returns The value, if any.
   */
  read<T>(key: string): T | undefined {
    return session.get<T>(key);
  }
}
