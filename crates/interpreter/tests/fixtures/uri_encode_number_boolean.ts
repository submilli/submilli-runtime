// SUB-1760: encodeURIComponent takes `string | number | boolean`, as in tsc,
// and converts the way `String(value)` does. encodeURI takes only a string.
function encodeEither(value: string | number): string {
  return encodeURIComponent(value);
}
function main(): void {
  assert(encodeURIComponent(5) === "5");
  assert(encodeURIComponent(-1.5) === "-1.5");
  assert(encodeURIComponent(1e21) === "1e%2B21");
  assert(encodeURIComponent(true) === "true");
  assert(encodeURIComponent(0.1) === "0.1");
  assert(encodeURI("a b/c") === "a%20b/c");
  assert(encodeURIComponent("a b&c") === "a%20b%26c");
  assert(encodeEither("x/y") === "x%2Fy" && encodeEither(42) === "42");
  assert(decodeURIComponent(encodeURIComponent(12)) === "12");
}
