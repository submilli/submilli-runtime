// `bigint` is wider than `1n`, as in TypeScript (TS2322).
// expect-error: expected `1n`, got `bigint`

function main(): void {
  const b: bigint = BigInt.fromString("1");
  const one: 1n = b;
  console.log(one);
}
