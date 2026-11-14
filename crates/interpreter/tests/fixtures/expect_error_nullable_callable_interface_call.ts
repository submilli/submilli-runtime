// Calling a nullable call-signature interface directly reports the optional-call
// hint (`f?.(…)`).
// expect-error: cannot call value of type `BooleanConstructor | undefined`
// expect-error-count: 1
function run(f: BooleanConstructor | undefined): void {
  f(1);
}

function main(): void {
  run(Boolean);
}
