// test262: test/built-ins/BigInt/constructor-from-hex-string.js
// expect-fail: StringToBigInt accepts 0x/0X-prefixed hex strings; BigInt(string) parses decimal only and throws on the prefix

function main(): void {
  assertSameValue(BigInt("0xa"), 10n);
  assertSameValue(BigInt("0xff"), 255n);
  assertSameValue(BigInt("0xfabc"), 64188n);
  assertSameValue(BigInt("0xfffffffffffffffffff"), 75557863725914323419135n);

  assertSameValue(BigInt("0Xa"), 10n);
  assertSameValue(BigInt("0Xff"), 255n);
  assertSameValue(BigInt("0Xfabc"), 64188n);
  assertSameValue(BigInt("0Xfffffffffffffffffff"), 75557863725914323419135n);
}
