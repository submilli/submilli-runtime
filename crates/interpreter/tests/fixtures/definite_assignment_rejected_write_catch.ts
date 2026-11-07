// expect-error: expected `number`, got `number | undefined`
// expect-error-count: 1
function main(): number {
  const counts: Record<string, number> = {};
  let plays: number;
  try {
    plays = counts["a"];
  } catch {
    return 0;
  }
  return plays;
}
