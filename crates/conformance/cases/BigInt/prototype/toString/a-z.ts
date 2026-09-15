// test262: test/built-ins/BigInt/prototype/toString/a-z.js
// The original loops a bigint counter compared against the number radix;
// the no-implicit-mixing rule (pinned in cases/BigInt/divergence/) turns
// that into a number counter converted explicitly.

function main(): void {
  for (let radix = 11; radix <= 36; radix++) {
    for (let i = 10; i < radix; i++) {
      assertSameValue(
        BigInt(i).toString(radix),
        String.fromCharCode(i + 87),
        `digit ${i}, radix ${radix}`,
      );
    }
  }
}
