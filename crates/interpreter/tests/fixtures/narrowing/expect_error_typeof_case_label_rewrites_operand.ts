// expect-error: cannot read field `toUpperCase`
// expect-error-count: 1
// A case label that writes the operand of `switch (typeof x)` leaves the
// matched tag a fact about the value `x` held before.
function tag(x: string | number): "string" {
  return "string";
}
function f(): string {
  let x: string | number = "a";
  switch (typeof x) {
    case tag((x = 1)):
      return x.toUpperCase();
  }
  return "";
}
function main(): void {
  f();
}
