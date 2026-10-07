// expect-error-count: 4
// expect-error: this comparison is always `false`: `NaN` is not equal to any value
// expect-error: this comparison is always `true`: `NaN` is not equal to any value
// expect-error: use `Number.isNaN(x)` to test for `NaN`
function main(): void {
  const x: number = 1;
  console.log(x === NaN, NaN === NaN);
  console.log((NaN) !== x);
  console.log(x === Number.NaN, Number.isNaN(x));
  const y: number | null = null;
  console.log(y === NaN);
}
