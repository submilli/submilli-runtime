// test262: test/built-ins/BigInt/prototype/toString/string-is-code-units-of-decimal-digits-only.js
// The BigInt(0n) row is dropped: BigInt() takes string | number here.

function main(): void {
  assertSameValue(BigInt(0).toString(), "0", "BigInt(0).toString() === '0'");
  assertSameValue((0n).toString(), "0", "0n.toString() === '0'");
}
