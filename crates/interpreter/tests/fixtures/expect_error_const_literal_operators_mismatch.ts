// A `const` bound to an operator over literals has exactly those literal types,
// as in TypeScript, so a narrower target rejects it.
// expect-error: expected `"a"`, got `"a" | null`
// expect-error: expected `"y"`, got `"x" | "y"`
// expect-error: expected `"t"`, got `"t" | boolean`
// expect-error-count: 3
function pick(): boolean {
  return "ab".length === 2;
}

function main(): void {
  const cond = pick();
  const optional = cond ? "a" : null;
  const onlyA: "a" = optional;
  const chosen: "x" | null = cond ? null : "x";
  const fallback = chosen ?? "y";
  const onlyY: "y" = fallback;
  const andThen = cond && "t";
  const onlyT: "t" = andThen;
}
