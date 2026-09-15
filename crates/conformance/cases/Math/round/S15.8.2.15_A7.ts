// test262: test/built-ins/Math/round/S15.8.2.15_A7.js
// expect-fail: Math.round compiles to floor(x + 0.5), which rounds 0.5 - 2**-54 up to 1 and bumps odd integers in [2**52 + 1, 2**53 - 1] (and their negatives) to the next even integer
// Checks 1-3 of the original (round keeps -0 for -0.5 <= x <= -0) are the
// documented sign-of-zero divergence and live in rejected/Math/round/.

function main(): void {
  let x: number = 0.5 - Number.EPSILON / 4;
  assertSameValue(1 / Math.round(x), 1 / 0,
    "The result of evaluating (1 / Math.round(x)) is expected to be 1 / 0");

  x = -(2 / Number.EPSILON - 1);
  assertSameValue(Math.round(x), x, "Math.round(-(2 / Number.EPSILON - 1)) returns x");

  x = -(1.5 / Number.EPSILON - 1);
  assertSameValue(Math.round(x), x, "Math.round(-(1.5 / Number.EPSILON - 1)) returns x");

  x = -(1 / Number.EPSILON + 1);
  assertSameValue(Math.round(x), x, "Math.round(-(1 / Number.EPSILON + 1)) returns x");

  x = 1 / Number.EPSILON + 1;
  assertSameValue(Math.round(x), x, "Math.round(1 / Number.EPSILON + 1) returns x");

  x = 1.5 / Number.EPSILON - 1;
  assertSameValue(Math.round(x), x, "Math.round(1.5 / Number.EPSILON - 1) returns x");

  x = 2 / Number.EPSILON - 1;
  assertSameValue(Math.round(x), x, "Math.round(2 / Number.EPSILON - 1) returns x");
}
