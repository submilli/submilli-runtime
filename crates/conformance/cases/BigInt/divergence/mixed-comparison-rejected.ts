// Divergence pin, derived from test262: test/built-ins/BigInt/prototype/toString/a-z.js
// (its loop compares a bigint counter against a number radix). number and
// bigint never mix implicitly — comparisons included; convert explicitly
// with BigInt(n) or Number(b).
// expect-error: `<` not defined for `bigint` and `number`

function main(): void {
  const i: bigint = 10n;
  const radix: number = 36;
  assert(i < radix, "unreachable");
}
