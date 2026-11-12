// A function whose parameter type is narrower than the call-signature
// interface's parameter type does not satisfy it.
// expect-error: expected `BooleanConstructor`, got `(arg0: number) => boolean`
// expect-error-count: 1
function positive(n: number): boolean {
  return n > 0;
}

function main(): void {
  const _B: BooleanConstructor = positive;
}
