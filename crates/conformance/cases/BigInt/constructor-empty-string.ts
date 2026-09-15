// test262: test/built-ins/BigInt/constructor-empty-string.js
// expect-fail: StringToBigInt maps empty/whitespace-only strings to 0n; BigInt("") throws "invalid bigint literal"

function main(): void {
  assertSameValue(BigInt(""), 0n);
  assertSameValue(BigInt(" "), 0n);
}
