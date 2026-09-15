// test262: test/built-ins/Math/pow/int32_min-exponent.js

function main(): void {
  const INT32_MIN = -2147483648;

  assertSameValue(Math.pow(2, INT32_MIN), 0,
    "Math.pow(2, -(gonzo huge exponent > 1074)) should be +0 " +
    "because 2**-1074 is the smallest positive IEEE-754 number");

  assertSameValue(Math.pow(1, INT32_MIN), 1,
    "1**-(gonzo huge exponent > 1074) should be 1");
}
