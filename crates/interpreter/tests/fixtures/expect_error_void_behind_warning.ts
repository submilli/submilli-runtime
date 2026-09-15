// A `void` in a value slot must still be reported when the same expression
// also raised a *warning*. Errors abort before codegen, so a warning is the
// only diagnostic that can both appear and let compilation continue — which is
// exactly when suppressing the screen would let the `void` reach codegen.
// expect-error: `void` cannot be a field value — it has no values
// expect-error: `void` cannot be an array element — it has no values
function nothing(): void {}

function main(): void {
  const n: number = 1;
  const o = { f: n ?? nothing() };
  const a = [n ?? nothing()];
}
