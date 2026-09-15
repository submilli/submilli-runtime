// expect-error: depth must be a non-negative integer literal
// Not a test262 port: pins a documented divergence (spec.md §1.2). flat's
// depth must be a non-negative integer literal so the result type is
// computable; a variable depth is a compile error. (flat(Infinity) is pinned
// by the ported prototype/flat/positive-infinity case.)

function main(): void {
  const nested = [[1], [2]];
  const depth: number = 1;
  const flattened = nested.flat(depth);
  assertSameValue(flattened.length, 2, "flattened.length");
}
