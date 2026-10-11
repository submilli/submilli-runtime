// expect-error: function `f` does not return a value on all paths
// expect-error-count: 1
// An optional discriminant may be absent, so a `switch` covering every
// literal can still fall through on `undefined`, as tsc reports (TS2366).
type U = { k?: "a"; a: number } | { k: "b"; b: number };
function f(u: U): number {
  switch (u.k) {
    case "a":
      return u.a;
    case "b":
      return u.b;
  }
}
function main(): void {
  f({ k: "b", b: 1 });
}
