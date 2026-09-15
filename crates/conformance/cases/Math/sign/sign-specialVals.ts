// test262: test/built-ins/Math/sign/sign-specialVals.js

function main(): void {
  assertSameValue(Math.sign(NaN), NaN, "NaN");
  assertSameValue(Math.sign(-0), -0, "-0");
  assertSameValue(Math.sign(0), 0, "0");

  assertSameValue(Math.sign(-0.000001), -1, "-0.000001");
  assertSameValue(Math.sign(-1), -1, "-1");
  assertSameValue(Math.sign(-Infinity), -1, "-Infinity");

  assertSameValue(Math.sign(0.000001), 1, "0.000001");
  assertSameValue(Math.sign(1), 1, "1");
  assertSameValue(Math.sign(Infinity), 1, "Infinity");
}
