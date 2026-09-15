// test262: test/built-ins/Math/ceil/S15.8.2.6_A6.js

function main(): void {
  assertSameValue(Math.ceil(-0.000000000000001), -0, "-0.000000000000001");
  assertSameValue(Math.ceil(-0.999999999999999), -0, "-0.999999999999999");
  assertSameValue(Math.ceil(-0.5), -0, "-0.5");
}
