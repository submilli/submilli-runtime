import { guarded } from "@test/inner";

/** Runs a caller-supplied closure inside this package. */
export function runCallback(fn: (n: number) => void): void {
  fn(1);
}

/** This package's own call, for contrast with the callback route. */
export function relay(foo: number): void {
  guarded(foo);
}
