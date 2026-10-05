// A `let` copying a literal type that came from an annotation, an assertion or
// a declared field keeps it, as in TypeScript, so a different literal can't be
// assigned to it later.
// expect-error: expected `"hello"`, got `"other"`
// expect-error: expected `0 | 1`, got `2`
// expect-error: expected `"on"`, got `"off"`
// expect-error-count: 3
interface Bit {
  value: 0 | 1;
}

function main(): void {
  const declared: "hello" = "hello";
  let copy = declared;
  copy = "other";
  const bit: Bit = { value: 1 };
  let value = bit.value;
  value = 2;
  let asserted = "on" as "on";
  asserted = "off";
}
