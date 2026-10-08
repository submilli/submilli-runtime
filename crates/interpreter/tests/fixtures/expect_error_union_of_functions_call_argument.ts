// The combined signature of a union of function types takes only what every
// member accepts: a parameter that is `number` in one member and `string` in
// another accepts nothing, so tsc rejects any argument there.
// expect-error: cannot call value of type
// expect-error-count: 1
function either(f: ((a: number) => void) | ((a: string) => void)): void {
  f(1);
}

function main(): void {
  either((a: number) => {});
}
