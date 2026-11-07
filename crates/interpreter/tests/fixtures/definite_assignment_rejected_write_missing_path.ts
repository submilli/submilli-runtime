// expect-error: expected `number`, got `number | undefined`
// expect-error: variable `plays` is used before being assigned
// expect-error-count: 2
function choose(counts: Record<string, number>, assign: boolean): number {
  let plays: number;
  if (assign) {
    plays = counts["a"];
  }
  return plays;
}

function main(): number {
  return choose({}, false);
}
