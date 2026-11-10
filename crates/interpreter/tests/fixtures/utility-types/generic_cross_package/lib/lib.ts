/** A collection whose items are patched in place. */
export interface Store<T> {
  /** The stored items. */
  items: T[];
  /** Apply `change` to the item at `index`. */
  patch(index: number, change: Partial<T>): T;
}
/** `value` seen through the single property `key`. */
export function pick<T, K extends keyof T>(value: T, key: K): Pick<T, K> { return value; }
/** The value of property `key` of `value`. */
export function read<T, K extends keyof T>(value: T, key: K): T[K] { return value[key]; }
/** `fallback`, typed as what `f` returns. */
export function call<F extends (...args: never[]) => unknown>(f: F, fallback: ReturnType<F>): ReturnType<F> { return fallback; }
/** `value` unless it is null or undefined, else `fallback`. */
export function present<T>(value: T, fallback: NonNullable<T>): NonNullable<T> {
  if (value === null || value === undefined) { return fallback; }
  return value;
}
/** The one key `first` of `value`, as a list. */
export function names<T extends object>(value: T, first: keyof T): (keyof T)[] { return [first]; }
