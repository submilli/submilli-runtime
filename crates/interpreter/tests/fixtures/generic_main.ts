// A generic `main` runs: the entry call passes `unknown` for a type
// parameter with no bound.
function main<T>(): number {
  const items: T[] = [];
  assert(items.length === 0, "a generic main runs");
  return 0;
}
