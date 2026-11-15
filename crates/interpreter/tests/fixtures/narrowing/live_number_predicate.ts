// expect-error-count: 1
// expect-error: expected `number`, got `number | null`
// A call in the condition invalidates the preceding module-variable guard.
let current: number | null = 3;
function clear(): boolean { current = null; return false; }
function read(): number { if (current === null || clear()) return 9; return current; }
function main(): void {
  const value = read();
  assert(!Number.isInteger(value));
  assert(!Number.isSafeInteger(value));
  assert(!Number.isFinite(value));
  assert(!Number.isNaN(value));
}
