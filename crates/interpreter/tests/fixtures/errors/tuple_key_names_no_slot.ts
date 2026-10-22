// expect-error: field `NaN` does not exist on `[number, string]`
// A tuple has no element at a key that names no slot, as tsc reports
// (TS2493); Node would read `undefined`.
function pick(t: [number, string]): number | string {
  return t["NaN"];
}

function main(): void {
  console.log(pick([1, "a"]));
}
