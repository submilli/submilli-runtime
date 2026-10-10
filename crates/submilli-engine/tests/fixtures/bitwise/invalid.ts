// expect-error: `>>>` not defined for `bigint` and `bigint`
// expect-error: `&` not defined for `number` and `bigint`
// expect-error: `&` not defined for `boolean` and `boolean`
function main(): void {
  const a = 1n >>> 1n;
  const b = 1 & 1n;
  const c = true & false;
}
