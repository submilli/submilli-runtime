// A plain function lacks `NumberConstructor`'s static properties and methods,
// so it does not satisfy `NumberConstructor`.
// expect-error: expected `NumberConstructor`, got `(arg0: unknown) => number`
// expect-error-count: 1
function zero(_v: unknown): number {
  return 0;
}

function main(): void {
  const _N: NumberConstructor = zero;
}
