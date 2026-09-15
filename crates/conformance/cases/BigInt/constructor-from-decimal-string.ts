// test262: test/built-ins/BigInt/constructor-from-decimal-string.js

function main(): void {
  assertSameValue(BigInt("10"), 10n);
  assertSameValue(BigInt("18446744073709551616"), 18446744073709551616n);
  assertSameValue(BigInt("7"), 7n);
  assertSameValue(BigInt("88"), 88n);
  assertSameValue(BigInt("900"), 900n);

  assertSameValue(BigInt("-10"), -10n);
  assertSameValue(BigInt("-18446744073709551616"), -18446744073709551616n);
  assertSameValue(BigInt("-7"), -7n);
  assertSameValue(BigInt("-88"), -88n);
  assertSameValue(BigInt("-900"), -900n);
}
