// expect-error-count: 2
// expect-error: expected `number`, got `number | null`
// A call in the condition invalidates the preceding module-variable guard.
let current: number | null = 3;
function clear(): boolean { current = null; return false; }
function assigned(): number {
  let saved = 1;
  if (current === null || clear()) { return 0; }
  return (saved = current);
}
function main(): void {
  const result: unknown = assigned();
  assert(result === null);
}
