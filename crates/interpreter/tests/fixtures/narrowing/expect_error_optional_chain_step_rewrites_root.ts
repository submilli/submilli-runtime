// expect-error: cannot read field `foo` on `Thing | undefined`
// expect-error-count: 1
// A later step of the chain clears its root, so the chain's result says
// nothing about what the root holds afterwards.
type Thing = { foo: number; bar(x: undefined): number };
function take(x: undefined): undefined {
  return x;
}
function g(o: Thing | undefined): number {
  if (o?.bar(take((o = undefined))) === 1) {
    return o.foo;
  }
  return 0;
}
function main(): void {
  assert(g(undefined) === 0);
}
