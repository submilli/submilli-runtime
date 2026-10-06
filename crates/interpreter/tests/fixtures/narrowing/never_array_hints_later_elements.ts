// An array literal's first element types the rest, even when it is a
// `never[]` that holds no element: a later empty array takes `never[]` from
// it, at any depth and through a spread.
function emptyAfterNeverArray(empty: never[], grid: never[][], flag: boolean): number {
  const withEmpty = [empty, []];
  const withSpreadEmpty = [empty, ...[]];
  const nested = [grid, [[]], ...[[]]];
  const chosen = [empty, flag ? [] : []];
  return withEmpty.length + withSpreadEmpty.length + nested.length + chosen.length;
}

function main(): void {
  assert(emptyAfterNeverArray([], [[]], true) === 2 + 1 + 3 + 2, "empty arrays take a never[] hint");
}
