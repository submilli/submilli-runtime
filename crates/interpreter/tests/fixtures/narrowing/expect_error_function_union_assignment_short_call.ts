// A function assigned to a union of function types is called with the
// longest parameter list, so a shorter call is rejected, as in TypeScript.
// expect-error: expected 2 argument(s), got 1
// expect-error-count: 1
type One = (x: number) => number;
type Two = (x: number, y: number) => number;

function main(): void {
  let f: One | Two | null = null;
  f = (x: number): number => x + 1;
  console.log(f(1));
}
