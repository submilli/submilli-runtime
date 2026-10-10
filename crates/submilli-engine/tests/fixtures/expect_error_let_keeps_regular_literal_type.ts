// A `let` copying a literal type that came from an annotation, an assertion or
// a declared field keeps it, as in TypeScript, so a different literal can't be
// assigned to it later. So does one a pattern or a loop takes from a declared
// value, and an object literal's property copying a declared field.
// expect-error: expected `"hello"`, got `"other"`
// expect-error: expected `0 | 1`, got `2`
// expect-error: expected `"on"`, got `"off"`
// expect-error: expected `"m"`, got `"n"`
// expect-error: expected `0 | 1`, got `3`
// expect-error: expected `"a"`, got `"z"`
// expect-error: expected `0 | 1`, got `4`
// expect-error: expected `0 | 1`, got `5`
// expect-error-count: 8
interface Bit {
  value: 0 | 1;
}

const moduleDeclared: "m" = "m";
let moduleCopy = moduleDeclared;

function main(): void {
  const declared: "hello" = "hello";
  let copy = declared;
  copy = "other";
  const bit: Bit = { value: 1 };
  let value = bit.value;
  value = 2;
  let asserted = "on" as "on";
  asserted = "off";
  moduleCopy = "n";

  let { value: destructured } = bit;
  destructured = 3;
  const pair: ["a", 1] = ["a", 1];
  let [first] = pair;
  first = "z";
  const bits: Bit[] = [bit];
  for (const each of bits) {
    let looped = each.value;
    looped = 4;
  }
  const plain: { value: 0 | 1 } = { value: 0 };
  const spread = { ...plain };
  spread.value = 5;
}
