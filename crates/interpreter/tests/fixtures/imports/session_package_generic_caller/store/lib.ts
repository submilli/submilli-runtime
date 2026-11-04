import session from "submilli:session";

/** Stores a value on the caller's behalf. */
export function save(key: string, value: unknown): void {
  session.set(key, value);
}

/** Reads a value back as whatever type the caller names. */
export function load<T>(key: string): T | undefined {
  return session.get<T>(key);
}
