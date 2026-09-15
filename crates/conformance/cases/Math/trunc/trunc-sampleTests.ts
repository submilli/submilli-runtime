// test262: test/built-ins/Math/trunc/trunc-sampleTests.js

function main(): void {
  assertSameValue(1 / Math.trunc(0.02047410048544407), Number.POSITIVE_INFINITY,
    "Math.trunc should produce +0 for values between 0 and 1");
  assertSameValue(1 / Math.trunc(0.00000000000000001), Number.POSITIVE_INFINITY,
    "Math.trunc should produce +0 for values between 0 and 1");
  assertSameValue(1 / Math.trunc(0.9999999999999999), Number.POSITIVE_INFINITY,
    "Math.trunc should produce +0 for values between 0 and 1");
  assertSameValue(1 / Math.trunc(Number.EPSILON), Number.POSITIVE_INFINITY,
    "Math.trunc should produce +0 for values between 0 and 1");
  assertSameValue(1 / Math.trunc(Number.MIN_VALUE), Number.POSITIVE_INFINITY,
    "Math.trunc should produce +0 for values between 0 and 1");

  assertSameValue(1 / Math.trunc(-0.02047410048544407), Number.NEGATIVE_INFINITY,
    "Math.trunc should produce -0 for values between -1 and 0");
  assertSameValue(1 / Math.trunc(-0.00000000000000001), Number.NEGATIVE_INFINITY,
    "Math.trunc should produce -0 for values between -1 and 0");
  assertSameValue(1 / Math.trunc(-0.9999999999999999), Number.NEGATIVE_INFINITY,
    "Math.trunc should produce -0 for values between -1 and 0");
  assertSameValue(1 / Math.trunc(-Number.EPSILON), Number.NEGATIVE_INFINITY,
    "Math.trunc should produce -0 for values between -1 and 0");
  assertSameValue(1 / Math.trunc(-Number.MIN_VALUE), Number.NEGATIVE_INFINITY,
    "Math.trunc should produce -0 for values between -1 and 0");

  assertSameValue(Math.trunc(Number.MAX_VALUE), Math.floor(Number.MAX_VALUE),
    "Math.trunc produces incorrect result for Number.MAX_VALUE");
  assertSameValue(Math.trunc(10), Math.floor(10),
    "Math.trunc produces incorrect result for 10");
  assertSameValue(Math.trunc(3.9), Math.floor(3.9),
    "Math.trunc produces incorrect result for 3.9");
  assertSameValue(Math.trunc(4.9), Math.floor(4.9),
    "Math.trunc produces incorrect result for 4.9");

  assertSameValue(Math.trunc(-Number.MAX_VALUE), Math.ceil(-Number.MAX_VALUE),
    "Math.trunc produces incorrect result for -Number.MAX_VALUE");
  assertSameValue(Math.trunc(-10), Math.ceil(-10),
    "Math.trunc produces incorrect result for -10");
  assertSameValue(Math.trunc(-3.9), Math.ceil(-3.9),
    "Math.trunc produces incorrect result for -3.9");
  assertSameValue(Math.trunc(-4.9), Math.ceil(-4.9),
    "Math.trunc produces incorrect result for -4.9");
}
