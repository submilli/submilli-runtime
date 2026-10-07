// expect-error: expected `}` to close interface
// expect-error-count: 1
// An unclosed interface stops at an exported declaration, which no member can
// start, so the function after it still parses.
interface I {
  a: number;

export function f(): number {
  return 1;
}

function main(): void {
  f();
}
