// expect-error: field `toFixed` does not exist on `string`
// expect-error-count: 1
// Every earlier clause's labels run before a clause matches, so a label that
// writes the operand of `switch (typeof x)` leaves the matched tag a fact
// about the value `x` held before.
function tag(x: string | number): "string" {
  return "string";
}
function f(t: string | number): number {
  let x: string | number = t;
  switch (typeof x) {
    case tag((x = "s")):
      return 0;
    case "number":
      return x.toFixed(1).length;
  }
  return -1;
}
function main(): void {
  f(1);
}
