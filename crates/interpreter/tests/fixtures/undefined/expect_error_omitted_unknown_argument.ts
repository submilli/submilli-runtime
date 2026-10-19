// expect-error: expected 2 argument(s), got 1
// Only `void` makes a parameter omittable: `unknown` and `undefined` still
// expect an argument, as in TypeScript.
function u(a: number, b: unknown): void {}
function main(): void {
  u(1);
}
