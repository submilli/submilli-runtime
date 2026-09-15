// test262: test/built-ins/BigInt/prototype/toString/default-radix.js
// The original also passes `undefined` explicitly; there is no `undefined`
// here, so only the omitted-argument form exercises the default.

function main(): void {
  assertSameValue((-100n).toString(), "-100", "(-100n).toString() === '-100'");
  assertSameValue((0n).toString(), "0", "(0n).toString() === '0'");
  assertSameValue((100n).toString(), "100", "(100n).toString() === '100'");
}
