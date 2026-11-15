// expect-error-count: 1
// expect-error: expected `number`, got `number | null`
// A call in the condition invalidates the preceding module-variable guard.
let current: number | null = 3;
function clear(): boolean { current = null; return false; }
function read(): number {
  if (current === null || clear()) return 9;
  return current;
}
function previous(value: number): number { return value++; }
function main(): void { assert(previous(read()) === 0); }
