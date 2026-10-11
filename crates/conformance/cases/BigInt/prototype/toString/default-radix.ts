// test262: test/built-ins/BigInt/prototype/toString/default-radix.js

function main(): void {
  assertSameValue((-100n).toString(), "-100", "(-100n).toString() === '-100'");
  assertSameValue((0n).toString(), "0", "(0n).toString() === '0'");
  assertSameValue((100n).toString(), "100", "(100n).toString() === '100'");

  assertSameValue((-100n).toString(undefined), "-100",
                  "(-100n).toString(undefined) === '-100'");
  assertSameValue((0n).toString(undefined), "0",
                  "(0n).toString(undefined) === '0'");
  assertSameValue((100n).toString(undefined), "100",
                  "(100n).toString(undefined) === '100'");
}
