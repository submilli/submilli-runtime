// Compound assignment accepts a literal operand, like `+` itself: `count += e` where
// `e` is a literal-typed `const` is ordinary arithmetic. Found by the full fixture
// sweep, which the default smoke subset does not run.
function main(): void {
  const step = 7;
  let count = 0;
  count += step;
  count -= 2;
  count *= 2;

  let label = "a";
  const suffix = "b";
  label += suffix;

  assert(count === 10, "numeric compound assignment");
  assert(label === "ab", "string compound assignment");
}
