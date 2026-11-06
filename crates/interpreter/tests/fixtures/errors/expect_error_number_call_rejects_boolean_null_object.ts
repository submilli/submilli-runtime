// SUB-2102: `Number(value)` takes a number, string, bigint or undefined.
// Booleans, `null` and objects stay out, because JavaScript's conversions of
// them surprise: `Number(null)` and `Number([])` are 0. tsc accepts them all.
// expect-error-count: 3
// expect-error: got `boolean`
// expect-error: got `null`
// expect-error: got `number[]`
function main(): void {
  console.log(Number(true), Number(null), Number([1]));
}
