function main(): void {
  const power = 2 ** 128;
  const expected = 340282366920938463463374607431768211456n;
  assert(BigInt(power) === expected, "large primitive number converts exactly");
  assert(BigInt(-power) === -expected, "large negative number converts exactly");
  assert(BigInt(2 ** 1023) === 2n ** 1023n, "largest exponent stays exact");
  assert(BigInt(-0) === 0n, "negative zero becomes zero");
  let refused = false;
  try { BigInt(1.5); } catch (error) { refused = error instanceof RangeError; }
  assert(refused, "fractional number remains a RangeError");
}
