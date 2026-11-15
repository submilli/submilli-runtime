import session from "submilli:session";

/**
 * A function that returns its argument, for any type.
 * @param value The argument.
 * @returns The argument.
 */
export type Identity = <T>(value: T) => T;

/**
 * Returns its argument.
 * @param value The argument.
 * @returns The argument.
 */
export const identity: Identity = <T,>(value: T): T => value;

/**
 * Reads a session value checked against `T`.
 * @param key The session key.
 * @returns The value, if any.
 */
export function load<T>(key: string): T | undefined {
  return session.get<T>(key);
}

/** Reads typed values. */
export interface Store {
  /**
   * Reads a session value checked against `T`.
   * @param key The session key.
   * @returns The value, if any.
   */
  read<T>(key: string): T | undefined;
}

/** The session store. */
export const store: Store = {
  read<T>(key: string): T | undefined {
    return session.get<T>(key);
  },
};

/** Wraps values in arrays. */
export class Wrapper {
  /**
   * Wraps `value` in an array.
   * @param value The value.
   * @returns An array holding it.
   */
  wrap<T>(value: T): T[] {
    return [value];
  }
}

/** Logs values of any type. */
export class Logger {
  /**
   * Logs `value`.
   * @param value The value.
   * @returns How many values were logged.
   */
  log<T>(value: T): number {
    return 1;
  }
}

/**
 * Logs through `logger`.
 * @param logger The logger.
 * @returns What it returned.
 */
export function logWith(logger: Logger): number {
  return logger.log<string>("a");
}
