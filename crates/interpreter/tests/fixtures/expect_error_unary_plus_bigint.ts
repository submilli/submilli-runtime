// JavaScript's unary `+` throws a TypeError on a bigint, so it is rejected (tsc:
// TS2736), alone or in a union. Unary `-` negates a bigint.
// expect-error-count: 2
// expect-error: unary `+` not defined for `bigint`
// expect-error: `Number(b)` converts it
// expect-error: unary `+` not defined for `number | bigint`
function main(): void {
  const b: bigint = 5n;
  const n = +b;
  const u: number | bigint = Math.random() > 2 ? 1 : 2n;
  const m = +u;
  const neg: bigint = -b;
}
