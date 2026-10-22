// `case null` matches only `null`: a value read from an absent optional field,
// a missing `Map` key, or an index past the end is `undefined`, which falls
// through to `default` or past the switch, as in Node.
interface O {
  kind?: "a" | null;
}
function f(o: O): number {
  const k = o.kind;
  switch (k) {
    case "a":
      return 1;
    case null:
      return 2;
  }
  return 0;
}
function g(m: Map<string, number | null>): number {
  switch (m.get("x")) {
    case null:
      return 2;
    default:
      return 3;
  }
}
function h(xs: (number | null)[]): number {
  const v = xs.at(5);
  switch (v) {
    case null:
      return 2;
  }
  return 9;
}
function main(): void {
  assert(f({}) === 0);
  assert(f({ kind: null }) === 2);
  assert(g(new Map()) === 3);
  assert(g(new Map<string, number | null>([["x", null]])) === 2);
  assert(h([]) === 9);
}
