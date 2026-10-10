// `first` calls `second`, which reads `base`, declared below both: `first` can
// only be called once `base` is.
// expect-error: `first` is used before `base`, which `second` uses, is declared
// expect-error-count: 1
function main(): void {
  function first(): number {
    return second();
  }
  function second(): number {
    return base;
  }
  const r = first();
  const base = 1;
}
