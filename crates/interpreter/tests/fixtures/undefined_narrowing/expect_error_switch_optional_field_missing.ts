// expect-error: function `f` does not return a value on all paths
// expect-error-count: 1
// A `switch` on an optional literal field, not the union's tag, can still
// fall through on `undefined`, as tsc reports (TS2366).
type S = { code?: 1; kind: "a" } | { code?: 2; kind: "b" };
function f(v: S): string {
  switch (v.code) {
    case 1:
      return "one";
    case 2:
      return "two";
  }
}
function main(): void {
  f({ kind: "a" });
}
