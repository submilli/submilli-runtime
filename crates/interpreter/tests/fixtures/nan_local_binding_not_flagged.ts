// Only the global `NaN` makes a comparison always false. A local of that name
// is an ordinary value, and `Number.NaN` isn't checked, as in tsc.
function equalsLocal(NaN: number, x: number): boolean {
  return x === NaN;
}

function main(): void {
  assert(equalsLocal(2, 2), "a parameter named `NaN` compares by value");
  const n = Number.NaN;
  assert(!(n === Number.NaN), "`Number.NaN` is unchecked and never equal");
}
