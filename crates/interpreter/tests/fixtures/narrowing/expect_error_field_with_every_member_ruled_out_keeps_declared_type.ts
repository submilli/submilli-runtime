// expect-error: expected `never`, got `E`
// Every member of a field ruled out still leaves its declared type, since an
// alias or a call may change a field behind the checks. TypeScript reads `never`.
enum E { A, B }
function never(x: never): number { return 0; }
function f(o: { e: E }): number {
  if (o.e === E.A) return 1;
  if (o.e === E.B) return 2;
  return never(o.e);
}
function main(): void { console.log(f({ e: E.A })); }
