// A bigint literal has a literal type, as in TypeScript: `const b = 123n` is
// `123n`, and `1n` and `-1n` are types. A `let` widens to `bigint`.

type Sign = -1n | 0n | 1n;

function sign(x: bigint): Sign {
  if (x < 0n) {
    return -1n;
  }
  return x === 0n ? 0n : 1n;
}

function twice(x: 1n | 2n): bigint {
  return x * 2n;
}

function main(): void {
  const b = 123n;
  const kept: 123n = b;
  assert(kept === 123n, "a const keeps its literal type");
  assert(b + 1n === 124n, "a literal type does arithmetic as a bigint");
  assert(-b === -123n, "negation reads the literal as a bigint");
  assert(`${b}` === "123", "a literal type converts like a bigint");
  assert(b.toString(16) === "7b", "a literal type has bigint's methods");

  let widened = 1n;
  widened = widened + 5n;
  assert(widened === 6n, "a `let` widens to bigint");

  const negative = -1n;
  const negativeKept: -1n = negative;
  assert(negativeKept < 0n, "a negated literal keeps its literal type");

  const hex: 16n = 0x10n;
  assert(hex === 16n, "a hex literal is its decimal value");

  assert(sign(-5n) === -1n, "-1n");
  assert(sign(0n) === 0n, "0n");
  assert(sign(9n) === 1n, "1n");
  assert(twice(2n) === 4n, "a literal union parameter");

  const big = 123456789123456789012345678901234567890n;
  const bigKept: 123456789123456789012345678901234567890n = big;
  assert(bigKept.toString() === "123456789123456789012345678901234567890", "a large literal");
}
