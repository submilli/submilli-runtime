// `typeof v === "undefined"` narrows a `void` member to `undefined`. The other
// branch keeps `void`: a value-returning function stored as a `void` one
// returns its value, so no test proves a `void` result absent. A `typeof`
// test for the other member's tag is what narrows to it.
function inspect(value: boolean | void): string {
  if (typeof value === "undefined") {
    const missing: undefined = value;
    return "missing";
  }
  if (typeof value === "boolean") {
    const present: boolean = value;
    return present ? "true" : "false";
  }
  return "other";
}
function nothing(): void {}
function main(): void {
  assert(inspect(nothing()) === "missing");
  assert(inspect(true) === "true");
  assert(inspect(false) === "false");
  const stored: () => void = () => 5;
  assert(inspect(stored()) === "other", "a value a `void` function returns");
}
