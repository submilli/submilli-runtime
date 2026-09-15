// expect-error: `void` cannot be used as a type argument to `run`
// A type argument is erased into a value slot, and `void` has no value
// representation there — the same rejection the class path already makes,
// applied at a generic call site. Without it codegen panics in `emit_cast_to`.
//
// `T` here is return-only, so this is also the case the eventual per-parameter
// position analysis is meant to *accept*. Until that exists the blanket refusal
// is what ships, and this fixture pins it — expect to revisit the file then.
function run<T>(f: (x: number) => T, v: number): T {
  return f(v);
}

function main(): void {
  run<void>((x: number): void => {}, 1);
}
