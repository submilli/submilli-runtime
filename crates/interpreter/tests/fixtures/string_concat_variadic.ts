// SUB-1752: `String#concat` takes any number of strings, as in tsc.
function main(): void {
  assert("a".concat("b", "c") === "abc");
  assert("x".concat() === "x");
  assert("".concat("😀", "") === "😀");
}
