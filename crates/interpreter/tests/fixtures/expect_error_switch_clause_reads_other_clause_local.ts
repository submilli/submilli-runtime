// A `let`/`const` of one clause is declared only when that clause runs, so
// another clause, which may be entered directly, can't use it.
// expect-error: `x` is declared in another `case` clause
// expect-error-count: 1
function pick(kind: number): string {
  switch (kind) {
    case 1:
      const x = "one";
      return x;
    default:
      return x;
  }
}
function main(): void {}
