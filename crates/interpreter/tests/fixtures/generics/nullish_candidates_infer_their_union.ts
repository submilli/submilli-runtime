// Candidates that are only `null` and `undefined` infer their union, as tsc
// does.
function pick<T>(a: T, b: T, first: boolean): T {
  return first ? a : b;
}
function main(): void {
  const either: null | undefined = pick(undefined, null, false);
  assert(either === null && pick(undefined, null, true) === undefined, "null | undefined");
}
