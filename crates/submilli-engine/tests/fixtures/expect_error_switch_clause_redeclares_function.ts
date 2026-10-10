// The clauses of a `switch` share one scope, as in JavaScript (TS2393).
// expect-error: binding `f` is already declared in this scope
// expect-error-count: 1
function pick(kind: number): string {
  switch (kind) {
    case 1:
      function f(): string {
        return "one";
      }
      return f();
    default:
      function f(): string {
        return "other";
      }
      return f();
  }
}
function main(): void {}
