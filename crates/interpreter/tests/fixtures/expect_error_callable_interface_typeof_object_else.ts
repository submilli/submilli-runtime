// A call-signature interface can hold both a `$Closure` (`typeof === "function"`)
// and an `{"@call": ...}` object (`typeof === "object"`), so `typeof x === "object"`
// does not eliminate `BooleanConstructor` from the `else` branch.
// expect-error: cannot read field `toUpperCase` on non-object type `string | BooleanConstructor`
// expect-error-count: 1
function badNarrow(x: BooleanConstructor | string): string {
  if (typeof x === "object") {
    return "obj";
  }
  return x.toUpperCase();
}

function main(): void {
  badNarrow("ok");
}
