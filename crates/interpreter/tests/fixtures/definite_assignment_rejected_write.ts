// expect-error: expected `number`, got `number | undefined`
// expect-error-count: 1
function main(): number {
  const counts = new Map<string, number>();
  let plays: number;
  plays = counts.get("a");
  return plays;
}
