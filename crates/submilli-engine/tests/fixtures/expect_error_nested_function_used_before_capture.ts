// `add` captures `base`, so it exists only once `base` is declared. JavaScript
// would throw a ReferenceError here at runtime.
// expect-error: `add` is used before `base`, which it uses, is declared
// expect-error-count: 1
function main(): void {
  const r = add(1);
  const base = 1;
  function add(x: number): number {
    return x + base;
  }
}
