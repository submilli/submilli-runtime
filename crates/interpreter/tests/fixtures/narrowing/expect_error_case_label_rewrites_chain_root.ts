// expect-error: cannot read field `foo`
// expect-error-count: 1
// A label of an earlier clause that clears the root of an optional chain
// leaves a later clause's match saying nothing about the root.
type Thing = { foo: number };
function clear(x: undefined): number {
  return 2;
}
function f(o: Thing | undefined): number {
  switch (o?.foo) {
    case clear((o = undefined)):
      return 0;
    case 1:
      return o.foo + 1;
  }
  return -1;
}
function main(): void {
  f({ foo: 1 });
}
