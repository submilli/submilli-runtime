// A value of a call-signature interface is a function, so `typeof x === "object"`
// rules `BooleanConstructor` out of the `then` branch, and the `else` branch
// keeps it, as in TypeScript.
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
