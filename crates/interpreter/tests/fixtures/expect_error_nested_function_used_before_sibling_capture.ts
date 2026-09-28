// `first` calls `second`, which captures `base`: `first` exists as soon as it
// is declared, but can only be called once `base` is.
// expect-error: `first` is used before `base`, which `second` uses, is declared
// expect-error-count: 1
function main(): void {
  function first(): number {
    return second();
  }
  const r = first();
  const base = 1;
  function second(): number {
    return base;
  }
}
