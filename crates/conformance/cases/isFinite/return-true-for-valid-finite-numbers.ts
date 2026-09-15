// test262: test/built-ins/isFinite/return-true-for-valid-finite-numbers.js

function main(): void {
  assertSameValue(isFinite(0), true, "0");
  assertSameValue(isFinite(-0), true, "-0");
  assertSameValue(isFinite(Math.pow(2, 53)), true, "Math.pow(2, 53)");
  assertSameValue(isFinite(-Math.pow(2, 53)), true, "-Math.pow(2, 53)");
  assertSameValue(isFinite(1), true, "1");
  assertSameValue(isFinite(-1), true, "-1");
  assertSameValue(isFinite(0.000001), true, "0.000001");
  assertSameValue(isFinite(-0.000001), true, "-0.000001");
  assertSameValue(isFinite(1e42), true, "1e42");
  assertSameValue(isFinite(-1e42), true, "-1e42");
}
