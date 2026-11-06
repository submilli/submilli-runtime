// An element read from an array literal of mixed elements holds their union,
// so a use only one of them allows needs narrowing first, as in tsc.
// expect-error: expected `number`, got `number | string`
// expect-error: cannot read field `toUpperCase` on non-object type `number | string`
// expect-error-count: 2
function main(): void {
  const row = [1, "x"];
  const id: number = row[0];
  const upper = row[1].toUpperCase();
  const cell = row[1];
  if (typeof cell === "string") console.log(cell.toUpperCase(), id, upper);
}
