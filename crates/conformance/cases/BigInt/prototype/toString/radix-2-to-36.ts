// test262: test/built-ins/BigInt/prototype/toString/radix-2-to-36.js

function main(): void {
  for (let r = 2; r <= 36; r++) {
    assertSameValue((0n).toString(r), "0", `0, radix ${r}`);
    assertSameValue((-1n).toString(r), "-1", `-1, radix ${r}`);
    assertSameValue((1n).toString(r), "1", `1, radix ${r}`);
  }
}
