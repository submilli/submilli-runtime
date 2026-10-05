// The clauses of a `switch` share one scope, as in JavaScript (TS2451).
// expect-error: binding `x` is already declared in this scope
// expect-error-count: 1
function pick(kind: number): string {
  switch (kind) {
    case 1:
      const x = "one";
      return x;
    default:
      const x = "other";
      return x;
  }
}
function main(): void {}
