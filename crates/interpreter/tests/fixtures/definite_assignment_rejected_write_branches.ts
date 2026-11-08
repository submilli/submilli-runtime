// expect-error: expected `number`, got `number | undefined`
// expect-error: expected `number`, got `string | undefined`
// expect-error-count: 2
function choose(counts: Map<string, number>, labels: Map<string, string>, chooseA: boolean): number {
  let plays: number;
  if (chooseA) {
    plays = counts.get("a");
  } else {
    plays = labels.get("b");
  }
  return plays;
}

function main(): number {
  return choose(new Map(), new Map(), true);
}
