// expect-error-count: 1
// expect-error: `concat` spreads `number[]` into elements of type `number[]`
// `concat` spreads an array argument, so on `number[][]` the array `[1, 2]`
// would append the numbers 1 and 2. tsc accepts the call through its element
// overload; Submilli reports it, since the result would hold numbers typed as
// arrays.
function main(): void {
  const rows: number[][] = [[0]];
  const nested = rows.concat([[1]], [[2], [3]]);
  const wrong = rows.concat([1, 2]);
}
