// A `void` call reaching a value slot is a diagnostic naming the fix, not a
// codegen ICE. `void` has no runtime representation, so it satisfies only
// `void` — not `unknown`, and not an unbound type parameter.
// expect-error: expected `unknown`, got `void`
// expect-error: `void` is not a value
function nothing(): void {}

function f(): unknown {
  return nothing();
}

function main(): void {
  f();
}
