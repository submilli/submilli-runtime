// expect-error: cannot read field `foo` on non-object type `undefined`
// expect-error-count: 1
// The compared value clears the chain's root after the chain ran, so the
// comparison says nothing about what the root holds now.
type Thing = { foo: number };
function take(x: undefined): number {
  return 1;
}
function g(o: Thing | undefined): number {
  if (o?.foo === take((o = undefined))) {
    return o.foo;
  }
  return 0;
}
function main(): void {
  assert(g({ foo: 2 }) === 0);
}
