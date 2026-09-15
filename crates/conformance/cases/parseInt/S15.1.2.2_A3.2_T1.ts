// test262: test/built-ins/parseInt/S15.1.2.2_A3.2_T1.js

function main(): void {
  assertSameValue(parseInt("11", NaN), parseInt("11", 10), 'parseInt("11", NaN) must return the same value returned by parseInt("11", 10)');
  assertSameValue(parseInt("11", 0), parseInt("11", 10), 'parseInt("11", +0) must return the same value returned by parseInt("11", 10)');
  assertSameValue(parseInt("11", -0), parseInt("11", 10), 'parseInt("11", -0) must return the same value returned by parseInt("11", 10)');

  assertSameValue(
    parseInt("11", Number.POSITIVE_INFINITY),
    parseInt("11", 10),
    'parseInt("11", Number.POSITIVE_INFINITY) must return the same value returned by parseInt("11", 10)',
  );

  assertSameValue(
    parseInt("11", Number.NEGATIVE_INFINITY),
    parseInt("11", 10),
    'parseInt("11", Number.NEGATIVE_INFINITY) must return the same value returned by parseInt("11", 10)',
  );
}
