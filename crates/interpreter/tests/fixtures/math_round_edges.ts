// `Math.round` rounds without an intermediate `x + 0.5`, whose own rounding
// gave wrong results below 0.5 and for odd integers past 2^52.
function main(): void {
  assert(Math.round(0.49999999999999994) === 0, "largest double below 0.5");
  assert(Math.round(4503599627370497) === 4503599627370497, "odd integer past 2^52");
  assert(Math.round(-4503599627370497) === -4503599627370497, "negative odd integer past 2^52");
  assert(Math.round(9007199254740991) === 9007199254740991, "MAX_SAFE_INTEGER");
  assert(Math.round(2.5) === 3, "a tie rounds up");
  assert(Math.round(-2.5) === -2, "a negative tie rounds toward +Infinity");
  assert(Object.is(Math.round(-0.5), -0), "-0.5 rounds to -0");
  assert(Object.is(Math.round(-0.2), -0), "a small negative rounds to -0");
  assert(Object.is(Math.round(-0), -0), "-0 stays -0");
  assert(Object.is(Math.round(0.4), 0), "a small positive rounds to +0");
  assert(Math.round(-0.5000000000000001) === -1, "just below -0.5 rounds down");
  assert(Math.round(0.5) === 1, "0.5 rounds up");
  assert(Number.isNaN(Math.round(NaN)), "NaN stays NaN");
  assert(Math.round(-Infinity) === -Infinity, "-Infinity stays");
}
