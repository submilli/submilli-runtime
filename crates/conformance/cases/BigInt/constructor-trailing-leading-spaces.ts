// test262: test/built-ins/BigInt/constructor-trailing-leading-spaces.js
// The "   0b1111" row rides the radix-prefix gap (constructor-from-hex-string.ts)
// and the "     " row the empty-string gap (constructor-empty-string.ts);
// the decimal rows are ported here.

function main(): void {
  assertSameValue(BigInt("18446744073709551616   "), 18446744073709551616n);
  assertSameValue(BigInt("   7   "), 7n);
  assertSameValue(BigInt("   -197   "), -197n);
}
