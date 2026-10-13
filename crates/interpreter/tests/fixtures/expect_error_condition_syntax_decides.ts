// A condition whose syntax alone decides it is a mistake, as tsc reports:
// always truthy (TS2872) or always falsy (TS2873). `0` and `1` stay legal, as
// in `while (1)`, and a value whose type alone decides the test is not judged.
// expect-error: this kind of expression is always truthy
// expect-error: this kind of expression is always falsy
// expect-error-count: 8
function main(): void {
  const n: number = 2;
  if ({}) {}
  if (!"") {}
  const a = [] ? 1 : 2;
  while (null) {}
  const b = (() => 1) || 3;
  const c = undefined && n;
  do {} while ("x");
  const d = n > 0 ? "a" : "b";
  if (d || "c") {}
  if (n ? {} : []) {}
  while (1) { break; }
  const t = true;
  if (t) {}
  console.log(a, b, c);
}
