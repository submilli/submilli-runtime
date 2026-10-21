// expect-error: cannot read field `foo` on `Thing | undefined`
// expect-error-count: 3
// Equal to a value that may be `undefined`, or of any `typeof` that may be
// `"undefined"`, the chain may have short-circuited.
type Thing = { foo: number };
function f(o: Thing | undefined, v: number | undefined, u: unknown): number {
  if (o?.foo === v) return o.foo;
  if (o?.foo === u) return o.foo;
  if (typeof o?.foo !== "number") return o.foo;
  return 0;
}
function main(): void {
  assert(f(undefined, undefined, 1) === 0);
}
