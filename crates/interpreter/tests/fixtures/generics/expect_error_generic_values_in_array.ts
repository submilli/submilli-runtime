// An array of generic functions can't become an array of plain ones: its
// elements would be called without type arguments (tsc accepts).
// expect-error: expected `((arg0: string) => number | undefined)[]`, got `(<T>(arg0: string) => T | undefined)[]`
// expect-error-count: 1
const load = <T,>(key: string): T | undefined => undefined;

function main(): void {
  const all = [load];
  const numbers: ((key: string) => number | undefined)[] = all;
}
