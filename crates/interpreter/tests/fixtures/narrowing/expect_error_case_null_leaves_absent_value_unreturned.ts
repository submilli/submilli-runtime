// expect-error: function `f` does not return a value on all paths
// expect-error-count: 1
// A copy of an optional field may be `undefined`, which no case here matches.
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
}
function main(): void {
  assert(f({ kind: "a" }) === 1);
}
