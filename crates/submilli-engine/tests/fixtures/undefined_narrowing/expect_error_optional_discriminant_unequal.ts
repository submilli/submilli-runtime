// expect-error: field `y` does not exist
// expect-error-count: 1
// An optional `kind?: "a"` is `"a"` or missing, so `kind !== "a"` can't rule
// that member out: a missing `kind` still lands on the unequal side, as in
// TypeScript.
type S = { kind?: "a"; x: number } | { kind: "b"; y: string };
function f(s: S): string {
  if (s.kind !== "a") {
    return s.y.toUpperCase();
  }
  return "a";
}
function main(): void {
  f({ x: 1 });
}
