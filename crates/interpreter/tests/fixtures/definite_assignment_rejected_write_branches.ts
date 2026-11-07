// expect-error: expected `number`, got `number | undefined`
// expect-error: expected `number`, got `string | undefined`
// expect-error-count: 2
function choose(counts: Record<string, number>, labels: Record<string, string>, chooseA: boolean): number {
  let plays: number;
  if (chooseA) {
    plays = counts["a"];
  } else {
    plays = labels["b"];
  }
  return plays;
}

function main(): number {
  return choose({}, {}, true);
}
