// expect-error: expected `void`, got `5`
// expect-error-count: 1
// A `void` result has no overlap with a number, as tsc reports (TS2367);
// comparing it with `undefined` is fine.
function v(): void {}
function main(): void {
  console.log(v() === undefined);
  console.log(v() === 5);
}
