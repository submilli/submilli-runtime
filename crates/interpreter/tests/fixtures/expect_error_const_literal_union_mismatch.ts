// A `const` bound to a `?:` of literals has exactly those literal types, so a
// narrower target rejects it, as in TypeScript.
// expect-error: expected `"a"`, got `"a" | "b"`
// expect-error: expected `1`, got `1 | 2`
// expect-error: expected `"x"`, got `"x" | "y" | "z"`
// expect-error-count: 3
function pick(): boolean {
  return "ab".length === 2;
}

function main(): void {
  const cond = pick();
  const letter = cond ? "a" : "b";
  const onlyA: "a" = letter;
  const digit = cond ? 1 : 2;
  const onlyOne: 1 = digit;
  const nested = cond ? (cond ? "x" : "y") : "z";
  const onlyX: "x" = nested;
}
