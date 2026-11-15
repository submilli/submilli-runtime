// An optional call doesn't infer a generic function's type arguments, so it
// is rejected rather than run without them.
// expect-error: in an optional chain is not supported yet
// expect-error-count: 1
function call(f: (<T>(x: T) => T) | undefined): void {
  const r = f?.("abc");
}

function main(): void {}
